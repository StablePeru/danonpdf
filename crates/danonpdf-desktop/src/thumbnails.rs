//! The state behind the Pages view, with no drawing in it.
//!
//! The view shows the pages of one PDF as thumbnails. Drawing a page is
//! PDFium's work and happens on another thread (`thumbnail_worker.rs`), so
//! this state never has a page the moment it wants one. What it holds is
//! the bookkeeping of a conversation: which file was asked about, which
//! pages were asked for, which answers have come, and what to say in the
//! status line meanwhile.
//!
//! Two things come in and two go out:
//!
//! - In, from the drawing code: what the user chose ([`Thumbnails::choose`])
//!   and which rows of thumbnails are on screen ([`Thumbnails::show_rows`]).
//! - In, from the worker: its replies ([`Thumbnails::receive`]).
//! - Out, to the worker: the requests that follow from all of that
//!   ([`Thumbnails::take_requests`]).
//! - Out, to the drawing code: the images that arrived
//!   ([`Thumbnails::update_pictures`]), and everything it needs to draw.
//!
//! Nothing here reads a file, calls PDFium or knows egui, and no method
//! waits for anything. That is what keeps the window answering, and it is
//! also what makes every rule testable by calling it: a test plays the
//! drawing code and the worker, one step at a time.
//!
//! As in the other views, the drawing is elsewhere (`thumbnails_view.rs`),
//! and so are the textures: an image handed out by `update_pictures`
//! belongs to the drawing code from then on, and this state only remembers
//! that the page has one. Which textures the drawing code may keep is
//! still decided here, in that same method.

use std::collections::BTreeMap;
use std::io;
use std::num::NonZeroUsize;
use std::ops::{Range, RangeInclusive};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use danonpdf_core::{PageImage, RenderError, error_chain};

use crate::drops::{OneOfSeveral, one_pdf_of};
use crate::paths::display_name;
use crate::texts;
use crate::thumbnail_worker::{Generation, Reply, Request, ThumbnailSize, Worker};

/// How wide a thumbnail is on screen, in egui points. A point is one pixel
/// on an ordinary screen and more on a sharper one: see
/// [`thumbnail_size`].
pub const THUMBNAIL_WIDTH: f32 = 160.0;

/// How high the box of a thumbnail is, in egui points. An A4 page drawn
/// [`THUMBNAIL_WIDTH`] wide is this high (160 * 842 / 595), so the most
/// common page fills its box exactly; a page of another shape is fitted
/// inside it.
pub const THUMBNAIL_HEIGHT: f32 = 226.0;

/// How many thumbnails are kept at most.
///
/// A long document would otherwise fill the memory as the user scrolls
/// through it: every thumbnail is a picture held twice, once in the memory
/// of the graphics card and, until the drawing code has taken it, once
/// here. Past this number, the thumbnails farthest from what is on screen
/// are forgotten, and drawn again if the user comes back to them.
///
/// What it costs: a thumbnail of an A4 page is 160 by 226 pixels of four
/// bytes each, about 145 KiB, so 120 of them take about 17 MiB. On a
/// screen with twice the pixels per point each one is four times that, and
/// the 120 take about 68 MiB. No page shape costs more: the worker fits
/// every thumbnail into the same box.
///
/// One exception keeps the limit honest instead of strict: what is on
/// screen is never forgotten, even in a window so large that it shows more
/// thumbnails than this (see [`Thumbnails::show_rows`]).
pub const MAX_KEPT: usize = 120;

/// How many rows above and below the ones on screen are drawn too, so
/// that scrolling by a row shows pages and not empty boxes.
pub const MARGIN_ROWS: usize = 1;

