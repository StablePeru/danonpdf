//! The state behind the Split view, with no drawing in it.
//!
//! The view works on one PDF at a time and offers two things to do with it:
//! copy some of its pages into one new file (extract), or write all of its
//! pages into several files (split). This struct holds the chosen file, what
//! the user typed for each of the two, and the outcome of the last action,
//! together with every rule about how they may change.
//!
//! As with `merge_list.rs`, keeping this apart from the drawing code
//! (`split_view.rs`) means the rules are tested without a window. The PDF
//! work itself is the core's: the page syntax, the names of the split
//! files and the operations all come from `danonpdf_core`, the same ones
//! the command-line tool uses.

use std::num::NonZeroUsize;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use danonpdf_core::{
    PageSelection, PdfError, SplitError, error_chain, extract_file, is_pdf_path, name_is_taken,
    page_count, plan_split, same_file, split_output_paths,
};

use crate::paths::{
    OutputRefusal, SaveSuggestion, display_name, first_free_path, output_from_dialog,
};
use crate::texts;

/// Everything the Split view remembers from one frame to the next.
///
/// Some fields are worked out from others (`pages` from the text and the
/// file, `preview` from the file and `pages_per_file`). They are stored, and
/// not computed each time they are drawn, because drawing happens on every
/// frame and working them out is not free: checking `1-` against a long
/// document lists its pages, and the preview builds every file name. The
/// private `refresh_*` methods are the only places that write them, and
/// every method that changes an input calls the one it needs.
#[derive(Debug)]
pub struct SplitForm {
    file: Option<ChosenFile>,
    mode: Mode,
    /// What is typed in the pages field, exactly as typed.
    pages_text: String,
    /// What `pages_text` means for `file`.
    pages: PagesCheck,
    /// A [`NonZeroUsize`], a number that cannot be 0, because that is what
    /// the core takes: "a file every 0 pages" cannot even be stored here.
    pages_per_file: NonZeroUsize,
    preview: Option<SplitPreview>,
    status: Status,
}

/// The PDF the view works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenFile {
    pub path: PathBuf,
    /// How many pages it had when it was chosen. The file is read again for
    /// every operation, so if it changes in between, the core answers for
    /// the file as it is then.
    pub pages: usize,
}

/// Which of the two things the view is set to do.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Copy chosen pages into one new file.
    #[default]
    Extract,
    /// Write all the pages into several files.
    Split,
}

/// What "Extract…" is waiting for while it cannot be clicked, so the
/// window can name the next thing to do and not just a thing that is
/// missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractNeeds {
    /// No file is chosen yet.
    File,
    /// A file is chosen, but the text does not select pages of it: it is
    /// empty, or it cannot be used.
    Pages,
}

/// What the text in the pages field means for the chosen file.
///
/// An enum and not a `bool` plus a message: the selection only exists when
/// the text is valid, and the message only when it is not, and here neither
/// can be read in the wrong case.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum PagesCheck {
    /// Nothing to judge yet: no text was typed, or no file is chosen to
    /// check it against. Not a mistake to point at, and nothing to extract.
    #[default]
    Pending,
    /// The text selects `pages` pages of the chosen file. `selection` is
    /// kept so that extracting does not read the text a second time.
    Selected {
        selection: PageSelection,
        pages: usize,
    },
    /// The text cannot be used, and `message` says why in the core's words:
    /// a mistake in the text itself, or a page the file does not have.
    Invalid { message: String },
}

/// What a split would write, for the line shown under "pages per file".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPreview {
    /// How many pages the chosen file has.
    pub pages: usize,
    /// How many files they go into.
    pub files: usize,
    /// The name of the first file, without a folder: that is only chosen
    /// when the user clicks "Split…".
    pub first: String,
    /// The name of the last file. The same as `first` when there is one.
    pub last: String,
}

