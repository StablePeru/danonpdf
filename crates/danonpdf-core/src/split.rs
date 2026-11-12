//! Take pages out of one PDF document: some of them into one new file
//! (extract), or all of them into several files (split). Also the question
//! both need answered first: how many pages are there.
//!
//! A merge pours whole documents together and afterwards throws away what
//! nothing uses. Here it works the other way round: the pages that are
//! wanted are chosen first, then everything they use is collected by
//! following their references, and only that goes into the new document.
//! Whatever the wanted pages do not use never gets there.
//!
//! # Known limitations
//!
//! As in a merge, only the pages and what they use travel. These parts of
//! the input are left behind:
//!
//! - bookmarks (the outline shown in a reader's side panel);
//! - named destinations, so links that jump to one stop working;
//! - interactive form fields (AcroForm): their widgets still show on the page,
//!   but the form no longer works as a form;
//! - the tagged structure tree that screen readers use;
//! - the document information (title, author) and metadata.
//!
//! Two more follow from cutting a document:
//!
//! - A link to a page that is not in the new file stays on its page but
//!   leads nowhere. The page it pointed at is not carried along in secret.
//! - A page brings everything its dictionary points at, used or not. Some
//!   programs give every page the same list of all the fonts and images of
//!   the document; each part of such a file then carries all of them and is
//!   almost as big as the whole.

use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId};

use crate::document::{
    DetachedPages, InputProblem, detach_pages, install_page_tree, is_locked, load, set_parent,
};
use crate::save::{Durability, save};
use crate::{PageSelection, PdfError};

/// How many pages the PDF file `input` has.
///
/// It refuses what the other operations refuse (a file that cannot be read,
/// that needs a password or that is damaged), so a count from here means
/// that those pages can also be extracted or split.
pub fn page_count(input: &Path) -> Result<usize, PdfError> {
    Ok(Source::load(input)?.pages.len())
}

/// Writes the pages of `input` that `pages` selects, in the order it gives
/// them, into a new file at `output`, and returns how many pages it has.
///
/// `input` is read whole before `output` is written, and `output` is
/// replaced in one step, so `output` may be `input` itself: if the save
/// fails, the input is still intact. What is not carried over to the new
/// file is listed at the top of `split.rs`.
///
/// The selection is checked against the document here, since only now is
/// its length known: a page past the end is [`PdfError::PageOutOfRange`],
/// and nothing is written.
pub fn extract_file(input: &Path, pages: &PageSelection, output: &Path) -> Result<usize, PdfError> {
    let mut extracted = Source::load(input)?.extract(pages)?;
    let page_count = extracted.page_iter().count();
    // Synced, as in a merge: one file, which may replace an existing one
    // (the input itself, even), so the short wait for the disk is worth it.
    save(&mut extracted, output, Durability::Synced)?;
    Ok(page_count)
}

/// The pages of `document` that `pages` selects, as a new document. The
/// tests use it to look at the result without going through a file.
#[cfg(test)]
fn extract_document(document: Document, pages: &PageSelection) -> Result<Document, PdfError> {
    Source::new(document)?.extract(pages)
}

/// A document that pages can be taken from.
struct Source {
    document: Document,
    /// The id of every page, in page order. Page 1 is `pages[0]`.
    pages: Vec<ObjectId>,
    /// Every page and every node of the old page tree. Collecting what a
    /// page uses stops at these: see [`objects_used_by`].
    fence: HashSet<ObjectId>,
}

impl Source {
    fn load(input: &Path) -> Result<Self, PdfError> {
        Self::new(load(input)?)
    }

    /// Checks `document` and gets its pages ready to leave it.
    ///
    /// There is one input here, so every problem is reported for input 1.
    fn new(mut document: Document) -> Result<Self, PdfError> {
        if is_locked(&document) {
            return Err(InputProblem::Encrypted.at(1));
        }
        // Each page now holds the attributes it used to inherit, so it can
        // be given to a new document without its old page tree.
        let DetachedPages { pages, old_nodes } =
            detach_pages(&mut document).map_err(|problem| problem.at(1))?;

        // `old_nodes` is moved into `fence` (no copy) and the pages are
        // added to it. `copied()` turns the `&ObjectId`s that `iter()`
        // lends into `ObjectId`s, which is what the set holds.
        let mut fence = old_nodes;
        fence.extend(pages.iter().copied());

        Ok(Self {
            document,
            pages,
            fence,
        })
    }

