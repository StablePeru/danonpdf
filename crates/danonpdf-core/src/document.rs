//! What every operation does with a document around its own work: open it,
//! refuse what cannot be handled, take its pages out of their page tree and
//! hang pages from a new one.
//!
//! A PDF lists its pages in a tree: the catalog names a root `Pages` node,
//! whose `Kids` are pages or more `Pages` nodes. A page can leave some of
//! its attributes to the nodes above it. To move a page into another
//! document, it must first hold those attributes itself, and then get a new
//! parent. That is the same job whether the pages come from several files
//! (a merge) or go to several files, so it is written once, here.

use std::collections::HashSet;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

use crate::PdfError;

/// Page attributes that a page may inherit from the `Pages` nodes above it
/// instead of storing them itself (PDF 1.7, section 7.7.3.4).
const INHERITABLE_KEYS: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Why a document is refused when its page tree lists a page whose object
/// is not in the file.
const MISSING_PAGE: &str = "a page in its page tree is missing";

/// What is wrong with one input, before we know its place in the list.
pub(crate) enum InputProblem {
    Encrypted,
    Broken(&'static str),
}

impl InputProblem {
    /// The error for the caller, now that the place of the input is known.
    /// Inputs are counted from 1.
    pub(crate) fn at(self, input: usize) -> PdfError {
        match self {
            Self::Encrypted => PdfError::Encrypted { input },
            Self::Broken(reason) => PdfError::Broken { input, reason },
        }
    }
}

/// Reads the PDF file at `path` into memory.
pub(crate) fn load(path: &Path) -> Result<Document, PdfError> {
    Document::load(path).map_err(|source| PdfError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Whether the document is encrypted and we could not open it.
///
/// lopdf opens an encrypted file by itself when its user password is empty,
/// as every PDF reader does, and then fills `encryption_state`. A file with a
/// real password loads with no objects and no state. A document encrypted in
/// memory with [`Document::encrypt`] has no state either.
pub(crate) fn is_locked(document: &Document) -> bool {
    document.trailer.has(b"Encrypt") && document.encryption_state.is_none()
}

/// The pages of a document, ready to leave its page tree.
pub(crate) struct DetachedPages {
    /// The id of every page, in page order.
    pub(crate) pages: Vec<ObjectId>,
    /// The catalog and every `Pages` node of the old tree. A new document
    /// gets a catalog and a page tree of its own, so these stay behind.
    pub(crate) old_nodes: HashSet<ObjectId>,
}

/// Makes every page of `document` hold the attributes it only had through
/// its ancestors, and returns the pages with the nodes they no longer need.
///
/// The pages still name their old parent afterwards: [`set_parent`] gives
/// them a new one.
pub(crate) fn detach_pages(document: &mut Document) -> Result<DetachedPages, InputProblem> {
    // The catalog first: the page tree hangs from it, so without a catalog
    // "no page tree" would be true but would hide the real problem.
    let catalog_id = catalog_of(document).ok_or(InputProblem::Broken("it has no catalog"))?;
    let root_id =
        page_tree_root(document, catalog_id).ok_or(InputProblem::Broken("it has no page tree"))?;

    // `page_iter` borrows the document, and the loop below needs to change
    // it. Collecting the ids into a `Vec` first ends that borrow.
    let pages: Vec<ObjectId> = document.page_iter().collect();
    if pages.is_empty() {
        return Err(InputProblem::Broken("its page tree has no pages"));
    }
    // lopdf keeps the place of a page whose object is missing, so the ids
    // can name objects that are not there. Found here, before any page is
    // taken, the whole document is refused: a split never gets halfway
    // through its files and then meets the bad page.
    if pages
        .iter()
        .any(|&page_id| document.get_dictionary(page_id).is_err())
    {
        return Err(InputProblem::Broken(MISSING_PAGE));
    }

    // Read the old tree before anyone takes it apart.
    for &page_id in &pages {
        copy_inherited_attributes(document, page_id);
    }
    let mut old_nodes = page_tree_nodes(document, root_id, &pages);
    old_nodes.insert(catalog_id);

    Ok(DetachedPages { pages, old_nodes })
}

/// Makes `parent` the parent of every page in `pages`.
pub(crate) fn set_parent(
    document: &mut Document,
    pages: &[ObjectId],
    parent: ObjectId,
) -> Result<(), InputProblem> {
    // `&page_id` in the pattern copies each id out of the borrowed slice;
    // `ObjectId` is a pair of numbers, so copying it is free.
    for &page_id in pages {
        document
            .get_dictionary_mut(page_id)
            .map_err(|_| InputProblem::Broken(MISSING_PAGE))?
            .set("Parent", parent);
    }
    Ok(())
}

/// Gives `document` a catalog and a page tree of one node that lists
/// `pages` in order, under the ids `catalog_id` and `pages_id`.
///
/// The pages must already name `pages_id` as their parent (see
/// [`set_parent`]), and both ids must be free.
pub(crate) fn install_page_tree(
    document: &mut Document,
    pages_id: ObjectId,
    catalog_id: ObjectId,
    pages: &[ObjectId],
) {
    // `as` would silently wrap a number too big for `i64`; harmless here,
    // since no document has anywhere near 2^63 pages.
    let page_count = pages.len() as i64;
    // `copied()` turns the borrowed ids into ids, and `Object::Reference`
    // is used as a function: it wraps each id in that variant of `Object`.
    let kids: Vec<Object> = pages.iter().copied().map(Object::Reference).collect();
    let page_tree = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => page_count };
    document
        .objects
        .insert(pages_id, Object::Dictionary(page_tree));
    let catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    document
        .objects
        .insert(catalog_id, Object::Dictionary(catalog));
    document.trailer.set("Root", catalog_id);

    // `max_id` is the highest object number in use; lopdf numbers new objects
    // from it. The map is sorted, so the last key holds the highest number.
    document.max_id = document.objects.keys().next_back().map_or(0, |id| id.0);
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
