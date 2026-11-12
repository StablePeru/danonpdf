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

use std::path::Path;

use lopdf::{Document, ObjectId};

use crate::PdfError;
use crate::document::{InputProblem, detach_pages, install_page_tree, is_locked, load, set_parent};
use crate::save::save_atomically;

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
pub fn merge_documents(documents: Vec<Document>) -> Result<Document, PdfError> {
    if documents.is_empty() {
        return Err(PdfError::NoInputs);
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
    let mut pages = Vec::new();

    // `into_iter()` takes each document out of the vector by value, so the
    // loop owns it and may move pieces out of it. `iter()` would only lend
    // `&Document`, and moving out of a borrow is not allowed.
    for (index, mut document) in documents.into_iter().enumerate() {
        // `map_err` turns the problem into a `PdfError`, then `?` returns
        // it from `merge_documents` if there was one.
        let page_ids = take_pages(&mut document, next_id, pages_id)
            .map_err(|problem| problem.at(index + 1))?;

        next_id = document.max_id + 1;
        // `extend` with the map itself (not `&document.objects`) moves every
        // object into the result: no copy of the data.
        merged.objects.extend(document.objects);
        pages.extend(page_ids);
    }

    install_page_tree(&mut merged, pages_id, catalog_id, &pages);

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

    // The inherited attributes are copied into the pages before the old
    // tree is taken apart; `detach_pages` does it in that order.
    let detached = detach_pages(document)?;
    set_parent(document, &detached.pages, new_parent)?;
    for node_id in detached.old_nodes {
        document.objects.remove(&node_id);
    }

    Ok(detached.pages)
}

/// Merges the PDF files `inputs`, in order, into a new file at `output`, and
/// returns how many pages it has.
///
/// The inputs are only borrowed (`&[P]`): the caller keeps its list. `P` can
/// be anything that looks like a path (`&str`, `String`, `PathBuf`...).
///
/// Every input is read into memory before `output` is written, and `output`
/// is replaced in one step (`save_atomically` in `save.rs`), so `output` may
/// be one of the inputs: if the save fails, that input is still intact.
/// Objects that nothing uses any more, such as the inputs' bookmarks, are
/// removed before saving.
pub fn merge_files<P: AsRef<Path>>(inputs: &[P], output: &Path) -> Result<usize, PdfError> {
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

    save_atomically(&mut merged, output).map_err(|source| PdfError::Save {
        path: output.to_path_buf(),
        source,
    })?;
    Ok(page_count)
}

#[cfg(test)]
mod tests;
