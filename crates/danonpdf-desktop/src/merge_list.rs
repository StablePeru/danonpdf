//! The state behind the merge window, with no drawing in it.
//!
//! egui redraws the whole window every frame from whatever state it is
//! given (see `app.rs`). Keeping that state, and every rule about how it may
//! change, in this plain struct means the rules can be tested without a
//! window, and the drawing code stays a thin layer that reads the state and
//! reports clicks.

use std::path::{Path, PathBuf};

use danonpdf_core::{PdfError, error_chain, is_pdf_path, merge_files, name_is_taken, same_file};

use crate::texts;

/// The files to merge, in merge order, and the outcome of the last action.
///
/// `Default` gives an empty list with an idle status, which is exactly how
/// the window starts.
#[derive(Debug, Default)]
pub struct MergeList {
    files: Vec<PathBuf>,
    status: Status,
}

/// What the status line at the bottom of the window says.
///
/// An enum and not, say, a `String` plus a few flags: each variant carries
/// exactly the data it needs, and a `match` on it must handle every case, so
/// adding a new kind of status later makes the compiler point at every place
/// that has to show it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing to report.
    #[default]
    Idle,
    /// Some of the files just added were left out.
    Skipped {
        /// Files whose name does not end in `.pdf`.
        not_pdf: usize,
        /// Files that were already in the list.
        duplicates: usize,
    },
    /// The last merge worked.
    Merged { pages: usize, output: PathBuf },
    /// The merge was refused because `output` is one of the files in the
    /// list: writing there would replace a file before it is read.
    OutputIsInput { output: PathBuf },
    /// The merge was refused because ".pdf" had to be added to the name the
    /// user typed, and `output` (the name with ".pdf") already exists.
    ///
    /// The save dialog only asks "replace?" for the name it returns. On Linux
    /// that is the name as typed ("report"), so nobody confirmed replacing
    /// "report.pdf"; writing there would destroy an unrelated file silently.
    ExistsAfterAddingExtension { output: PathBuf },
    /// The last merge failed. The message already holds the whole chain of
    /// causes, because `PdfError` cannot be cloned or kept around cheaply.
    Failed { message: String },
}

/// A click on one row of the list.
///
/// The drawing code cannot change the list while it is still walking through
/// it (that would be a mutable borrow during an immutable one), so it only
/// remembers which button was clicked and applies it after the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    MoveUp(usize),
    MoveDown(usize),
    Remove(usize),
}

impl MergeList {
    /// The files, in the order they will be merged.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Appends `paths` to the end of the list, leaving out every path that
    /// does not name a PDF and every path already in the list.
    ///
    /// Duplicates are left out because adding the same file twice is far
    /// more often an accident (the same files dropped again, or picked with
    /// the button after being dropped) than a wish, and two identical rows
    /// would only be confusing. Paths are compared as given, without asking
    /// the disk, so `C:\a.pdf` and `c:\A.PDF` count as different files.
    ///
    /// `impl IntoIterator` accepts a `Vec`, an array or any iterator, so the
    /// caller does not have to build a particular collection first.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut not_pdf = 0;
        let mut duplicates = 0;
        for path in paths {
            if !is_pdf_path(&path) {
                not_pdf += 1;
            } else if self.files.contains(&path) {
                // `contains` walks the whole list, so adding is slower the
                // longer the list. With the tens or hundreds of files a person
                // merges by hand, that is still far below a millisecond.
                duplicates += 1;
            } else {
                // `path` is moved into the list: no copy of the path.
                self.files.push(path);
            }
        }