    /// A new document made of the pages that `selection` names, in its
    /// order.
    ///
    /// Takes `self` by value: this is the last use of the source, so the
    /// objects of the chosen pages are moved into the new document instead
    /// of being copied. For a big scan that is the difference between
    /// holding its images in memory once and holding them twice.
    fn extract(self, selection: &PageSelection) -> Result<Document, PdfError> {
        let numbers = selection.resolve(self.pages.len())?;
        // Page numbers count from 1 and positions in a `Vec` from 0, hence
        // the `- 1`. `resolve` only returns numbers from 1 to the page
        // count, so the index is always inside the list.
        let chosen: Vec<ObjectId> = numbers
            .iter()
            .map(|&number| self.pages[number - 1])
            .collect();

        let used = objects_used_by(&self.document, &chosen, &self.fence);
        // `self.document.objects` is moved out of the source: allowed
        // because `self` is owned here and nothing uses that field again.
        let mut objects = self.document.objects;
        // `retain` keeps the entries for which the closure says `true` and
        // drops the rest, in place.
        objects.retain(|id, _| used.contains(id));

        new_document(&self.document.version, objects, &chosen).map_err(|problem| problem.at(1))
    }

    /// A new document made of `pages` (ids of pages of this source), in
    /// that order.
    ///
    /// Takes `&self`: a split asks for one part after another, so the
    /// source must stay whole and each part gets a copy of what it uses.
    fn part(&self, pages: &[ObjectId]) -> Result<Document, PdfError> {
        let used = objects_used_by(&self.document, pages, &self.fence);
        // Every id in `used` names an object of the document, so `get`
        // always finds it; `filter_map` would skip one that did not,
        // instead of panicking as `objects[&id]` would.
        let objects = used
            .into_iter()
            .filter_map(|id| Some((id, self.document.objects.get(&id)?.clone())))
            .collect();

        new_document(&self.document.version, objects, pages).map_err(|problem| problem.at(1))
    }
}

/// The ids of `pages` and of every object they use, directly or through
/// other objects.
///
/// An object uses another one by holding a reference to it: a page refers
/// to its content and its resources, the resources to fonts and images, and
/// so on. This follows those references until nothing new turns up.
///
/// It never goes through `fence`, which holds every page of the document
/// and the nodes of its page tree. Without that, the first step would
/// already go wrong: every page refers to its parent node, the parent lists
/// all the pages, and each part of a split would end up holding the whole
/// document. A link from a chosen page to a page that was not chosen is
/// stopped the same way, so the page it points at stays out.
fn objects_used_by(
    document: &Document,
    pages: &[ObjectId],
    fence: &HashSet<ObjectId>,
) -> HashSet<ObjectId> {
    let mut used: HashSet<ObjectId> = pages.iter().copied().collect();
    // What is still to be looked into. A `Vec` used as a stack: `push` adds
    // at the end and `pop` takes from the end. A function that called
    // itself for every nested value would do the same job, but a file with
    // values nested thousands of levels deep could then overflow the
    // program's own stack; a `Vec` just grows.
    let mut pending: Vec<&Object> = pages
        .iter()
        .filter_map(|id| document.objects.get(id))
        .collect();

    while let Some(object) = pending.pop() {
        // One arm per kind of PDF value, with no `_` arm: if lopdf ever
        // gains a kind of value, this stops compiling until someone decides
        // whether it can hold references.
        match object {
            Object::Reference(id) => {
                // `insert` answers whether the id was new. An object seen
                // before is not looked into again, which is also what ends
                // the walk when two objects refer to each other.
                if !fence.contains(id)
                    && let Some(target) = document.objects.get(id)
                    && used.insert(*id)
                {
                    pending.push(target);
                }
            }
            // The containers: what they hold may be, or hold, references.
            Object::Array(items) => pending.extend(items),
            Object::Dictionary(dictionary) => {
                pending.extend(dictionary.iter().map(|(_, value)| value));
            }
            // The bytes of a stream are data (page content, an image); only
            // its dictionary can refer to other objects.
            Object::Stream(stream) => {
                pending.extend(stream.dict.iter().map(|(_, value)| value));
            }
            // Plain values: nothing inside to follow.
            Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::Name(_)
            | Object::String(..) => {}
        }
    }
    used
}

