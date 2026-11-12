//! Files dragged from the file explorer onto the window: reading the ones
//! that were let go of, and showing that letting go will do something.
//!
//! Every view takes dropped files, each in its own way (the merge list adds
//! them, the Split and Pages views choose one), but they read them and
//! light up the same way, so that part lives here. So does the rule that
//! the two views that work on one file share: which one of several files
//! they take ([`one_pdf_of`]).

use std::path::PathBuf;

use danonpdf_core::is_pdf_path;
use eframe::egui;

/// What a view that works on one file makes of the files it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneOfSeveral {
    /// The file to use: the first one whose name ends in `.pdf`. `None`
    /// when there is no such file.
    pub pdf: Option<PathBuf>,
    /// PDFs after the first one, which are left out.
    pub other_pdfs: usize,
    /// Files whose name does not end in `.pdf`.
    pub not_pdf: usize,
}

/// Picks the one PDF that a view which takes one file at a time uses out
/// of `paths`, and counts the rest, so the view can say what it left out.
///
/// Several files can arrive together (a drop), and such a view can only
/// take one. It is always the first whose name ends in `.pdf`, whatever
/// becomes of it afterwards: a view never moves on to the next one when
/// the first cannot be opened, so that "the first one is used" has no
/// exceptions to explain.
///
/// Only names are looked at here, as in `is_pdf_path`: nothing is read.
pub fn one_pdf_of(paths: impl IntoIterator<Item = PathBuf>) -> OneOfSeveral {
    let mut chosen = OneOfSeveral {
        pdf: None,
        other_pdfs: 0,
        not_pdf: 0,
    };
    for path in paths {
        if !is_pdf_path(&path) {
            chosen.not_pdf += 1;
        } else if chosen.pdf.is_none() {
            // `path` is moved into the `Option`: no copy of the path.
            chosen.pdf = Some(path);
        } else {
            chosen.other_pdfs += 1;
        }
    }
    chosen
}

/// The files dropped onto the window during this frame. Empty on nearly
/// every frame.
pub fn dropped_files(ui: &egui::Ui) -> Vec<PathBuf> {
    // `input` lends egui's input state to the closure while holding a lock
    // on it, so the closure only copies the paths out; the caller changes
    // its own state after the lock is released.
    ui.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    })
}

/// How a drop zone looks during this frame.
pub struct DropZone {
    /// Whether files are being dragged over the window right now.
    pub hovering: bool,
    /// The border to draw around the zone: highlighted while `hovering`,
    /// the usual thin line otherwise.
    pub border: egui::Stroke,
}

/// Looks at whether files are being dragged over the window, and picks the
/// border for a drop zone drawn in `ui`.
///
/// While files are dragged over the window, egui lists them in
/// `hovered_files`. The border lights up so the user sees that letting go
/// will do something.
pub fn drop_zone(ui: &egui::Ui) -> DropZone {
    let hovering = ui.input(|input| !input.raw.hovered_files.is_empty());
    let visuals = ui.visuals();
    let border = if hovering {
        egui::Stroke::new(2.0, visuals.selection.bg_fill)
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    DropZone { hovering, border }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A path inside a folder called `docs`. Only names are looked at, so
    /// nothing has to exist there.
    fn doc(name: &str) -> PathBuf {
        Path::new("docs").join(name)
    }

    #[test]
    fn the_first_pdf_is_the_one_and_the_rest_are_counted() {
        let chosen = one_pdf_of([
            doc("notes.txt"),
            doc("a.pdf"),
            doc("b.PDF"),
            doc("photo.jpg"),
            doc("c.pdf"),
        ]);

        assert_eq!(
            chosen,
            OneOfSeveral {
                pdf: Some(doc("a.pdf")),
                other_pdfs: 2,
                not_pdf: 2,
            }
        );
    }

    #[test]
    fn without_a_pdf_there_is_no_file_to_use() {
        let chosen = one_pdf_of([doc("notes.txt"), doc("photo.jpg")]);

        assert_eq!(
            chosen,
            OneOfSeveral {
                pdf: None,
                other_pdfs: 0,
                not_pdf: 2,
            }
        );
    }

    #[test]
    fn no_files_at_all_count_as_nothing() {
        // `[PathBuf; 0]`: the type of an array of no paths has to be
        // written out, because there is no element to infer it from.
        let nothing: [PathBuf; 0] = [];

        assert_eq!(
            one_pdf_of(nothing),
            OneOfSeveral {
                pdf: None,
                other_pdfs: 0,
                not_pdf: 0,
            }
        );
    }
}