        self.status = if not_pdf == 0 && duplicates == 0 {
            Status::Idle
        } else {
            Status::Skipped {
                not_pdf,
                duplicates,
            }
        };
    }

    /// Swaps the file at `index` with the one above it. Does nothing for the
    /// first file or an index past the end.
    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.files.len() {
            self.files.swap(index - 1, index);
            self.list_changed();
        }
    }

    /// Swaps the file at `index` with the one below it. Does nothing for the
    /// last file or an index past the end.
    pub fn move_down(&mut self, index: usize) {
        // Not `index + 1 < len`: for the largest `usize` that addition would
        // overflow. `saturating_sub` stops at 0 instead of going below it,
        // so an empty list gives `index < 0`, which is always false.
        if index < self.files.len().saturating_sub(1) {
            self.files.swap(index, index + 1);
            self.list_changed();
        }
    }

    /// Takes the file at `index` out of the list. Does nothing for an index
    /// past the end.
    pub fn remove(&mut self, index: usize) {
        // `Vec::remove` panics on a bad index, so check first.
        if index < self.files.len() {
            self.files.remove(index);
            self.list_changed();
        }
    }

    /// Empties the list.
    pub fn clear(&mut self) {
        self.files.clear();
        self.list_changed();
    }

    /// Applies one click from the list.
    pub fn apply(&mut self, action: RowAction) {
        match action {
            RowAction::MoveUp(index) => self.move_up(index),
            RowAction::MoveDown(index) => self.move_down(index),
            RowAction::Remove(index) => self.remove(index),
        }
    }

    /// Whether there is anything worth merging: at least two files.
    pub fn can_merge(&self) -> bool {
        self.files.len() >= 2
    }

    /// Where the save dialog proposes to write the result: `merged.pdf` in
    /// the folder of the first file, or `merged (2).pdf`, `merged (3).pdf`…
    /// when that name is already one of the files in the list. `None` while
    /// the list is empty.
    ///
    /// Proposing an input would be a trap: merge a and b into merged.pdf,
    /// then merged.pdf with c, and one click on "replace" in the dialog
    /// would overwrite a file that is still to be read.
    pub fn default_output_path(&self) -> Option<PathBuf> {
        // `?` on an `Option` returns `None` from the whole function when the
        // list is empty, so the rest only runs when there is a first file.
        let first = self.files.first()?;
        // `1..` counts up without end, and `find` stops at the first number
        // whose path is free. It always stops: the list is finite, so it
        // cannot hold every name. `with_file_name` keeps the folder and
        // swaps the name.
        (1..)
            .map(|attempt| first.with_file_name(texts::output_name(attempt)))
            .find(|candidate| !self.is_input(candidate))
    }

    /// What the save dialog starts with: the folder and file name of
    /// [`Self::default_output_path`]. Empty while the list is empty.
    pub fn save_suggestion(&self) -> SaveSuggestion {
        self.default_output_path()
            .map(|path| SaveSuggestion::for_path(&path))
            .unwrap_or_default()
    }

    /// Merges the files into `output` and records the outcome in the status.
    /// Does nothing unless [`Self::can_merge`].
    ///
    /// `output` gets ".pdf" added when it has no extension at all (see
    /// [`with_pdf_extension`]). The merge is refused, and the core is never
    /// called, when `output` is one of the files in the list, or when ".pdf"
    /// was added and a file with the new name already exists.
    ///
    /// This runs on the thread that draws the window, so the window does not
    /// answer until the merge ends: a moment for everyday files, but several
    /// seconds for big scans. A later change will move merging to a
    /// background thread.
    pub fn merge_into(&mut self, output: PathBuf) {
        if !self.can_merge() {
            return;
        }
        // The extension first, so that typing "a" when "a.pdf" is in the
        // list is caught by the checks below. The typed name is kept (a
        // `clone` of a short path costs next to nothing) to tell whether
        // `with_pdf_extension` changed it, without repeating its rule here.
        let typed = output.clone();
        let output = with_pdf_extension(output);
        if self.is_input(&output) {
            self.status = Status::OutputIsInput { output };
            return;
        }
        // The dialog asked "replace?" only for the name it returned. When
        // that name was changed, an existing file under the new name was
        // never confirmed, so refuse instead of replacing it silently. On
        // Windows the dialog adds ".pdf" itself, so this only fires on
        // systems whose dialog returns the name as typed, such as Linux.
        //
        // `name_is_taken` is the core's rule, the one the command-line tool
        // uses for `--force`. Unlike `Path::exists` it also says yes for a
        // link whose target is gone: saving would replace that link too.
        if output != typed && name_is_taken(&output) {
            self.status = Status::ExistsAfterAddingExtension { output };
            return;
        }
        // `&self.files` lends the list to the core, which only reads it.
        let result = merge_files(&self.files, &output);
        self.record_merge(output, result);
    }

    /// Whether `path` names one of the files in the list, however it is
    /// spelled. [`same_file`] is the core's rule, shared with the
    /// command-line tool so both refuse exactly the same outputs.
    fn is_input(&self, path: &Path) -> bool {
        self.files.iter().any(|file| same_file(file, path))
    }

    /// Records the outcome of a merge into `output`.
    ///
    /// Kept apart from [`Self::merge_into`] so that tests can feed it any
    /// outcome, including errors that are hard to cause with real files.
    /// Private: the tests are a child module (`mod tests` below), and a
    /// child module can see its parent's private items.
    fn record_merge(&mut self, output: PathBuf, result: Result<usize, PdfError>) {
        self.status = match result {
            Ok(pages) => Status::Merged { pages, output },
            Err(error) => Status::Failed {
                message: error_chain(&error),
            },
        };
    }

    /// Any change to the list makes the old status stale: a "merged 3 pages"
    /// or "skipped 1 file" line would describe a list that no longer exists.
    fn list_changed(&mut self) {
        self.status = Status::Idle;
    }
}

