//! Merge several PDF documents into one.
//!
//! A PDF is a bag of numbered objects (pages, fonts, images, content
//! streams) plus a catalog that says where the page tree starts. Merging
//! means: renumber each input so its object numbers do not collide with the
//! others, pour all the objects into one document, and give that document a
//! single new page tree that lists every page in order.
//!
//! # Known limitations
//!
//! Only the pages and what they use travel to the merged file. These parts of
//! the inputs are left behind:
//!
//! - bookmarks (the outline shown in a reader's side panel);
//! - named destinations, so links that jump to one stop working;
//! - interactive form fields (AcroForm): their widgets still show on the page,
//!   but the form no longer works as a form;
//! - the tagged structure tree that screen readers use;
//! - the document information (title, author) and metadata.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

/// Page attributes that a page may inherit from the `Pages` nodes above it
/// instead of storing them itself (PDF 1.7, section 7.7.3.4).
const INHERITABLE_KEYS: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Everything that can stop a merge.
///
/// `input` counts the inputs from 1, in the order they were given, so the
/// caller (which has the list) can name the file.
#[derive(Debug)]
pub enum MergeError {
    /// The list of inputs was empty.
    NoInputs,
    /// An input file could not be read, or is not a PDF.
    Read { path: PathBuf, source: lopdf::Error },
    /// An input is encrypted and needs a password to be opened.
    Encrypted { input: usize },
    /// An input opened, but its structure is broken.
    Broken { input: usize, reason: &'static str },
    /// The merged document could not be written. Writing reports a plain
    /// input/output error, not a `lopdf::Error`.
    Save {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Path` is not always valid text, so it has no `Display` of its own:
        // `display()` borrows it as something printable.
        match self {
            Self::NoInputs => write!(f, "there are no input files to merge"),
            Self::Read { path, .. } => write!(f, "cannot read {} as a PDF", path.display()),
            Self::Encrypted { input } => {
                write!(f, "input {input} is encrypted and needs a password")
            }
            Self::Broken { input, reason } => write!(f, "input {input} is damaged: {reason}"),
            Self::Save { path, .. } => write!(f, "cannot save {}", path.display()),
        }
    }
}

impl std::error::Error for MergeError {
    /// The lower-level error behind this one, if any. The message above
    /// leaves it out on purpose, so a caller can print the whole chain
    /// without saying the same thing twice.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Two arms, not `Read { source, .. } | Save { source, .. }`: in an
        // or-pattern a name must have one type, and these two differ.
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Save { source, .. } => Some(source),
            Self::NoInputs | Self::Encrypted { .. } | Self::Broken { .. } => None,
        }
    }
}

/// What is wrong with one input, before we know its place in the list.
enum InputProblem {
    Encrypted,
    Broken(&'static str),
}

impl InputProblem {
    fn at(self, input: usize) -> MergeError {
        match self {
            Self::Encrypted => MergeError::Encrypted { input },
            Self::Broken(reason) => MergeError::Broken { input, reason },
        }
    }
}

/// Merges `documents` into a new document: all the pages of the first one,
/// then all the pages of the second one, and so on.
///
/// The function takes the documents by value (`Vec<Document>`, not
/// `&[Document]`): it owns them, so it can move their objects into the result
/// instead of copying them. A big scanned PDF can be hundreds of megabytes of
/// images, and moving them costs nothing. The price is that the caller cannot
/// use the documents afterwards, which is fine: they were loaded only to be
/// merged.
///
/// The result can still hold objects that only the old catalogs used, such as
/// bookmarks. Call [`Document::prune_objects`] before saving it, as
/// [`merge_files`] does.
pub fn merge_documents(documents: Vec<Document>) -> Result<Document, MergeError> {
    if documents.is_empty() {
        return Err(MergeError::NoInputs);
    }

    // The merged file declares the newest PDF version among the inputs, so a
    // feature from a 1.7 file is not served under a 1.4 header. `iter()` only
    // borrows the documents to read their versions; `to_owned()` copies the
    // winner into a new `String`, so the borrow ends here and the documents
    // can be moved in the loop below. The list is not empty, so `max()`
    // always finds one and the fallback is never used.
    //
    // `max()` compares the versions as text, character by character. That
    // gives the right answer for every real PDF version ("1.0" to "1.7", and
    // "2.0"), because each part has one digit. It would put "1.10" before
    // "1.9", but no such version exists.
    let version = documents
        .iter()
        .map(|document| document.version.as_str())
        .max()
        .unwrap_or("1.7")
        .to_owned();
    let mut merged = Document::with_version(version);

    // Object numbers 1 and 2 are kept for the new page tree and catalog; the
    // inputs are numbered after them.
    let pages_id: ObjectId = (1, 0);
    let catalog_id: ObjectId = (2, 0);
    let mut next_id = 3;
    let mut kids = Vec::new();

    // `into_iter()` takes each document out of the vector by value, so the
    // loop owns it and may move pieces out of it. `iter()` would only lend
    // `&Document`, and moving out of a borrow is not allowed.
    for (index, mut document) in documents.into_iter().enumerate() {
        // `map_err` turns the problem into a `MergeError`, then `?` returns
        // it from `merge_documents` if there was one.
        let page_ids = take_pages(&mut document, next_id, pages_id)
            .map_err(|problem| problem.at(index + 1))?;

        next_id = document.max_id + 1;
        // `extend` with the map itself (not `&document.objects`) moves every
        // object into the result: no copy of the data.
        merged.objects.extend(document.objects);
        kids.extend(page_ids.into_iter().map(Object::Reference));
    }

    // `as` would silently wrap a number too big for `i64`; harmless here,
    // since no document has anywhere near 2^63 pages.
    let page_count = kids.len() as i64;
    let pages = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => page_count };
    merged.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    merged
        .objects
        .insert(catalog_id, Object::Dictionary(catalog));
    merged.trailer.set("Root", catalog_id);

    // `max_id` is the highest object number in use; lopdf numbers new objects
    // from it. The map is sorted, so the last key holds the highest number.
    merged.max_id = merged.objects.keys().next_back().map_or(0, |id| id.0);

    Ok(merged)
}

/// Prepares one input to be poured into the merged document, and returns
/// its page ids in page order.
///
/// Its objects are renumbered from `first_id`, every page gets `new_parent`
/// as its parent, and its old catalog and page tree nodes are removed.
fn take_pages(
    document: &mut Document,
    first_id: u32,
    new_parent: ObjectId,
) -> Result<Vec<ObjectId>, InputProblem> {
    if is_locked(document) {
        return Err(InputProblem::Encrypted);
    }

    // After this, the document uses numbers `first_id..=max_id` only, so it
    // cannot collide with the inputs merged before it.
    document.renumber_objects_with(first_id);

    // The catalog first: the page tree hangs from it, so without a catalog
    // "no page tree" would be true but would hide the real problem.
    let catalog_id = catalog_of(document).ok_or(InputProblem::Broken("it has no catalog"))?;
    let root_id =
        page_tree_root(document, catalog_id).ok_or(InputProblem::Broken("it has no page tree"))?;

    // `page_iter` borrows the document, and the loop below needs to change
    // it. Collecting the ids into a `Vec` first ends that borrow.
    let page_ids: Vec<ObjectId> = document.page_iter().collect();
    if page_ids.is_empty() {
        return Err(InputProblem::Broken("its page tree has no pages"));
    }

    // Read the old tree before taking it apart.
    for &page_id in &page_ids {
        copy_inherited_attributes(document, page_id);
    }
    let old_nodes = page_tree_nodes(document, root_id, &page_ids);

    for &page_id in &page_ids {
        document
            .get_dictionary_mut(page_id)
            .map_err(|_| InputProblem::Broken("a page in its page tree is missing"))?
            .set("Parent", new_parent);
    }
    for node_id in old_nodes.into_iter().chain([catalog_id]) {
        document.objects.remove(&node_id);
    }

    Ok(page_ids)
}

/// Whether the document is encrypted and we could not open it.
///
/// lopdf opens an encrypted file by itself when its user password is empty,
/// as every PDF reader does, and then fills `encryption_state`. A file with a
/// real password loads with no objects and no state. A document encrypted in
/// memory with [`Document::encrypt`] has no state either.
fn is_locked(document: &Document) -> bool {
    document.trailer.has(b"Encrypt") && document.encryption_state.is_none()
}

/// The id of the catalog, named by the trailer's `/Root`, if that object is
/// really there.
fn catalog_of(document: &Document) -> Option<ObjectId> {
    // `.ok()?` turns each lopdf `Result` into an `Option` and returns `None`
    // at the first step that fails.
    let id = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    // Only checked, not kept: we need the id, not the dictionary.
    document.get_dictionary(id).ok()?;
    Some(id)
}

/// The id of the root `Pages` node, named by the catalog.
fn page_tree_root(document: &Document, catalog_id: ObjectId) -> Option<ObjectId> {
    document
        .get_dictionary(catalog_id)
        .ok()?
        .get(b"Pages")
        .ok()?
        .as_reference()
        .ok()
}

/// Writes into the page dictionary every inheritable attribute that it only
/// had through its ancestors. Without this, a page re-parented under the new
/// tree would lose its size, its rotation or its fonts.
fn copy_inherited_attributes(document: &mut Document, page_id: ObjectId) {
    for key in INHERITABLE_KEYS {
        // `inherited_value` borrows `document` to read, and returns an owned
        // copy of the value. That borrow is over by the time
        // `get_dictionary_mut` borrows `document` again to write.
        let Some(value) = inherited_value(document, page_id, key) else {
            continue;
        };
        if let Ok(page) = document.get_dictionary_mut(page_id) {
            page.set(key, value);
        }
    }
}

/// The value of `key` that the page inherits from its nearest ancestor, or
/// `None` when the page has its own value or no ancestor has one.
fn inherited_value(document: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let page = document.get_dictionary(page_id).ok()?;
    if own_value(page, key).is_some() {
        return None;
    }

    let mut visited = HashSet::from([page_id]);
    let mut node_id = parent_of(page)?;
    loop {
        // A damaged file can make the chain of parents loop. Stop when we
        // reach a node we have already seen.
        if !visited.insert(node_id) {
            return None;
        }
        let node = document.get_dictionary(node_id).ok()?;
        if let Some(value) = own_value(node, key) {
            // Usually a reference or a small array, so the copy is cheap.
            return Some(value.clone());
        }
        node_id = parent_of(node)?;
    }
}

/// The value stored under `key` in this very node, if any.
///
/// The lifetime `'a` says the returned reference lives inside `node`, not
/// inside `key`. With two borrowed inputs the compiler cannot guess which
/// one, so it has to be written down.
fn own_value<'a>(node: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    // PDF treats an entry whose value is null as if it were not there.
    node.get(key)
        .ok()
        .filter(|value| !matches!(value, Object::Null))
}

fn parent_of(node: &Dictionary) -> Option<ObjectId> {
    node.get(b"Parent").and_then(Object::as_reference).ok()
}

/// The ids of the old page tree: its root and every `Pages` node above the
/// given pages.
fn page_tree_nodes(
    document: &Document,
    root_id: ObjectId,
    page_ids: &[ObjectId],
) -> HashSet<ObjectId> {
    let mut nodes = HashSet::from([root_id]);
    // `&page_id` in the pattern copies each id out of the borrowed slice;
    // `ObjectId` is a pair of numbers, so copying it is free.
    for &page_id in page_ids {
        let mut current = document.get_dictionary(page_id).ok().and_then(parent_of);
        while let Some(node_id) = current {
            // Seen already: the rest of the way up is known (or it loops).
            if !nodes.insert(node_id) {
                break;
            }
            current = document.get_dictionary(node_id).ok().and_then(parent_of);
        }
    }
    nodes
}

/// Merges the PDF files `inputs`, in order, into a new file at `output`, and
/// returns how many pages it has.
///
/// The inputs are only borrowed (`&[P]`): the caller keeps its list. `P` can
/// be anything that looks like a path (`&str`, `String`, `PathBuf`...).
///
/// Every input is read into memory before `output` is written, and `output`
/// is replaced in one step (see [`save_atomically`]), so `output` may be one
/// of the inputs: if the save fails, that input is still intact. Objects that
/// nothing uses any more, such as the inputs' bookmarks, are removed before
/// saving.
pub fn merge_files<P: AsRef<Path>>(inputs: &[P], output: &Path) -> Result<usize, MergeError> {
    // `collect` into a `Result<Vec<_>, _>` stops at the first error and
    // returns it; `?` then hands that error to our caller.
    let documents = inputs
        .iter()
        .map(|input| load(input.as_ref()))
        .collect::<Result<Vec<_>, _>>()?;

    // `documents` is moved into `merge_documents` here, and gone afterwards.
    let mut merged = merge_documents(documents)?;
    merged.prune_objects();
    let page_count = merged.page_iter().count();

    save_atomically(&mut merged, output).map_err(|source| MergeError::Save {
        path: output.to_path_buf(),
        source,
    })?;
    Ok(page_count)
}

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
fn save_atomically(document: &mut Document, path: &Path) -> io::Result<()> {
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

fn load(path: &Path) -> Result<Document, MergeError> {
    Document::load(path).map_err(|source| MergeError::Read {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests;