/// What the status line at the bottom of the Split view says.
///
/// One variant per thing that can be said, each with exactly the data its
/// sentence needs (see `Status` in `merge_list.rs` for why an enum).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing to report.
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
    /// `file` could not be read as a PDF that can be worked on (it needs a
    /// password, it is damaged, it is not a PDF), so it was not chosen.
    NotAccepted { file: PathBuf, message: String },
    /// The last extraction worked.
    Extracted { pages: usize, output: PathBuf },
    /// The last split worked.
    SplitDone {
        files: usize,
        pages: usize,
        folder: PathBuf,
    },
    /// Nothing was written because `output` is the chosen file itself.
    /// `mode` says which operation was refused.
    OutputIsInput { mode: Mode, output: PathBuf },
    /// The extraction was refused because ".pdf" had to be added to the
    /// name the user typed, and `output` (the name with ".pdf") already
    /// exists. Why that is refused is told on `OutputRefusal`, in
    /// `paths.rs`.
    ExistsAfterAddingExtension { output: PathBuf },
    /// The split was refused, and nothing was written, because `output`,
    /// the first of its names found in use, already exists. The window has
    /// no way to say "replace them", so the way out is another folder.
    NameTaken { output: PathBuf },
    /// The split stopped after `written` of its `parts` files. Those are
    /// complete and stay in `folder`; `message` is what stopped it.
    SplitStopped {
        written: usize,
        parts: usize,
        folder: PathBuf,
        message: String,
    },
    /// The operation `mode` failed before writing anything. The message
    /// already holds the whole chain of causes.
    Failed { mode: Mode, message: String },
}

// `Default` is written by hand because `NonZeroUsize` has no default of its
// own (0 is the usual one, and 0 is the value it cannot hold), so
// `#[derive(Default)]` is not available for the struct.
impl Default for SplitForm {
    /// No file, "Extract pages" selected, nothing typed, one page per file.
    fn default() -> Self {
        Self {
            file: None,
            mode: Mode::default(),
            pages_text: String::new(),
            pages: PagesCheck::default(),
            // `MIN` is the smallest value of the type: 1.
            pages_per_file: NonZeroUsize::MIN,
            preview: None,
            status: Status::default(),
        }
    }
}

