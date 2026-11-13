//! Put the pages of one PDF document in place: turn some, delete some, move
//! some, and write what is left into a new file.
//!
//! The three things are one operation, [`arrange_file`]. It is given the
//! list of pages the new file is to have, in order, each with the turn it
//! gets. A page that is not in the list is deleted; a page listed somewhere
//! else than where it was has moved. [`rotate_file`] and [`delete_file`]
//! are the two everyday cases of it, for a caller that has a page selection
//! and not a whole list.
//!
//! The new document is built the way an extraction builds its own (see
//! `split.rs`, which also lists what does not travel): the pages that stay
//! are chosen, then everything they use is collected by following their
//! references. So a deleted page is really gone from the new file, with its
//! content, and is not just hidden from the page tree.

use std::collections::HashSet;
use std::path::Path;

use lopdf::{Document, ObjectId};

use crate::save::{Durability, save};
use crate::split::Source;
use crate::{PageSelection, PdfError, Turn};

/// One page of the document being written: which page of the input it is,
/// and how far it is turned on the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageEdit {
    /// The number of the page in the input, counted from 1.
    pub page: usize,
    /// The turn it gets, on top of the rotation it already has.
    /// [`Turn::Zero`] leaves the page exactly as it is.
    pub turn: Turn,
}

/// Writes a new file at `output` with the pages of `input` that `edits`
/// lists, in that order, each with its turn, and returns how many pages it
/// has.
///
/// A page of `input` that no edit names is deleted. Nothing else changes
/// the page count: the list may not be empty
/// ([`PdfError::NoPagesLeft`]), may not name a page twice
/// ([`PdfError::PageRepeated`]) and may not name a page the input does not
/// have ([`PdfError::PageOutOfRange`]). The first edit that breaks a rule
/// is the one reported, and nothing is written.
///
/// # Rotation
///
/// A turn is relative to the rotation the page already has, its own or the
/// one it inherits from the page tree. A page that is turned ends up with a
/// `/Rotate` of its own holding 0, 90, 180 or 270: a page at 270 that gets
/// a quarter turn is written with `/Rotate 0`, not left without the key.
///
/// An input may hold a `/Rotate` that the PDF specification does not allow:
/// a number that is not a multiple of 90, or something that is not a whole
/// number at all. Such a page counts as not rotated (0) when it is turned:
/// there is no sound way to add a quarter turn to 45 degrees.
///
/// A page with [`Turn::Zero`] is not touched: whatever it had under
/// `/Rotate` travels as it is, also `-90` or a value that is not allowed.
///
/// # Saving
///
/// As in an extraction: `input` is read whole before `output` is written,
/// `output` is replaced in one step, and the save waits for the disk. So
/// `output` may be `input` itself, and a save that fails leaves the input
/// intact.
pub fn arrange_file(input: &Path, edits: &[PageEdit], output: &Path) -> Result<usize, PdfError> {
    write(Source::load(input)?, edits, output)
}

/// Writes every page of `input`, in its order, into a new file at `output`,
/// with the pages that `pages` selects turned by `turn`. Returns how many
/// pages the new file has: as many as the input.
///
/// The order in which `pages` names the pages does not matter here: `3,1`
/// turns pages 1 and 3 and moves neither. A page past the end of the
/// document is [`PdfError::PageOutOfRange`], and nothing is written.
///
/// How a turn is applied and how the file is saved are described on
/// [`arrange_file`], which does the work.
pub fn rotate_file(
    input: &Path,
    pages: &PageSelection,
    turn: Turn,
    output: &Path,
) -> Result<usize, PdfError> {
    let source = Source::load(input)?;
    let page_count = source.pages.len();
    // `resolve` gives the chosen pages as a list. A set answers "is this
    // page among them?" in one step, where a list would be read from its
    // start for every page of the document.
    let chosen: HashSet<usize> = pages.resolve(page_count)?.into_iter().collect();

    // One edit per page of the input, in page order: nothing moves and
    // nothing is deleted. `if` is an expression, so it can give the value
    // of a field directly.
    let edits: Vec<PageEdit> = (1..=page_count)
        .map(|page| PageEdit {
            page,
            turn: if chosen.contains(&page) {
                turn
            } else {
                Turn::Zero
            },
        })
        .collect();

    write(source, &edits, output)
}

/// Writes the pages of `input` that `pages` does not select, in their
/// order, into a new file at `output`, and returns how many pages it has.
///
/// A page past the end of the document is [`PdfError::PageOutOfRange`].
/// Deleting every page is [`PdfError::NoPagesLeft`]: the result would be a
/// file that this library itself refuses to read (a page tree with no
/// pages counts as damaged), and whoever asks for one has more likely
/// mistyped the selection. Either way, nothing is written.
///
/// How the file is saved is described on [`arrange_file`], which does the
/// work.
pub fn delete_file(input: &Path, pages: &PageSelection, output: &Path) -> Result<usize, PdfError> {
    let source = Source::load(input)?;
    let page_count = source.pages.len();
    let deleted: HashSet<usize> = pages.resolve(page_count)?.into_iter().collect();

    // Every page that was not chosen, in page order and as it is. `filter`
    // lends each number to its closure (`page` is a `&usize` there), which
    // is just what `contains` wants.
    let edits: Vec<PageEdit> = (1..=page_count)
        .filter(|page| !deleted.contains(page))
        .map(|page| PageEdit {
            page,
            turn: Turn::Zero,
        })
        .collect();

    // With every page deleted the list is empty, and an empty list is
    // refused in `check` as `NoPagesLeft`: the rule is written once.
    write(source, &edits, output)
}