/// Where the save dialog opens and the name it proposes.
///
/// rfd takes the folder and the file name separately. Splitting the path
/// here, and not in the drawing code, keeps the rules about empty folders
/// and odd names where they can be tested.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SaveSuggestion {
    /// The folder to open the dialog in. `None` lets the system choose.
    pub folder: Option<PathBuf>,
    /// The name already typed in the dialog. `None` leaves it empty.
    pub file_name: Option<String>,
}

impl SaveSuggestion {
    /// Splits `path` into the folder to open and the name to propose.
    pub fn for_path(path: &Path) -> Self {
        // A bare file name such as "merged.pdf" has an empty folder (""),
        // which is not a place to open the dialog in; `filter` turns it
        // into `None`. `to_path_buf` copies the borrowed folder out.
        let folder = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(Path::to_path_buf);
        // rfd wants the name as text. `to_string_lossy` replaces bytes that
        // are not valid UTF-8 with "�": good enough for a proposal the
        // user can still edit.
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        Self { folder, file_name }
    }
}

/// `path` with ".pdf" added when its file name has no extension at all.
///
/// The Windows save dialog adds ".pdf" itself, but the Linux file chooser
/// returns the name exactly as typed, and a merged PDF called "report" is
/// hard to open with a double click. A name with another extension
/// ("report.txt") is left as the user typed it: that looks deliberate.
///
/// "Has an extension" follows std's [`Path::extension`], which can surprise:
///
/// - "v1.2 notes" has one ("2 notes", everything after the last dot), so it
///   stays as typed.
/// - ".report" has none (a leading dot starts a hidden name), so it becomes
///   ".report.pdf".
/// - "report." has an empty one, which still counts, so it stays as typed.
pub fn with_pdf_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        // `with_extension` builds a new path; the old one is dropped.
        path.with_extension("pdf")
    } else {
        path
    }
}

/// The text shown for `path` in the list: its file name, or the whole path
/// if it has no file name (a path such as `/` or `..`).
pub fn display_name(path: &Path) -> String {
    // `to_string_lossy` turns a name that is not valid UTF-8 into text by
    // replacing the bad bytes with "�", which is fine for showing it. The
    // path itself, used to open the file, stays untouched.
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests;
