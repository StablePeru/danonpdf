//! The state behind the merge window, with no drawing in it.
//!
//! egui redraws the whole window every frame from whatever state it is
//! given (see `app.rs`). Keeping that state, and every rule about how it may
//! change, in this plain struct means the rules can be tested without a
//! window, and the drawing code stays a thin layer that reads the state and
//! reports clicks.

use std::error::Error;
use std::path::{Path, PathBuf};

use danonpdf_core::{MergeError, is_pdf_path, merge_files};

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
    /// The last merge failed. The message already holds the whole chain of
    /// causes, because `MergeError` cannot be cloned or kept around cheaply.
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
    /// the folder of the first file. `None` while the list is empty.
    pub fn default_output_path(&self) -> Option<PathBuf> {
        // `first()` is `None` for an empty list, and `map` only runs the
        // closure when there is a value: no `if` and no unwrapping needed.
        // `with_file_name` keeps the folder and swaps the name.
        self.files
            .first()
            .map(|first| first.with_file_name(texts::DEFAULT_OUTPUT_NAME))
    }

    /// Merges the files into `output` and records the outcome in the status.
    /// Does nothing unless [`Self::can_merge`].
    ///
    /// This runs on the thread that draws the window, so the window does not
    /// answer until the merge ends: a moment for everyday files, but several
    /// seconds for big scans. Moving the work to a background thread is a
    /// later task (T8).
    pub fn merge_into(&mut self, output: PathBuf) {
        if !self.can_merge() {
            return;
        }
        // `&self.files` lends the list to the core, which only reads it.
        let result = merge_files(&self.files, &output);
        self.record_merge(output, result);
    }

    /// Records the outcome of a merge into `output`.
    ///
    /// Kept apart from [`Self::merge_into`] so that tests can feed it any
    /// outcome, including errors that are hard to cause with real files.
    pub fn record_merge(&mut self, output: PathBuf, result: Result<usize, MergeError>) {
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

/// `error`'s message followed by the message of each error behind it,
/// joined with ": ". For example: "cannot save out.pdf: access denied".
pub fn error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    // `source()` gives the error behind this one, if any. Walking it until
    // it returns `None` visits the whole chain, however long it is.
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    message
}

#[cfg(test)]
mod tests;