/// Builds the document that `edits` describe out of `source` and saves it
/// at `output`. The three public functions end here.
fn write(source: Source, edits: &[PageEdit], output: &Path) -> Result<usize, PdfError> {
    // If `arrange` refuses the edits, `?` returns here and `save` is never
    // reached: that is all "nothing is written" takes.
    let mut arranged = arrange(source, edits)?;
    let page_count = arranged.page_iter().count();
    // Synced, as in an extraction: one file, which may replace an existing
    // one (the input itself, even), so the short wait for the disk is
    // worth it.
    save(&mut arranged, output, Durability::Synced)?;
    Ok(page_count)
}

/// A new document made of the pages of `source` that `edits` list, in that
/// order, each turned as its edit says.
///
/// Takes the source by value, as an extraction does: the pages that stay
/// are moved into the new document, not copied.
fn arrange(mut source: Source, edits: &[PageEdit]) -> Result<Document, PdfError> {
    // Every edit is checked before any page is changed. The source is
    // thrown away on an error anyway, but this way the loop below can
    // count on every page number being a real page.
    check(edits, source.pages.len())?;

    let mut chosen = Vec::with_capacity(edits.len());
    for edit in edits {
        // Page numbers count from 1 and positions in a `Vec` from 0, hence
        // the `- 1`. `check` saw to it that the page is neither 0 nor past
        // the end.
        let page_id = source.pages[edit.page - 1];
        turn_page(&mut source.document, page_id, edit.turn);
        chosen.push(page_id);
    }

    // From here on it is an extraction: these pages, in this order, with
    // what they use. The pages left out stay behind with the source.
    source.take(&chosen)
}

/// Whether `edits` can be carried out on a document of `page_count` pages:
/// there is at least one, and each names a different page that exists.
///
/// The edits are looked at in order and the first one that breaks a rule is
/// reported, so the answer does not depend on which rule is checked first.
fn check(edits: &[PageEdit], page_count: usize) -> Result<(), PdfError> {
    if edits.is_empty() {
        return Err(PdfError::NoPagesLeft);
    }

    let mut seen = HashSet::new();
    for edit in edits {
        // Page 0 is as much outside the document as a page past the end.
        // Left unchecked, `edit.page - 1` would go below zero and stop the
        // program.
        if edit.page == 0 || edit.page > page_count {
            return Err(PdfError::PageOutOfRange {
                page: edit.page,
                pages: page_count,
            });
        }
        // `insert` answers whether the number was new to the set.
        if !seen.insert(edit.page) {
            return Err(PdfError::PageRepeated { page: edit.page });
        }
    }
    Ok(())
}

/// Turns the page `page_id` of `document` by `turn`, on top of the rotation
/// it has. With [`Turn::Zero`] the page is not touched at all.
fn turn_page(document: &mut Document, page_id: ObjectId, turn: Turn) {
    if turn == Turn::Zero {
        return;
    }
    // Read first, write after: `rotation_of` borrows the document to read
    // it and gives back a plain value, so that borrow is over by the time
    // `get_dictionary_mut` borrows the document again to change it.
    let rotation = rotation_of(document, page_id).then(turn);
    if let Ok(page) = document.get_dictionary_mut(page_id) {
        // Always written, also when it is 0: a page that used to say 270
        // must not go on saying it, and a key that is there cannot be
        // mistaken for one that was forgotten.
        page.set("Rotate", rotation.degrees());
    }
}

/// How the page `page_id` is rotated now, as the turn that its `/Rotate`
/// stands for.
///
/// The page's own dictionary is the only place to look: a [`Source`] has
/// already copied onto every page the rotation it used to inherit from the
/// page tree.
///
/// A page without the key is not rotated. One whose `/Rotate` the PDF
/// specification does not allow (not a whole number, or not a multiple of
/// 90) counts as not rotated too: see "Rotation" on [`arrange_file`].
fn rotation_of(document: &Document, page_id: ObjectId) -> Turn {
    // Each step may come up empty, and `and_then` only goes on while there
    // is a value: no page, no key, not a whole number, not a multiple of
    // 90. `get_deref` also follows a `/Rotate` that is kept in an object
    // of its own and only referred to from the page.
    document
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get_deref(b"Rotate", document).ok())
        .and_then(|value| value.as_i64().ok())
        .and_then(Turn::from_rotation)
        .unwrap_or(Turn::Zero)
}

#[cfg(test)]
mod tests;
