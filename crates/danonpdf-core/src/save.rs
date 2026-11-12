//! Writing a document to disk without ever leaving a half-written file
//! behind a failed save, and with a stated answer ([`Durability`]) to "what
//! if the power goes right after it?".

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lopdf::Document;

use crate::PdfError;

/// How sure a save must be that the new file outlives a power cut.
///
/// Every save writes a temporary file and renames it into place, whatever
/// is chosen here, so a save that fails or a program that crashes never
/// leaves half a file under the final name. The choice is about one thing
/// only: whether to wait for the disk before the rename.
///
/// An enum and not a `bool`: `save(document, path, true)` says nothing to
/// someone reading the call, and `save(document, path, Durability::Synced)`
/// says what was chosen. Each caller picks one, and writes down why where
/// it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Durability {
    /// Wait until the bytes are on the disk itself before the file gets
    /// its name (`sync_all`). Once the save returns, a power cut cannot
    /// leave `path` naming an empty or incomplete file.
    ///
    /// For a save that may replace a file the user cannot get back, such
    /// as a merge or an extraction written over an existing file. The wait
    /// is a few milliseconds to a few tens of them per file, depending on
    /// the disk.
    Synced,
    /// Rename as soon as the system has taken the bytes, without waiting
    /// for the disk. The system writes them out by itself a moment later.
    ///
    /// If the power goes in that moment, `path` may be left naming an empty
    /// or incomplete file. Only for files that can simply be made again
    /// from an input that is still there.
    Unsynced,
}

/// Saves `document` at `path` in one step (see [`save_atomically`]), and
/// names `path` in the error if it cannot. `durability` says whether to
/// wait for the disk first: see [`Durability`].
///
/// `pub(crate)`: every module of this crate may call it, and no other crate
/// can. Callers outside reach it through the operations that save.
pub(crate) fn save(
    document: &mut Document,
    path: &Path,
    durability: Durability,
) -> Result<(), PdfError> {
    save_atomically(document, path, durability).map_err(|source| PdfError::Save {
        path: path.to_path_buf(),
        source,
    })
}

/// Saves `document` at `path` without ever leaving a half-written file there.
///
/// Writing straight into `path` would first empty the file and then fill it
/// again. If that stops halfway (disk full, a crash, the process killed), the
/// old contents are gone and the new ones are incomplete. Since `path` may be
/// one of the inputs, that would destroy the user's own file.
///
/// So the document goes to a temporary file in the same folder first. Only
/// once it is complete (and, with [`Durability::Synced`], on the disk) does
/// `rename` give it the name `path`, replacing whatever was there. A rename
/// does not copy any data: it points the name at the other file, in one
/// step. Anyone opening `path` gets either the old file or the new one,
/// never a mix. `std::fs::rename` replaces an existing file on Linux, macOS
/// and Windows alike.
fn save_atomically(document: &mut Document, path: &Path, durability: Durability) -> io::Result<()> {
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
    let result = write_and_replace(document, file, &temporary, path, durability);
    if result.is_err() {
        // Best effort: if the removal fails too, there is nothing better to
        // do, and the save error is the one worth reporting.
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Writes `document` into `file` (open at `temporary`), waits for the disk
/// if `durability` asks for it, and renames the file to `path`.
fn write_and_replace(
    document: &mut Document,
    file: File,
    temporary: &Path,
    path: &Path,
    durability: Durability,
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
    // After `into_inner` the bytes are with the system, which keeps them in
    // memory for a moment before it writes them out. `sync_all` waits until
    // they are really on the disk. Without it, a power cut just after the
    // rename could leave `path` naming a file that was never fully written.
    //
    // A `match` with both variants named and no `_` arm: a third kind of
    // durability added later would not compile until it is handled here.
    match durability {
        Durability::Synced => file.sync_all()?,
        Durability::Unsynced => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ScratchDir, markers, with_pages};

    /// Both ways of saving, with a name for the scratch folder of each.
    ///
    /// What sets them apart (whether the system was asked to wait for the
    /// disk) cannot be seen from a test: it only shows after a power cut.
    /// So the tests below check what the two must have in common.
    const BOTH: [(Durability, &str); 2] = [
        (Durability::Synced, "synced"),
        (Durability::Unsynced, "unsynced"),
    ];

    #[test]
    fn either_durability_writes_a_whole_file_and_no_temporary_one() {
        for (durability, name) in BOTH {
            let scratch = ScratchDir::new(&format!("save-{name}"));
            let path = scratch.file("out.pdf");

            save(&mut with_pages("A", 2), &path, durability).unwrap();

            let reloaded = Document::load(&path).unwrap();
            assert_eq!(markers(&reloaded), ["A1", "A2"], "{durability:?}");
            assert_eq!(scratch.entries(), ["out.pdf"], "{durability:?}");
        }
    }

    #[test]
    fn either_durability_replaces_the_file_that_was_there() {
        for (durability, name) in BOTH {
            let scratch = ScratchDir::new(&format!("save-replace-{name}"));
            let path = scratch.save("out.pdf", with_pages("OLD", 1));

            save(&mut with_pages("NEW", 1), &path, durability).unwrap();

            let reloaded = Document::load(&path).unwrap();
            assert_eq!(markers(&reloaded), ["NEW1"], "{durability:?}");
            assert_eq!(scratch.entries(), ["out.pdf"], "{durability:?}");
        }
    }

    #[test]
    fn a_failed_save_names_the_path_and_leaves_nothing_behind() {
        for (durability, name) in BOTH {
            let scratch = ScratchDir::new(&format!("save-failed-{name}"));
            // The folder the file should go into does not exist.
            let path = scratch.file("missing").join("out.pdf");

            let result = save(&mut with_pages("A", 1), &path, durability);

            match result {
                Err(PdfError::Save { path: named, .. }) => assert_eq!(named, path),
                other => panic!("expected a save error, got {other:?}"),
            }
            assert_eq!(scratch.entries(), Vec::<String>::new(), "{durability:?}");
        }
    }
}