impl SplitForm {
    /// The file the view works on. `None` until one is chosen.
    pub fn file(&self) -> Option<&ChosenFile> {
        // `as_ref` turns `&Option<ChosenFile>` into `Option<&ChosenFile>`:
        // the caller gets to look at the file without taking it away.
        self.file.as_ref()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The text of the pages field, exactly as typed.
    pub fn pages_text(&self) -> &str {
        &self.pages_text
    }

    /// What that text means for the chosen file.
    pub fn pages_check(&self) -> &PagesCheck {
        &self.pages
    }

    pub fn pages_per_file(&self) -> usize {
        self.pages_per_file.get()
    }

    /// What a split would write now. `None` while no file is chosen.
    pub fn split_preview(&self) -> Option<&SplitPreview> {
        self.preview.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Makes the first PDF among `paths` the file to work on, in place of
    /// the one chosen before.
    ///
    /// The view takes one file at a time, so of several files (dropped
    /// together, say) only the first whose name ends in `.pdf` is used, and
    /// the status says how many were left out. With no PDF among them,
    /// nothing changes.
    ///
    /// The file is read here, with the core's `page_count`, which refuses
    /// what the operations refuse: a file that needs a password, a damaged
    /// one, one that is not a PDF. Such a file is not accepted: the file
    /// chosen before stays, and the status says why. The next PDF in
    /// `paths` is not tried instead, so that "the first one is used" has no
    /// exceptions to explain.
    ///
    /// What was typed (the pages, the pages per file) is kept and checked
    /// again against the new file.
    ///
    /// Reading the file happens on the thread that draws the window, like
    /// the operations themselves: see [`Self::extract_into`].
    pub fn choose(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut first_pdf = None;
        let mut other_pdfs = 0;
        let mut not_pdf = 0;
        for path in paths {
            if !is_pdf_path(&path) {
                not_pdf += 1;
            } else if first_pdf.is_none() {
                // `path` is moved into the `Option`: no copy of the path.
                first_pdf = Some(path);
            } else {
                other_pdfs += 1;
            }
        }

        // `let ... else` takes the path out of the `Option`, or runs the
        // `else` block, which must leave the function.
        let Some(path) = first_pdf else {
            // No path at all (an empty drop) is nothing to report.
            if not_pdf > 0 {
                self.status = Status::NoPdf { not_pdf };
            }
            return;
        };

        match page_count(&path) {
            Ok(pages) => {
                self.file = Some(ChosenFile { path, pages });
                // Everything that was worked out from the old file.
                self.refresh_pages_check();
                self.pages_per_file = self.clamped_pages_per_file(self.pages_per_file.get());
                self.refresh_preview();
                self.status = if other_pdfs == 0 && not_pdf == 0 {
                    Status::Idle
                } else {
                    Status::LeftOut {
                        other_pdfs,
                        not_pdf,
                    }
                };
            }
            Err(error) => {
                self.status = Status::NotAccepted {
                    file: path,
                    message: error_chain(&error),
                };
            }
        }
    }

    /// Switches between extracting and splitting. What was typed for the
    /// other mode is kept for when the user comes back to it.
    pub fn set_mode(&mut self, mode: Mode) {
        // Only a real change counts: egui also reports a click on the radio
        // button that is already selected, and that must not clear the
        // status line.
        if mode != self.mode {
            self.mode = mode;
            self.form_changed();
        }
    }

    /// Replaces the text of the pages field and checks it against the
    /// chosen file (see [`PagesCheck`]).
    ///
    /// Takes the `String` by value: the drawing code has an edited copy it
    /// no longer needs, so it is moved in here instead of being copied
    /// again.
    pub fn set_pages_text(&mut self, text: String) {
        if text != self.pages_text {
            self.pages_text = text;
            self.refresh_pages_check();
            self.form_changed();
        }
    }

    /// The values "pages per file" may take: from 1 to the page count of
    /// the chosen file, and only 1 while there is none.
    ///
    /// More pages per file than the document has would mean the same as
    /// all of them (one file), so the number stops at the page count
    /// instead of growing without telling the user anything new.
    pub fn pages_per_file_range(&self) -> RangeInclusive<usize> {
        // `map_or(1, ...)` gives 1 when there is no file, and otherwise the
        // result of the closure. No accepted file has 0 pages (the core
        // refuses it), but `max(1)` keeps the range from ever running
        // backwards if one did.
        let most = self.file.as_ref().map_or(1, |file| file.pages.max(1));
        1..=most
    }

    /// Sets how many pages go into each file of a split. A value outside
    /// [`Self::pages_per_file_range`] is brought to its nearest end, so 0
    /// becomes 1.
    pub fn set_pages_per_file(&mut self, pages: usize) {
        let pages = self.clamped_pages_per_file(pages);
        if pages != self.pages_per_file {
            self.pages_per_file = pages;
            self.refresh_preview();
            self.form_changed();
        }
    }

    /// What "Extract…" is still waiting for, or `None` when it has
    /// everything: a file, and a text that selects pages of it.
    ///
    /// The file comes first: while none is chosen the pages field cannot
    /// even be typed in, so "type the pages" would be advice the user
    /// cannot follow.
    pub fn extract_needs(&self) -> Option<ExtractNeeds> {
        if self.file.is_none() {
            Some(ExtractNeeds::File)
        } else if matches!(self.pages, PagesCheck::Selected { .. }) {
            None
        } else {
            Some(ExtractNeeds::Pages)
        }
    }

    /// Whether "Extract…" has everything it needs.
    pub fn can_extract(&self) -> bool {
        // Asked of `extract_needs` so that the button and the reason shown
        // on it while it is disabled can never disagree.
        self.extract_needs().is_none()
    }

    /// Whether "Split…" has everything it needs: a file. The number of
    /// pages per file is always usable.
    pub fn can_split(&self) -> bool {
        self.file.is_some()
    }

    /// Where the save dialog proposes to write an extraction:
    /// `report-pages.pdf` next to the chosen `report.pdf`, or
    /// `report-pages (2).pdf`, `report-pages (3).pdf`… when that name is
    /// taken. `None` while no file is chosen.
    ///
    /// "Taken" is the core's `name_is_taken`: a file, a folder or a link
    /// already has the name. A merge proposes `merged.pdf` even when it
    /// exists and lets the dialog ask; here the first choice would exist
    /// after every extraction from the same file, and proposing it again
    /// would make "replace?" the normal answer to a second extraction.
    ///
    /// This asks the disk, so it is called when the button is clicked and
    /// not on every frame.
    pub fn default_extract_path(&self) -> Option<PathBuf> {
        // `?` on an `Option` returns `None` from the whole function when
        // there is no file, or when its path has no name to build on.
        let file = self.file.as_ref()?;
        // `file_stem` is the file name without its last extension. It
        // becomes text because the proposal is text; a byte that is not
        // valid UTF-8 turns into "�" in a name the user can still edit.
        let stem = file.path.file_stem()?.to_string_lossy();
        let first_choice = file.path.with_file_name(texts::extract_output_name(&stem));
        first_free_path(&first_choice, |candidate| !name_is_taken(candidate))
    }

    /// What the save dialog starts with: the folder and file name of
    /// [`Self::default_extract_path`]. Empty while no file is chosen.
    pub fn extract_suggestion(&self) -> SaveSuggestion {
        self.default_extract_path()
            .map(|path| SaveSuggestion::for_path(&path))
            .unwrap_or_default()
    }

    /// Copies the selected pages into `output` and records the outcome in
    /// the status. Does nothing unless [`Self::can_extract`].
    ///
    /// `output` goes through the same rules as the output of a merge
    /// (`output_from_dialog` in `paths.rs`): ".pdf" is added to a name
    /// without an extension, and the extraction is refused, with the core
    /// never called, when the result is the chosen file itself or when
    /// ".pdf" was added and a file with the new name already exists.
    ///
    /// This runs on the thread that draws the window, so the window does
    /// not answer until it ends: a moment for everyday files, but several
    /// seconds for big scans. A later change will move the work to a
    /// background thread.
    pub fn extract_into(&mut self, output: PathBuf) {
        // Both must match for there to be anything to do. A tuple of two
        // references is matched against a tuple of two patterns; `..`
        // skips the page count, which is not needed here.
        let (Some(file), PagesCheck::Selected { selection, .. }) = (&self.file, &self.pages) else {
            return;
        };
        // The closure tells the shared rule what an input is here: the one
        // chosen file, however it is spelled (`same_file` is the core's
        // rule, the one the command-line tool uses).
        let output = match output_from_dialog(output, |path| same_file(&file.path, path)) {
            Ok(output) => output,
            Err(OutputRefusal::IsInput { output }) => {
                self.status = Status::OutputIsInput {
                    mode: Mode::Extract,
                    output,
                };
                return;
            }
            Err(OutputRefusal::ExistsAfterAddingExtension { output }) => {
                self.status = Status::ExistsAfterAddingExtension { output };
                return;
            }
        };
        let result = extract_file(&file.path, selection, &output);
        self.record_extract(output, result);
    }

    /// The folder the "choose a folder" dialog starts in: the one the
    /// chosen file is in. `None` while no file is chosen, or when its path
    /// names no folder; the system then picks where to start.
    pub fn split_folder_suggestion(&self) -> Option<PathBuf> {
        // The same split of a path as for a save dialog, and the same rule
        // about an empty folder; only the folder half is used.
        SaveSuggestion::for_path(&self.file.as_ref()?.path).folder
    }

    /// Splits the chosen file into files of [`Self::pages_per_file`] pages
    /// inside `folder`, and records the outcome in the status. Does nothing
    /// unless [`Self::can_split`].
    ///
    /// The core first reads the file and names the files it would write (a
    /// plan). Those names are checked here before any of them is written,
    /// in the order the command-line tool checks them: none may be the
    /// chosen file itself, and none may already exist. If one does, nothing
    /// is written, not even the files whose names were free. The window has
    /// no "replace them all" (the command line has `--force`): the way out
    /// is another folder, which is what the status says.
    ///
    /// The check looks first and writes later, so a file created by another
    /// program in between would still be replaced; the command-line tool
    /// accepts the same gap.
    ///
    /// This runs on the thread that draws the window, as
    /// [`Self::extract_into`] does: the window does not answer until every
    /// file is written. A later change will move the work to a background
    /// thread.
    pub fn split_into(&mut self, folder: PathBuf) {
        let Some(file) = &self.file else {
            return;
        };
        let plan = match plan_split(&file.path, self.pages_per_file, &folder) {
            Ok(plan) => plan,
            Err(error) => {
                self.status = Status::Failed {
                    mode: Mode::Split,
                    message: error_chain(&error),
                };
                return;
            }
        };

        // `find` gives the first output for which the test holds, or
        // `None`. Being the input comes first: an input always exists, so
        // with the other order that mistake would be reported as "the name
        // is taken", and "choose an empty folder" would be the wrong
        // advice. `clone` copies the one path that goes into the status.
        let outputs = plan.outputs();
        if let Some(output) = outputs.iter().find(|output| same_file(&file.path, output)) {
            self.status = Status::OutputIsInput {
                mode: Mode::Split,
                output: output.clone(),
            };
            return;
        }
        if let Some(output) = outputs.iter().find(|output| name_is_taken(output)) {
            self.status = Status::NameTaken {
                output: output.clone(),
            };
            return;
        }

        // Read before `write`, which takes the plan by value: afterwards
        // there is no plan left to ask.
        let pages = plan.pages();
        let result = plan.write();
        self.record_split(folder, pages, result);
    }

    /// `pages` brought into [`Self::pages_per_file_range`], as the type
    /// the core takes.
    fn clamped_pages_per_file(&self, pages: usize) -> NonZeroUsize {
        let range = self.pages_per_file_range();
        // `clamp` returns the nearest value inside the two bounds.
        let clamped = pages.clamp(*range.start(), *range.end());
        // The range starts at 1, so `new` never sees a 0 and the fallback
        // is never used; it is there so that no `unwrap` can panic.
        NonZeroUsize::new(clamped).unwrap_or(NonZeroUsize::MIN)
    }

    /// Works out [`PagesCheck`] again from the text and the chosen file.
    /// Called whenever one of the two changes.
    fn refresh_pages_check(&mut self) {
        self.pages = match &self.file {
            Some(file) => check_pages(&self.pages_text, file.pages),
            // No document to check the text against yet.
            None => PagesCheck::Pending,
        };
    }

    /// Works out the preview of a split again from the chosen file and the
    /// pages per file. Called whenever one of the two changes.
    fn refresh_preview(&mut self) {
        // `and_then` runs the closure only when there is a file, and the
        // closure may itself answer `None`.
        self.preview = self.file.as_ref().and_then(|file| {
            // `div_ceil` divides and rounds up: 5 pages in files of 2 make
            // 3 files. The core's plan counts the same way.
            let files = file.pages.div_ceil(self.pages_per_file.get());
            // The names come from the core, so the preview cannot promise
            // names the split would not write. An empty folder ("") leaves
            // bare file names. Every name is built to show two of them,
            // which is why this runs when the form changes and not on
            // every frame.
            let names = split_output_paths(&file.path, Path::new(""), files);
            Some(SplitPreview {
                pages: file.pages,
                files,
                first: display_name(names.first()?),
                last: display_name(names.last()?),
            })
        });
    }

    /// Records the outcome of an extraction into `output`.
    ///
    /// Kept apart from [`Self::extract_into`] so that tests can feed it any
    /// outcome, including errors that are hard to cause with real files.
    fn record_extract(&mut self, output: PathBuf, result: Result<usize, PdfError>) {
        self.status = match result {
            Ok(pages) => Status::Extracted { pages, output },
            Err(error) => Status::Failed {
                mode: Mode::Extract,
                message: error_chain(&error),
            },
        };
    }

    /// Records the outcome of a split of `pages` pages into `folder`.
    ///
    /// Kept apart from [`Self::split_into`] for the same reason as
    /// [`Self::record_extract`]: every name is checked before the first
    /// file is written, so a split that stops halfway is hard to cause in a
    /// test.
    fn record_split(
        &mut self,
        folder: PathBuf,
        pages: usize,
        result: Result<Vec<PathBuf>, SplitError>,
    ) {
        self.status = match result {
            Ok(outputs) => Status::SplitDone {
                files: outputs.len(),
                pages,
                folder,
            },
            // The error says what stopped the split (`stopped.error`) and
            // lists the files it had finished. The status line builds its
            // own sentence from the counts, so only the cause is kept as
            // text.
            Err(stopped) => Status::SplitStopped {
                written: stopped.written.len(),
                parts: stopped.parts,
                folder,
                message: error_chain(&stopped.error),
            },
        };
    }

    /// Any change to the form makes the old status stale: an "extracted 3
    /// pages" line would describe a form that no longer exists.
    fn form_changed(&mut self) {
        self.status = Status::Idle;
    }
}

/// What `text` means as a page selection for a document of `page_count`
/// pages.
///
/// The two steps are the core's (see `pages.rs` there): parsing finds the
/// mistakes in the text itself, and resolving finds the pages that this
/// document does not have. Both messages are shown as the core words them,
/// so the window and the command-line tool explain a mistake the same way.
fn check_pages(text: &str, page_count: usize) -> PagesCheck {
    // The core would call this a mistake ("no pages were given"). In a
    // form, an empty field is only a field nobody has filled in yet.
    if text.trim().is_empty() {
        return PagesCheck::Pending;
    }
    // `parse` knows what to build from the type written on the left.
    let selection: PageSelection = match text.parse() {
        Ok(selection) => selection,
        Err(error) => {
            return PagesCheck::Invalid {
                message: error.to_string(),
            };
        }
    };
    // `resolve` lists the selected pages, which is how they are counted.
    // The list is never longer than the document (no page can be selected
    // twice), and this runs once per change of the text, not per frame.
    match selection.resolve(page_count) {
        Ok(pages) => PagesCheck::Selected {
            pages: pages.len(),
            selection,
        },
        Err(error) => PagesCheck::Invalid {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests;