/// A document of PDF version `version` that holds `objects` and shows
/// `pages` (ids among those objects), in that order.
///
/// The objects keep the numbers they had in the input. Giving them new
/// ones (1, 2, 3...) would make a tidier file, but a reference left
/// pointing at an object that stayed behind, such as the target of a link
/// to a page that was not taken, could then land on the new number of some
/// unrelated object. Left alone, it points at a number that nothing has,
/// which every PDF reader takes to mean "nothing".
fn new_document(
    version: &str,
    objects: BTreeMap<ObjectId, Object>,
    pages: &[ObjectId],
) -> Result<Document, InputProblem> {
    let mut document = Document::with_version(version);
    document.objects = objects;

    // The new page tree and catalog take the two numbers after the highest
    // one in use. The map is sorted, so its last key holds that number.
    // `checked_add` gives `None` instead of wrapping round to a small
    // number if a damaged file already uses the highest number there is.
    let highest = document.objects.keys().next_back().map_or(0, |id| id.0);
    let (Some(pages_number), Some(catalog_number)) =
        (highest.checked_add(1), highest.checked_add(2))
    else {
        return Err(InputProblem::Broken(
            "its object numbers leave no room for a new page tree",
        ));
    };
    let pages_id = (pages_number, 0);

    set_parent(&mut document, pages, pages_id)?;
    install_page_tree(&mut document, pages_id, (catalog_number, 0), pages);
    Ok(document)
}

/// The names of the `parts` files that a split of `input` writes into
/// `folder`, in order.
///
/// Each is the name of the input without its extension, a dash, the number
/// of the part and `.pdf`: `report.pdf` gives `report-001.pdf`,
/// `report-002.pdf`... The parts are numbered from 1. The number is padded
/// with zeros to three digits, or to as many as the last part needs when
/// there are a thousand parts or more, so the files of one split always
/// sort by name in page order.
///
/// It only builds names and touches no file. The rule lives in the core so
/// that the window and the command-line tool name the files the same way.
pub fn split_output_paths(input: &Path, folder: &Path, parts: usize) -> Vec<PathBuf> {
    // `file_stem` is the file name without its last extension. It is `None`
    // for a path with no file name, such as "..": no PDF is ever read from
    // one, but a name is still needed here.
    let stem = input.file_stem().unwrap_or(OsStr::new("part"));
    // How many digits the highest number has, and never fewer than three.
    let width = parts.to_string().len().max(3);

    // `1..=parts` is a range: every number from 1 to `parts`, both
    // included. `map` builds one path from each, and `collect` gathers the
    // paths into the `Vec`. With `parts` at 0 the range is empty, and so is
    // the list.
    (1..=parts)
        .map(|number| {
            // A file name may not be valid text, so it is built as an
            // `OsString`, not a `String`. `{number:0width$}` writes the
            // number with leading zeros up to `width` digits.
            let mut name = stem.to_os_string();
            name.push(format!("-{number:0width$}.pdf"));
            folder.join(name)
        })
        .collect()
}

/// Reads `input` and works out how to split it into files of `every` pages
/// each (the last one may have fewer), inside `folder`. Nothing is written
/// yet: see [`SplitPlan`].
///
/// `every` is a [`NonZeroUsize`], a number that cannot be 0: "a file every
/// 0 pages" means nothing, and with this type the mistake cannot even be
/// passed in. With `every` at 1, each page gets its own file.
pub fn plan_split(input: &Path, every: NonZeroUsize, folder: &Path) -> Result<SplitPlan, PdfError> {
    Ok(SplitPlan::new(Source::load(input)?, input, every, folder))
}

/// A split that has been worked out and not yet written.
///
/// Splitting in two steps lets a front end look at the names
/// ([`outputs`](Self::outputs)) before any file exists, and refuse if one
/// of them would replace something, without reading the input twice: the
/// plan keeps the document it read.
pub struct SplitPlan {
    source: Source,
    every: NonZeroUsize,
    folder: PathBuf,
    outputs: Vec<PathBuf>,
}

impl SplitPlan {
    /// The plan for splitting `source`, which was read from `input`, into
    /// files of `every` pages inside `folder`.
    ///
    /// Apart from [`plan_split`] so that a test can plan the split of a
    /// document it built in memory, without a file to read it from. `input`
    /// only lends its name to the files.
    fn new(source: Source, input: &Path, every: NonZeroUsize, folder: &Path) -> Self {
        // `div_ceil` divides and rounds up: 5 pages in files of 2 make 3
        // files.
        let parts = source.pages.len().div_ceil(every.get());
        let outputs = split_output_paths(input, folder, parts);

        Self {
            source,
            every,
            folder: folder.to_path_buf(),
            outputs,
        }
    }

    /// The files this split will write, in order. Their names follow
    /// [`split_output_paths`].
    pub fn outputs(&self) -> &[PathBuf] {
        &self.outputs
    }

