//! Writing a document to disk without ever leaving a half-written file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lopdf::Document;

/// Saves `document` at `path` without ever leaving a half-written file there.
///
/// Writing straight into `path` would first empty the file and then fill it
/// again. If that stops halfway (disk full, a crash, the process killed), the
/// old contents are gone and the new ones are incomplete. Since `path` may be
/// one of the inputs, that would destroy the user's own file.
///
/// So the document goes to a temporary file in the same folder first. Only
/// once it is complete and on the disk does `rename` give it the name `path`,
/// replacing whatever was there. A rename does not copy any data: it points
/// the name at the other file, in one step. Anyone opening `path` gets either
/// the old file or the new one, never a mix. `std::fs::rename` replaces an
/// existing file on Linux, macOS and Windows alike.
///
/// `pub(crate)`: every module of this crate may call it, and no other crate
/// can. Callers outside get it through the operations that save.
pub(crate) fn save_atomically(document: &mut Document, path: &Path) -> io::Result<()> {
    let temporary = temporary_path_for(path)?;
    // `create_new` refuses to open a file that already exists, so we never
    // write over a file that happens to have the same name.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;

    // From here on the temporary file exists, so every failure must remove
    // it. Doing the steps in their own function lets `?` stop at the first
    // failure while we still get the chance to clean up here.
    let result = write_and_replace(document, file, &temporary, path);
    if result.is_err() {
        // Best effort: if the removal fails too, there is nothing better to
        // do, and the save error is the one worth reporting.
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Writes `document` into `file` (open at `temporary`), makes sure it is on
/// the disk, and renames it to `path`.
fn write_and_replace(
    document: &mut Document,
    file: File,
    temporary: &Path,
    path: &Path,
) -> io::Result<()> {
    // Writing a PDF means many small writes; `BufWriter` gathers them into
    // a few big ones.
    let mut writer = BufWriter::new(file);
    document.save_to(&mut writer)?;
    // `into_inner` writes out what is still in the buffer and hands the file
    // back. Its error also carries the writer; `into_error` keeps only the
    // `io::Error`, so `?` can return it.
    let file = writer
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    // `sync_all` waits until the bytes are really on the disk, not just in
    // the system's memory. Without it, a power cut just after the rename
    // could leave `path` naming a file that was never fully written.
    file.sync_all()?;
    // Close the file before the rename, so nothing holds it open while it
    // takes the place of `path`.
    drop(file);
    fs::rename(temporary, path)
}

/// A free name for the temporary file, in the same folder as `path`.
///
/// The same folder matters: a rename only works within one file system, and
/// another folder (like the system temp folder) may sit on another one.
///
/// For `merged.pdf` the name looks like `.merged.pdf.1234-0.tmp`. The leading
/// dot hides it on Linux and macOS. The process id keeps two programs apart,
/// and the counter keeps apart two saves in this one (tests run in parallel
/// threads, for example).
fn temporary_path_for(path: &Path) -> io::Result<PathBuf> {
    // One counter for the whole program. `AtomicU64` can be shared by every
    // thread without a lock, and `fetch_add` hands each caller its own value.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the output path does not name a file",
        )
    })?;
    // `Relaxed`: we only need each number to be different, not any order
    // between threads.
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);

    // A file name may not be valid text, so it is built as an `OsString`,
    // not a `String`.
    let mut temporary_name = OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".{}-{number}.tmp", std::process::id()));
    Ok(path.with_file_name(temporary_name))
}