/// Everything the Pages view remembers from one frame to the next.
///
/// `Default` is an empty view: no file, nothing asked for.
#[derive(Debug, Default)]
pub struct Thumbnails {
    /// Which requests are still wanted: see `Generation` in
    /// `thumbnail_worker.rs`. The worker holds another owner of the same
    /// number.
    generation: Generation,
    document: Document,
    /// What is known about each page that was asked for, by page number
    /// (counted from 1). A page that is not in the map was never asked
    /// for, or was forgotten.
    ///
    /// A `BTreeMap` keeps its keys in order, so the tests and the code
    /// that picks what to forget walk the pages from first to last.
    slots: BTreeMap<usize, Slot>,
    /// The images that arrived and that the drawing code has not taken
    /// yet, each with its page number.
    fresh: Vec<(usize, PageImage)>,
    /// The size the thumbnails in `slots` were asked at. `None` until the
    /// first row is shown.
    size: Option<ThumbnailSize>,
    /// The pages wanted now: the rows on screen and their margin, as the
    /// drawing code last reported them. `None` while there are none.
    wanted: Option<RangeInclusive<usize>>,
    status: Status,
    /// The requests that the worker has not been given yet.
    outbox: Vec<Request>,
    /// Raised when a file is chosen, and lowered by the drawing code when
    /// it has shown the grid from its first row: see
    /// [`Thumbnails::take_scroll_to_top`].
    scroll_to_top: bool,
}

/// The file the view shows, and how far it has got.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Document {
    /// No file is chosen.
    #[default]
    None,
    /// A file was chosen and the worker was asked to open it. The answer
    /// has not come yet: it usually takes a few milliseconds, and the
    /// first time also the time to load PDFium.
    Opening { path: PathBuf },
    /// The worker opened the file and counted its pages.
    Open { path: PathBuf, pages: usize },
}

/// What is known about one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// The worker was asked to draw it, in the current generation.
    Requested,
    /// It was drawn. The image goes out through
    /// [`Thumbnails::update_pictures`]; the drawing code holds its texture
    /// from then on.
    Ready,
    /// The worker could not draw it. `reason` is the error and its causes,
    /// in the core's words. The page keeps its place in the grid.
    Failed { reason: String },
}

/// What the status line at the bottom of the Pages view says.
///
/// One variant per thing that can be said, each with exactly the data its
/// sentence needs (see `Status` in `merge_list.rs` for why an enum).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing to report: the line describes the document instead.
    #[default]
    Idle,
    /// Files were given and none of them is a PDF, so nothing changed.
    NoPdf { not_pdf: usize },
    /// A file was chosen out of several: the view takes one at a time.
    LeftOut {
        /// PDFs after the first one.
        other_pdfs: usize,
        /// Files whose name does not end in `.pdf`.
        not_pdf: usize,
    },
    /// `file` could not be opened (it needs a password, it is damaged, it
    /// is not a PDF, it has no pages), so the view shows nothing.
    NotAccepted { file: PathBuf, message: String },
    /// The library that draws pages is missing or does not load. Nothing
    /// is wrong with the file that was chosen. `message` is the error and
    /// its causes, and names the library file that was looked for.
    NoEngine { message: String },
    /// The worker thread ended by itself, which only a bug can cause.
    WorkerStopped,
    /// The system could not start the worker thread. `message` is what
    /// it said.
    WorkerNotStarted { message: String },
}