    /// How many pages the input has. Every one of them goes into exactly
    /// one of the files.
    pub fn pages(&self) -> usize {
        self.source.pages.len()
    }

    /// Creates the folder if it is missing, writes the files, and returns
    /// their paths in order.
    ///
    /// Each file is written whole and then put in place in one step, so a
    /// split that fails, or a program that crashes, never leaves half a
    /// file under one of the names. The files are written one after
    /// another; if one fails, the split stops there. The ones already
    /// written are complete and are kept, and the [`SplitError`] lists them.
    ///
    /// Takes `self` by value: a plan is carried out once, and the list of
    /// paths it held is handed back without being copied.
    ///
    /// # Not forced to the disk
    ///
    /// Unlike a merge or an extraction, a split does not wait for the disk
    /// after each file ([`Durability::Unsynced`]). If the power goes while
    /// it runs or in the moments after it, some of its files may be left
    /// empty or incomplete. The input is never touched, so the way out is
    /// to run the split again.
    ///
    /// That wait was measured at over 99% of the time of a split into many
    /// small files: with it, 300 one-page files took 2.9 s on Windows and
    /// about 5 s in the build container; without it, about 20 ms in that
    /// same container (the timing test at the end of `split/tests.rs`).
    /// The price is a split to run again after a power cut, from an input
    /// that is still whole.
    ///
    /// # Cost
    ///
    /// Time grows with the size of what is written. For each file, the
    /// objects its pages use are found by following references (only those
    /// objects are visited, not the whole document), copied, and saved. An
    /// object used by pages of several files, such as a font, is copied
    /// into each of them. So splitting a document whose pages share little
    /// costs about as much as copying it once, however many files that
    /// makes; one whose every page points at the same large resources costs
    /// that much per file.
    ///
    /// Memory holds the input and one file's worth of copies at a time.
    pub fn write(self) -> Result<Vec<PathBuf>, SplitError> {
        // Taking the plan apart gives each field a name and an owner of
        // its own: `folder` and `outputs` can then be moved into the result
        // further down, one without the other.
        let Self {
            source,
            every,
            folder,
            mut outputs,
        } = self;
        let parts = outputs.len();

        if let Err(error) = fs::create_dir_all(&folder) {
            return Err(SplitError {
                error: PdfError::CreateFolder {
                    path: folder,
                    source: error,
                },
                written: Vec::new(),
                parts,
            });
        }

        // `chunks` hands out the page ids in groups of `every` (the last
        // group has what is left), and `zip` walks that next to the list of
        // paths: each turn of the loop gets the pages of one file and the
        // path of that same file. The plan made one path per group, so
        // neither list runs out before the other.
        let mut written = 0;
        let mut failure = None;
        for (pages, path) in source.pages.chunks(every.get()).zip(&outputs) {
            // `and_then` runs the save only if the part could be built, and
            // gives the first error of the two otherwise.
            //
            // Unsynced: this is the one place in the core that does not
            // wait for the disk, and "Not forced to the disk" above says
            // why. A part can always be made again from the input, which
            // no split ever writes to.
            let saved = source
                .part(pages)
                .and_then(|mut part| save(&mut part, path, Durability::Unsynced));
            if let Err(error) = saved {
                failure = Some(error);
                break;
            }
            written += 1;
        }

        match failure {
            None => Ok(outputs),
            Some(error) => {
                // Cut the list down to the files that were written; the
                // rest of the paths name files that do not exist.
                outputs.truncate(written);
                Err(SplitError {
                    error,
                    written: outputs,
                    parts,
                })
            }
        }
    }
}

/// A split that stopped before its last file.
///
/// A struct around [`PdfError`] and not one more variant of it: what went
/// wrong is one of the usual failures, and what a split adds is the news
/// about the files it had already finished.
#[derive(Debug)]
pub struct SplitError {
    /// What stopped the split. Its `path`, if it has one, is the file or
    /// folder that could not be written.
    pub error: PdfError,
    /// The files written before it stopped, in order. They are complete,
    /// and they are left where they are.
    pub written: Vec<PathBuf>,
    /// How many files the split was going to write.
    pub parts: usize,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the split stopped after {} of {} files",
            self.written.len(),
            self.parts
        )
    }
}

impl Error for SplitError {
    /// The failure that stopped the split. `error_chain` prints it after
    /// the message above, so the whole line reads "the split stopped after
    /// 2 of 5 files: cannot save ...: ...".
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests;
