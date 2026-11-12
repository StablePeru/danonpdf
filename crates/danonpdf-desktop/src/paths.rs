//! What the window does with paths, whichever view is asking: how a file is
//! named on screen, which name a save dialog proposes, and what happens to
//! the path a save dialog gives back.
//!
//! Every view that saves one file goes through the same three steps: propose
//! a name that is free ([`first_free_path`], [`SaveSuggestion`]), let the
//! user choose, and check the answer before writing ([`output_from_dialog`]).
//! The steps live here once, so a rule such as "never write over an input"
//! cannot be kept in one view and forgotten in the next.
//!
//! Nothing here draws or opens a dialog, so all of it is tested without a
//! window.

use std::path::{Path, PathBuf};

use danonpdf_core::name_is_taken;

use crate::texts;

/// The text shown for `path` in the window: its file name, or the whole
/// path if it has no file name (a path such as `/` or `..`).
pub fn display_name(path: &Path) -> String {
    // `to_string_lossy` turns a name that is not valid UTF-8 into text by
    // replacing the bad bytes with "�", which is fine for showing it. The
    // path itself, used to open the file, stays untouched.
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// The first path that `is_free` accepts among `first_choice` and the
/// numbered names after it in the same folder: `merged.pdf`, then
/// `merged (2).pdf`, `merged (3).pdf`… (the numbering is
/// [`texts::numbered_name`]). `None` when `first_choice` has no file name
/// to number.
///
/// What "free" means is the caller's business, which is why it comes in as
/// a function: a merge must not propose one of the files in its list, and
/// an extraction must not propose a name that already exists.
///
/// `impl Fn(&Path) -> bool` accepts any function or closure that takes a
/// path and answers yes or no. A closure can use what is around it (the
/// list of files, say), which a plain `fn` could not.
pub fn first_free_path(first_choice: &Path, is_free: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    // `?` on an `Option` returns `None` from the whole function when there
    // is no file name. The name becomes text because the number has to be
    // put inside it; a proposal the user can still edit may lose a byte
    // that was not valid text.
    let name = first_choice.file_name()?.to_string_lossy();
    // `1..` counts up without end, and `find` stops at the first number
    // whose path is free. It always stops for the two callers: a list of
    // files and a folder are both finite, so neither can hold every name.
    // `with_file_name` keeps the folder and swaps the name.
    (1..)
        .map(|attempt| first_choice.with_file_name(texts::numbered_name(&name, attempt)))
        .find(|candidate| is_free(candidate))
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
/// returns the name exactly as typed, and a PDF called "report" is hard to
/// open with a double click. A name with another extension ("report.txt")
/// is left as the user typed it: that looks deliberate.
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

/// Why the path that a save dialog returned is not written to.
///
/// Both carry the path as it would have been written, with ".pdf" added if
/// it was, so the status line can name the file the user has to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputRefusal {
    /// `output` is a file that the operation reads: writing there would
    /// replace an input.
    IsInput { output: PathBuf },
    /// ".pdf" had to be added to the name the user typed, and `output` (the
    /// name with ".pdf") already exists.
    ///
    /// The save dialog only asks "replace?" for the name it returns. On Linux
    /// that is the name as typed ("report"), so nobody confirmed replacing
    /// "report.pdf"; writing there would destroy an unrelated file silently.
    ExistsAfterAddingExtension { output: PathBuf },
}

/// Turns the path a save dialog returned into the path to write to, or the
/// reason not to write at all.
///
/// `chosen` gets ".pdf" added when it has no extension (see
/// [`with_pdf_extension`]). It is then refused when `is_input` says the
/// result is a file the operation reads, or when ".pdf" was added and a file
/// with the new name already exists. Nothing is written or changed here.
///
/// `is_input` is a function for the same reason as in [`first_free_path`]:
/// a merge has a list of inputs and an extraction has one, and this rule
/// does not need to know which.
pub fn output_from_dialog(
    chosen: PathBuf,
    is_input: impl Fn(&Path) -> bool,
) -> Result<PathBuf, OutputRefusal> {
    // The extension first, so that typing "a" when "a.pdf" is an input is
    // caught by the check below. The typed name is kept (a `clone` of a
    // short path costs next to nothing) to tell whether `with_pdf_extension`
    // changed it, without repeating its rule here.
    let output = with_pdf_extension(chosen.clone());
    if is_input(&output) {
        return Err(OutputRefusal::IsInput { output });
    }
    // The dialog asked "replace?" only for the name it returned. When that
    // name was changed, an existing file under the new name was never
    // confirmed, so refuse instead of replacing it silently. On Windows the
    // dialog adds ".pdf" itself, so this only fires on systems whose dialog
    // returns the name as typed, such as Linux.
    //
    // `name_is_taken` is the core's rule, the one the command-line tool
    // uses for `--force`. Unlike `Path::exists` it also says yes for a link
    // whose target is gone: saving would replace that link too.
    if output != chosen && name_is_taken(&output) {
        return Err(OutputRefusal::ExistsAfterAddingExtension { output });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A path inside a folder called `docs`, built with `join` so the
    /// separator is right on every system.
    fn doc(name: &str) -> PathBuf {
        Path::new("docs").join(name)
    }

    /// An empty folder of its own for one test, named after the test and
    /// the process so that tests running at the same time never share one.
    fn fresh_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "danonpdf-desktop-paths-{name}-{}",
            std::process::id()
        ));
        // Left over from an earlier run that stopped half way; no error if not.
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    // Names

    #[test]
    fn display_name_is_the_file_name() {
        assert_eq!(display_name(&doc("report.pdf")), "report.pdf");
    }

    #[test]
    fn display_name_without_a_file_name_is_the_whole_path() {
        assert_eq!(display_name(Path::new("..")), "..");
    }

    // Proposing a name

    #[test]
    fn the_first_choice_is_proposed_when_it_is_free() {
        let proposed = first_free_path(&doc("merged.pdf"), |_| true);

        assert_eq!(proposed, Some(doc("merged.pdf")));
    }

    #[test]
    fn taken_names_are_skipped_in_order_and_the_folder_is_kept() {
        let taken = [doc("merged.pdf"), doc("merged (2).pdf")];

        let proposed = first_free_path(&doc("merged.pdf"), |path| {
            // `any` asks: is there a taken name equal to this path?
            !taken.iter().any(|name| name == path)
        });

        assert_eq!(proposed, Some(doc("merged (3).pdf")));
    }

    #[test]
    fn a_path_without_a_file_name_has_nothing_to_propose() {
        assert_eq!(first_free_path(Path::new(".."), |_| true), None);
    }

    #[test]
    fn save_suggestion_splits_folder_and_name() {
        assert_eq!(
            SaveSuggestion::for_path(&doc("merged.pdf")),
            SaveSuggestion {
                folder: Some(PathBuf::from("docs")),
                file_name: Some("merged.pdf".to_owned()),
            }
        );
    }

    #[test]
    fn save_suggestion_for_a_bare_name_has_no_folder() {
        // An empty folder ("") is not a place to open a dialog in.
        assert_eq!(
            SaveSuggestion::for_path(Path::new("merged.pdf")),
            SaveSuggestion {
                folder: None,
                file_name: Some("merged.pdf".to_owned()),
            }
        );
    }

    #[test]
    fn save_suggestion_for_a_path_without_a_name_has_no_name() {
        assert_eq!(
            SaveSuggestion::for_path(Path::new("..")),
            SaveSuggestion {
                folder: None,
                file_name: None,
            }
        );
    }

    // The path a dialog gave back

    #[test]
    fn with_pdf_extension_adds_pdf_only_when_there_is_no_extension() {
        assert_eq!(with_pdf_extension(doc("report")), doc("report.pdf"));
        assert_eq!(with_pdf_extension(doc("report.pdf")), doc("report.pdf"));
        assert_eq!(with_pdf_extension(doc("report.PDF")), doc("report.PDF"));
        // Another extension looks deliberate, so it stays as typed.
        assert_eq!(with_pdf_extension(doc("report.txt")), doc("report.txt"));
    }

    #[test]
    fn with_pdf_extension_follows_the_std_rule_for_odd_names() {
        // For std, everything after the last dot is the extension: "2 notes"
        // here. So this name counts as having one and stays as typed.
        assert_eq!(with_pdf_extension(doc("v1.2 notes")), doc("v1.2 notes"));
        // A leading dot starts a hidden name, not an extension.
        assert_eq!(with_pdf_extension(doc(".report")), doc(".report.pdf"));
        // A trailing dot gives an empty extension, which is still "some".
        assert_eq!(with_pdf_extension(doc("report.")), doc("report."));
    }

    #[test]
    fn a_free_name_that_is_no_input_is_accepted_with_pdf_added() {
        // Nothing here is on disk, so no name is taken.
        assert_eq!(
            output_from_dialog(doc("out"), |_| false),
            Ok(doc("out.pdf"))
        );
        assert_eq!(
            output_from_dialog(doc("out.pdf"), |_| false),
            Ok(doc("out.pdf"))
        );
    }

    #[test]
    fn an_input_is_refused_under_the_name_it_would_be_written_as() {
        // The question is asked about "a.pdf", not about "a" as typed.
        let is_input = |path: &Path| path == doc("a.pdf");

        assert_eq!(
            output_from_dialog(doc("a"), is_input),
            Err(OutputRefusal::IsInput {
                output: doc("a.pdf")
            })
        );
    }

    #[test]
    fn an_existing_file_is_refused_only_when_pdf_had_to_be_added() {
        let folder = fresh_folder("added-extension");
        let existing = folder.join("report.pdf");
        fs::write(&existing, b"someone else's report").unwrap();

        let typed_without = output_from_dialog(folder.join("report"), |_| false);
        // The dialog asked "replace report.pdf?" for this exact name.
        let chosen_whole = output_from_dialog(existing.clone(), |_| false);
        fs::remove_dir_all(&folder).unwrap();

        assert_eq!(
            typed_without,
            Err(OutputRefusal::ExistsAfterAddingExtension {
                output: existing.clone()
            })
        );
        assert_eq!(chosen_whole, Ok(existing));
    }

    #[test]
    fn being_an_input_is_reported_before_already_existing() {
        // An input always exists, so with the other order the same mistake
        // would be told as "it exists" on Linux and "it is an input" on
        // Windows.
        let folder = fresh_folder("input-first");
        let input = folder.join("a.pdf");
        fs::write(&input, b"x").unwrap();

        let result = output_from_dialog(folder.join("a"), |path| path == input);
        fs::remove_dir_all(&folder).unwrap();

        assert_eq!(result, Err(OutputRefusal::IsInput { output: input }));
    }
}