impl Thumbnails {
    /// The number that says which requests are still wanted. The drawing
    /// code gives the worker another owner of it when it starts one.
    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// How many pages the open document has. `None` while no file is open.
    pub fn pages(&self) -> Option<usize> {
        match &self.document {
            Document::Open { pages, .. } => Some(*pages),
            Document::None | Document::Opening { .. } => None,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// What is known about page `page`, counted from 1. `None` for a page
    /// that was never asked for, or was forgotten.
    pub fn slot(&self, page: usize) -> Option<&Slot> {
        self.slots.get(&page)
    }

    /// How many pages hold an answer, drawn or failed: the number that
    /// [`MAX_KEPT`] limits.
    pub fn kept(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| !matches!(slot, Slot::Requested))
            .count()
    }

    /// Makes the first PDF among `paths` the file to show, in place of the
    /// one shown before.
    ///
    /// Which file is taken out of several, and what is said about the
    /// rest, is the rule of the Split view (`one_pdf_of` in `drops.rs`).
    /// With no PDF among them, nothing changes.
    ///
    /// The file is not read here: this runs on the thread that draws the
    /// window. The worker is asked to open it, and the view shows
    /// [`Document::Opening`] until the answer comes. So, unlike in the
    /// Split view, the file shown before is closed at once and not only if
    /// the new one turns out to be readable: its pages must stop being
    /// drawn the moment the user asks for another file. A file that
    /// cannot be opened leaves the view empty, and the status says why.
    pub fn choose(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let OneOfSeveral {
            pdf,
            other_pdfs,
            not_pdf,
        } = one_pdf_of(paths);

        let Some(path) = pdf else {
            // No path at all (an empty drop) is nothing to report.
            if not_pdf > 0 {
                self.status = Status::NoPdf { not_pdf };
            }
            return;
        };

        // Everything about the old file goes, and a new generation starts:
        // the worker skips what is still in its queue for the old file,
        // and whatever it is drawing right now is ignored when it arrives.
        self.close();
        // A new file is looked at from its first page, wherever the one
        // before it was left.
        self.scroll_to_top = true;
        self.outbox.push(Request::Open {
            generation: self.generation.current(),
            // The path is needed twice, here and in the document below.
            path: path.clone(),
        });
        self.document = Document::Opening { path };
        self.status = if other_pdfs == 0 && not_pdf == 0 {
            Status::Idle
        } else {
            Status::LeftOut {
                other_pdfs,
                not_pdf,
            }
        };
    }

    /// Tells the state that the view is no longer shown: the user went to
    /// another view.
    ///
    /// The pages still waited for are cancelled, so the worker does not
    /// draw for a view nobody looks at. The document stays open and the
    /// thumbnails that arrived stay drawn, so coming back shows them at
    /// once and only asks for what was cancelled.
    ///
    /// A file that is still being opened is left alone: that is one short
    /// call, and its answer is what the user will want to find on coming
    /// back.
    pub fn hide(&mut self) {
        if self.pages().is_some() {
            self.cancel_requests();
            self.wanted = None;
        }
    }

    /// Tells the state which rows of thumbnails are on screen, and asks
    /// the worker for the pages in them that are missing.
    ///
    /// The drawing code calls this on every frame that shows the grid.
    /// `rows` are the rows on screen, counted from 0, `columns` is how
    /// many thumbnails a row holds, and `size` the size of a thumbnail in
    /// pixels (see [`thumbnail_size`]). Nothing happens while no document
    /// is open.
    ///
    /// Only those rows are asked for, with [`MARGIN_ROWS`] more on each
    /// side, and the pages on screen go first. A document of a thousand
    /// pages costs what one screenful costs.
    ///
    /// Three more things are settled here, because they all depend on
    /// what is on screen:
    ///
    /// - A change of `size` (the window moved to a screen with another
    ///   scale) forgets every thumbnail: they would be blurred, or larger
    ///   than needed.
    /// - If pages are still waited for that are no longer wanted (the user
    ///   scrolled away before they were drawn), everything waited for is
    ///   cancelled with a new generation, and what is wanted now is asked
    ///   for again. The worker then draws what is on screen next, not
    ///   what was on screen a moment ago.
    /// - Past [`MAX_KEPT`] thumbnails, the ones farthest from these rows
    ///   are forgotten. A page that is wanted now is never forgotten.
    pub fn show_rows(&mut self, rows: Range<usize>, columns: usize, size: ThumbnailSize) {
        let Document::Open { path, pages } = &self.document else {
            return;
        };
        // Copied out, so that the borrow of `self.document` ends here and
        // the methods below can change `self`.
        let (path, pages) = (path.clone(), *pages);

        if self.size != Some(size) {
            // The first call finds nothing to forget and only notes the
            // size.
            if !self.slots.is_empty() {
                self.generation.advance();
                self.slots.clear();
                self.fresh.clear();
                self.outbox.clear();
            }
            self.size = Some(size);
        }

        let on_screen = pages_in_rows(&rows, columns, pages);
        let margin = (rows.start.saturating_sub(MARGIN_ROWS))..(rows.end + MARGIN_ROWS);
        self.wanted = pages_in_rows(&margin, columns, pages);
        // `let ... else` with a clone: the range is needed below while
        // `self` is changed.
        let Some(wanted) = self.wanted.clone() else {
            return;
        };

        let waits_for_unwanted = self
            .slots
            .iter()
            .any(|(page, slot)| *slot == Slot::Requested && !wanted.contains(page));
        if waits_for_unwanted {
            self.cancel_requests();
        }

        // On screen first, then the margin: `chain` runs through the first
        // range and then the second. A page of the margin that is also on
        // screen has its slot by then and is skipped.
        for page in on_screen.into_iter().flatten().chain(wanted) {
            if self.slots.contains_key(&page) {
                continue;
            }
            // Page numbers start at 1, so `new` always gives `Some`; the
            // `let ... else` is there so that no `unwrap` can panic.
            let Some(number) = NonZeroUsize::new(page) else {
                continue;
            };
            self.slots.insert(page, Slot::Requested);
            self.outbox.push(Request::Page {
                generation: self.generation.current(),
                path: path.clone(),
                page: number,
                size,
            });
        }

        self.forget_the_farthest();
    }

    /// Takes in one reply of the worker.
    ///
    /// A reply of an older generation is dropped without a trace: it
    /// answers a question that is no longer asked (another file was
    /// chosen, the page scrolled away). The worker cannot always avoid
    /// sending one, because it may have been in the middle of the work
    /// when the generation moved on.
    pub fn receive(&mut self, reply: Reply) {
        match reply {
            Reply::Opened { generation, result } => {
                if generation == self.generation.current() {
                    self.opened(result);
                }
            }
            Reply::Page {
                generation,
                page,
                result,
            } => {
                if generation == self.generation.current() {
                    self.page_answered(page.get(), result);
                }
            }
        }
    }

    /// Whether there are requests that the worker has not been given yet.
    /// The drawing code starts the worker thread the first time this is
    /// true, so a window that never opens a file here never starts it.
    pub fn has_requests(&self) -> bool {
        !self.outbox.is_empty()
    }

    /// Hands over the requests for the worker, in the order they were
    /// made, and keeps none.
    pub fn take_requests(&mut self) -> Vec<Request> {
        // `take` swaps the list for an empty one and returns the full one:
        // no request is copied.
        std::mem::take(&mut self.outbox)
    }

    /// Hands over the images that arrived since the last call, each with
    /// its page number, and keeps none. The drawing code does not call
    /// this itself: it gets the images through [`Self::update_pictures`],
    /// which also says which of its pictures must go.
    pub fn take_fresh(&mut self) -> Vec<(usize, PageImage)> {
        std::mem::take(&mut self.fresh)
    }

    /// Brings `pictures` in line with this state: when it returns, there
    /// is a picture for every page that is drawn, made from the image of
    /// that page, and none for any other page.
    ///
    /// `pictures` is whatever the drawing code shows a page with, by page
    /// number, and `make` turns the image of a page into one. In the
    /// window a picture is a texture of egui; here it is any type `P`, so
    /// the rule is tested without a window.
    ///
    /// Two steps:
    ///
    /// 1. The picture of every page that this state does not hold as
    ///    drawn is dropped: the file was replaced, the page was forgotten
    ///    to save memory, or it is being drawn again at another size.
    /// 2. Every image that arrived since the last call becomes the
    ///    picture of its page, in the place of any picture that was there.
    ///
    /// A page number means another page once another file is open, and
    /// the picture of the old page must never be shown for the new one. It
    /// cannot be, however many frames go by between two calls and whatever
    /// happened in them. A page is only held as drawn from the moment its
    /// image arrives, and that image waits here until this is called. So
    /// for a page that is drawn now, either its image is handed over in
    /// this very call, and step 2 replaces what was there, or it was
    /// handed over in an earlier one and the page has been drawn ever
    /// since, and the picture made then is still the right one.
    pub fn update_pictures<P>(
        &mut self,
        pictures: &mut BTreeMap<usize, P>,
        mut make: impl FnMut(usize, PageImage) -> P,
    ) {
        pictures.retain(|page, _| self.slots.get(page) == Some(&Slot::Ready));
        for (page, image) in self.take_fresh() {
            pictures.insert(page, make(page, image));
        }
    }

    /// Whether the grid must be shown from its first row, whatever row it
    /// was left at. True once after every file that is chosen: the call
    /// that says so also lowers the flag.
    ///
    /// The drawing code asks when it draws the grid, which is not the
    /// frame in which the file was chosen (the file is still being opened
    /// then), so the answer has to wait here until it is asked for. The
    /// reason it is needed at all is in the drawing code: the scroll area
    /// remembers how far it was scrolled, and knows nothing about files.
    pub fn take_scroll_to_top(&mut self) -> bool {
        // `take` on a `bool` leaves `false` behind.
        std::mem::take(&mut self.scroll_to_top)
    }

    /// Records that the worker thread has ended by itself. Whatever was
    /// being shown is closed: no request could be answered any more.
    /// Choosing a file starts over, with a new worker.
    pub fn worker_stopped(&mut self) {
        self.close();
        self.status = Status::WorkerStopped;
    }

    /// Records that the worker thread could not be started. `error` is
    /// what the system said.
    ///
    /// The requests that were waiting are dropped, so the drawing code
    /// does not try to start a thread again on every frame: the next
    /// attempt is the next file the user chooses.
    pub fn worker_did_not_start(&mut self, error: &io::Error) {
        self.close();
        self.status = Status::WorkerNotStarted {
            message: error_chain(error),
        };
    }

    /// Gives `worker` the requests that are waiting and takes in every
    /// reply that has arrived. Neither waits: a frame calls this and goes
    /// on drawing.
    ///
    /// Returns `false` if the worker thread turned out to be gone, which
    /// is recorded as [`Self::worker_stopped`]. The caller drops that
    /// worker; the next request starts a new one.
    #[must_use = "a worker that is gone must be dropped and replaced"]
    pub fn exchange(&mut self, worker: &Worker) -> bool {
        for request in self.take_requests() {
            // Sending fails only when the other end of the channel is
            // gone: the worker thread has ended.
            if worker.send(request).is_err() {
                self.worker_stopped();
                return false;
            }
        }
        loop {
            match worker.try_recv() {
                Ok(reply) => self.receive(reply),
                // Nothing more for now.
                Err(TryRecvError::Empty) => return true,
                // Nothing more, ever: the worker dropped its sender.
                Err(TryRecvError::Disconnected) => {
                    self.worker_stopped();
                    return false;
                }
            }
        }
    }

    /// Records the answer to "open this file".
    fn opened(&mut self, result: Result<usize, RenderError>) {
        // `take` moves the document out and leaves `Document::None`
        // behind, so the path can be moved on into its next home without
        // a copy.
        let path = match std::mem::take(&mut self.document) {
            Document::Opening { path } => path,
            // An answer only counts while a file is being opened. In any
            // other state the document goes back as it was.
            other => {
                self.document = other;
                return;
            }
        };
        match result {
            // A document without pages has nothing to show, and "0 pages"
            // over an empty grid would look like a bug in the program.
            Ok(0) => {
                self.status = Status::NotAccepted {
                    file: path,
                    message: texts::NO_PAGES.to_owned(),
                };
            }
            // The status is left as it is: it may say which files were
            // left out, and that is still true.
            Ok(pages) => self.document = Document::Open { path, pages },
            Err(error) if is_about_the_library(&error) => {
                self.status = Status::NoEngine {
                    message: error_chain(&error),
                };
            }
            Err(error) => {
                self.status = Status::NotAccepted {
                    file: path,
                    message: reason(error),
                };
            }
        }
    }

    /// Records the answer about page `page` of the current generation.
    fn page_answered(&mut self, page: usize, result: Result<PageImage, RenderError>) {
        // Only a page that is waited for can be answered. `get_mut` lends
        // the slot for changing it in place.
        let Some(slot) = self.slots.get_mut(&page) else {
            return;
        };
        if *slot != Slot::Requested {
            return;
        }
        match result {
            Ok(image) => {
                *slot = Slot::Ready;
                self.fresh.push((page, image));
            }
            Err(error) => {
                *slot = Slot::Failed {
                    reason: reason(error),
                };
            }
        }
        self.forget_the_farthest();
    }

    /// Forgets the document and everything that was asked or answered
    /// about it, and starts a new generation.
    fn close(&mut self) {
        self.generation.advance();
        self.document = Document::None;
        self.slots.clear();
        self.fresh.clear();
        self.size = None;
        self.wanted = None;
        self.outbox.clear();
    }

    /// Cancels every page that is still waited for, by starting a new
    /// generation. The pages that were answered stay.
    ///
    /// Only called while a document is open: the requests still to go out
    /// are all page requests then, and all of them are dropped.
    fn cancel_requests(&mut self) {
        self.generation.advance();
        // `retain` keeps the entries for which the closure says `true`.
        self.slots.retain(|_, slot| *slot != Slot::Requested);
        self.outbox.clear();
    }

    /// Brings the number of answered pages down to [`MAX_KEPT`], by
    /// forgetting the ones farthest from the pages wanted now. A wanted
    /// page is never forgotten, so the number can stay above the limit in
    /// a window that shows more thumbnails than that.
    fn forget_the_farthest(&mut self) {
        let Some(excess) = self.kept().checked_sub(MAX_KEPT).filter(|more| *more > 0) else {
            return;
        };
        // Without wanted pages (the view is hidden) there is no "far":
        // nothing arrives then either, so the limit already holds.
        let Some(wanted) = self.wanted.clone() else {
            return;
        };

        // How many pages lie between `page` and the nearest wanted one.
        let distance = |page: usize| {
            if page < *wanted.start() {
                wanted.start() - page
            } else {
                page.saturating_sub(*wanted.end())
            }
        };
        let mut candidates: Vec<usize> = self
            .slots
            .iter()
            .filter(|(page, slot)| **slot != Slot::Requested && !wanted.contains(page))
            .map(|(page, _)| *page)
            .collect();
        // Farthest first. `Reverse` turns the order of the key around.
        candidates.sort_by_key(|page| std::cmp::Reverse(distance(*page)));

        for page in candidates.into_iter().take(excess) {
            self.slots.remove(&page);
            // Its image, if the drawing code has not taken it yet.
            self.fresh.retain(|(fresh, _)| *fresh != page);
        }
    }
}

/// Whether `error` says that the library that draws pages is not there,
/// and nothing about the file that was to be opened.
fn is_about_the_library(error: &RenderError) -> bool {
    // No `_ =>` arm, as in the command-line tool: a variant added to
    // `RenderError` must be sorted into one of the two groups before this
    // crate builds again.
    match error {
        RenderError::LibraryFolderUnknown { .. }
        | RenderError::LibraryMissing { .. }
        | RenderError::LibraryFailed { .. }
        | RenderError::LibraryAlreadyLoaded { .. } => true,
        RenderError::Read { .. }
        | RenderError::NotPdf { .. }
        | RenderError::Encrypted
        | RenderError::UnsupportedProtection
        | RenderError::Broken { .. }
        | RenderError::PageOutOfRange { .. }
        | RenderError::WidthOutOfRange { .. }
        | RenderError::Engine { .. }
        | RenderError::Save { .. } => false,
    }
}

/// Why the file on screen could not be opened, or one of its pages could
/// not be drawn: `error` and its causes, in the core's words, with the
/// file named the way this window names files.
///
/// The core names a file by the whole path it was given. That is what the
/// command-line tool wants, since a script has to find the file again. In
/// the window it is too much: every other line names a file by its name
/// alone and shows the path when the name is hovered, and a path makes a
/// status line run over three lines of a narrow window.
///
/// The sentence is not written again here, where it could drift from the
/// core's. The error is given the shorter path and the core words it, as
/// it words every other reason the window shows.
fn reason(error: RenderError) -> String {
    let name_only = |path: PathBuf| PathBuf::from(display_name(&path));
    let error = match error {
        RenderError::Read { path, source } => RenderError::Read {
            path: name_only(path),
            source,
        },
        RenderError::NotPdf { path } => RenderError::NotPdf {
            path: name_only(path),
        },
        // Every other kind either names no file, or names one that is not
        // the file on screen and is wanted in full: the library that is
        // missing, which the user has to go and find. (`is_about_the_library`
        // above is the function that has no such arm, so that a new kind
        // of error is looked at before this crate builds again.)
        other => other,
    };
    error_chain(&error)
}

/// The pages that `rows` hold, in a grid of `columns` thumbnails per row
/// and a document of `pages` pages. `None` when they hold none: the rows
/// are empty, or past the end of the document.
///
/// Rows count from 0 and pages from 1, so row 0 of four columns holds
/// pages 1 to 4.
fn pages_in_rows(
    rows: &Range<usize>,
    columns: usize,
    pages: usize,
) -> Option<RangeInclusive<usize>> {
    let columns = columns.max(1);
    // `saturating_*` stops at the largest number instead of wrapping
    // around: a row number that makes no sense gives no pages, not a
    // panic.
    let first = rows.start.saturating_mul(columns).saturating_add(1);
    let last = rows.end.saturating_mul(columns).min(pages);
    (first <= last).then_some(first..=last)
}

/// The size of a thumbnail in pixels of the screen, for a screen that has
/// `pixels_per_point` pixels in every egui point.
///
/// The view is laid out in points, so that it looks the same size on every
/// screen. A thumbnail drawn [`THUMBNAIL_WIDTH`] pixels wide would be
/// blurred on a screen with two pixels per point, where it covers twice as
/// many: it is drawn at the number of pixels it will really cover.
pub fn thumbnail_size(pixels_per_point: f32) -> ThumbnailSize {
    // `as` from a float to an integer cuts the decimals and stops at the
    // ends of the integer's range (a negative number or "not a number"
    // gives 0), and `ThumbnailSize::new` brings the result inside what
    // the core draws. So no scale, however absurd, gives a size that
    // cannot be drawn.
    let pixels = |points: f32| (points * pixels_per_point).round() as u32;
    ThumbnailSize::new(pixels(THUMBNAIL_WIDTH), pixels(THUMBNAIL_HEIGHT))
}

/// How many cells `cell_width` wide, with `spacing` between two of them,
/// fit side by side in `available_width`. At least one, so a window
/// narrower than a thumbnail still shows one per row.
pub fn columns_for(available_width: f32, cell_width: f32, spacing: f32) -> usize {
    // N cells take N widths and N - 1 spacings: adding one spacing to the
    // room makes it a plain division.
    let columns = ((available_width + spacing) / (cell_width + spacing)).floor();
    (columns as usize).max(1)
}

/// How far from the left edge of `available_width` the grid starts, so
/// that it stands in the middle of the room there is.
///
/// What is centred is the widest row. In a document of several rows that
/// is `columns` cells, and a last row that is not full starts under the
/// first column like the rows above it, as text does. A document with
/// fewer pages than one row holds has no full row: its few thumbnails are
/// centred by themselves, instead of standing on the left of a window
/// that is mostly empty.
///
/// Never negative: in a window narrower than one thumbnail the grid
/// starts at the edge.
pub fn grid_offset(
    available_width: f32,
    cell_width: f32,
    spacing: f32,
    columns: usize,
    pages: usize,
) -> f32 {
    // The cells of the widest row, and never none.
    let cells = columns.min(pages).max(1);
    // N cells take N widths and N - 1 spacings.
    let used = cells as f32 * (cell_width + spacing) - spacing;
    // `max` with a number that is "not a number" gives the other one, so
    // a width that makes no sense gives 0.
    ((available_width - used) / 2.0).max(0.0)
}

/// How many rows `pages` thumbnails take at `columns` per row. The last
/// row may not be full.
pub fn row_count(pages: usize, columns: usize) -> usize {
    // `div_ceil` divides and rounds up.
    pages.div_ceil(columns.max(1))
}

/// The size at which an image of `image` pixels (width, height) is shown
/// in a box `width` by `height`: as large as fits, keeping its shape.
pub fn fit_within(image: [usize; 2], width: f32, height: f32) -> (f32, f32) {
    let [image_width, image_height] = image.map(|side| side as f32);
    if image_width <= 0.0 || image_height <= 0.0 {
        return (0.0, 0.0);
    }
    // The smaller of the two scales is the one at which both sides fit.
    let scale = (width / image_width).min(height / image_height);
    (image_width * scale, image_height * scale)
}

#[cfg(test)]
mod tests;
