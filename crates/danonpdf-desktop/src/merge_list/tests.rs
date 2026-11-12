//! Tests for the merge window's state. None of them opens a window: that is
//! the point of keeping the state apart from the drawing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lopdf::{Document, Object, dictionary};

use super::*;

/// A path inside a folder called `docs`, built with `join` so the separator
/// is right on every system.
fn doc(name: &str) -> PathBuf {
    Path::new("docs").join(name)
}

/// A list holding `names`, all inside `docs`.
fn list_of(names: &[&str]) -> MergeList {
    let mut list = MergeList::default();
    list.add(names.iter().map(|name| doc(name)));
    list
}

/// The file names in the list, in order, to compare against an array.
fn names(list: &MergeList) -> Vec<String> {
    list.files().iter().map(|path| display_name(path)).collect()
}

// Adding

#[test]
fn new_list_is_empty_and_idle() {
    let list = MergeList::default();
    assert!(list.files().is_empty());
    assert_eq!(list.status(), &Status::Idle);
}

#[test]
fn add_appends_pdfs_in_the_given_order() {
    let list = list_of(&["b.pdf", "a.pdf", "c.PDF"]);
    assert_eq!(names(&list), ["b.pdf", "a.pdf", "c.PDF"]);
    assert_eq!(list.status(), &Status::Idle);
}

#[test]
fn add_appends_after_the_files_already_there() {
    let mut list = list_of(&["a.pdf"]);
    list.add([doc("b.pdf")]);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
}

#[test]
fn add_leaves_out_files_that_are_not_pdf_and_says_how_many() {
    let list = list_of(&["a.pdf", "photo.jpg", "notes.txt", "b.pdf"]);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
    assert_eq!(
        list.status(),
        &Status::Skipped {
            not_pdf: 2,
            duplicates: 0
        }
    );
}

#[test]
fn add_leaves_out_files_already_in_the_list() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.add([doc("b.pdf"), doc("c.pdf")]);
    assert_eq!(names(&list), ["a.pdf", "b.pdf", "c.pdf"]);
    assert_eq!(
        list.status(),
        &Status::Skipped {
            not_pdf: 0,
            duplicates: 1
        }
    );
}

#[test]
fn add_leaves_out_repeats_within_the_same_batch() {
    let list = list_of(&["a.pdf", "a.pdf"]);
    assert_eq!(names(&list), ["a.pdf"]);
    assert_eq!(
        list.status(),
        &Status::Skipped {
            not_pdf: 0,
            duplicates: 1
        }
    );
}

#[test]
fn same_name_in_another_folder_is_not_a_duplicate() {
    let mut list = list_of(&["a.pdf"]);
    list.add([Path::new("other").join("a.pdf")]);
    assert_eq!(list.files().len(), 2);
}

#[test]
fn a_clean_add_clears_an_old_skipped_notice() {
    let mut list = list_of(&["a.pdf", "photo.jpg"]);
    list.add([doc("b.pdf")]);
    assert_eq!(list.status(), &Status::Idle);
}

// Moving and removing

#[test]
fn move_up_swaps_with_the_file_above() {
    let mut list = list_of(&["a.pdf", "b.pdf", "c.pdf"]);
    list.move_up(2);
    assert_eq!(names(&list), ["a.pdf", "c.pdf", "b.pdf"]);
}

#[test]
fn move_up_on_the_first_file_does_nothing() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.move_up(0);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
}

#[test]
fn move_up_past_the_end_does_nothing() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.move_up(5);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
}

#[test]
fn move_down_swaps_with_the_file_below() {
    let mut list = list_of(&["a.pdf", "b.pdf", "c.pdf"]);
    list.move_down(0);
    assert_eq!(names(&list), ["b.pdf", "a.pdf", "c.pdf"]);
}

#[test]
fn move_down_on_the_last_file_does_nothing() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.move_down(1);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
}

#[test]
fn move_down_past_the_end_does_nothing() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.move_down(7);
    assert_eq!(names(&list), ["a.pdf", "b.pdf"]);
}

#[test]
fn moves_on_an_empty_list_do_nothing() {
    let mut list = MergeList::default();
    list.move_up(0);
    list.move_down(0);
    list.remove(0);
    assert!(list.files().is_empty());
}

#[test]
fn remove_takes_the_file_out_and_keeps_the_order() {
    let mut list = list_of(&["a.pdf", "b.pdf", "c.pdf"]);
    list.remove(1);
    assert_eq!(names(&list), ["a.pdf", "c.pdf"]);
}

#[test]
fn remove_past_the_end_does_nothing() {
    let mut list = list_of(&["a.pdf"]);
    list.remove(1);
    assert_eq!(names(&list), ["a.pdf"]);
}

#[test]
fn clear_empties_the_list_and_the_status() {
    let mut list = list_of(&["a.pdf", "photo.jpg"]);
    list.clear();
    assert!(list.files().is_empty());
    assert_eq!(list.status(), &Status::Idle);
}

#[test]
fn changing_the_list_clears_an_old_merge_result() {
    // A "merged 3 pages" line would describe a list that no longer exists.
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.record_merge(doc("merged.pdf"), Ok(3));
    list.move_down(0);
    assert_eq!(list.status(), &Status::Idle);
}

#[test]
fn apply_runs_each_row_action() {
    let mut list = list_of(&["a.pdf", "b.pdf", "c.pdf"]);
    list.apply(RowAction::MoveDown(0));
    assert_eq!(names(&list), ["b.pdf", "a.pdf", "c.pdf"]);
    list.apply(RowAction::MoveUp(2));
    assert_eq!(names(&list), ["b.pdf", "c.pdf", "a.pdf"]);
    list.apply(RowAction::Remove(0));
    assert_eq!(names(&list), ["c.pdf", "a.pdf"]);
}

// Ready to merge, and where to

#[test]
fn can_merge_needs_at_least_two_files() {
    assert!(!MergeList::default().can_merge());
    assert!(!list_of(&["a.pdf"]).can_merge());
    assert!(list_of(&["a.pdf", "b.pdf"]).can_merge());
}

#[test]
fn empty_list_has_no_default_output() {
    assert_eq!(MergeList::default().default_output_path(), None);
}

#[test]
fn default_output_is_merged_pdf_next_to_the_first_file() {
    let mut list = list_of(&["a.pdf"]);
    list.add([Path::new("elsewhere").join("b.pdf")]);
    assert_eq!(list.default_output_path(), Some(doc("merged.pdf")));
}

#[test]
fn default_output_never_proposes_a_file_already_in_the_list() {
    // Merging a+b into merged.pdf and then merging merged.pdf with c must
    // not propose merged.pdf again: that would replace one of the inputs.
    let list = list_of(&["merged.pdf", "c.pdf"]);
    assert_eq!(list.default_output_path(), Some(doc("merged (2).pdf")));

    let list = list_of(&["merged.pdf", "merged (2).pdf"]);
    assert_eq!(list.default_output_path(), Some(doc("merged (3).pdf")));
}

#[test]
fn a_merged_pdf_in_another_folder_does_not_change_the_default() {
    let mut list = list_of(&["a.pdf"]);
    list.add([Path::new("elsewhere").join("merged.pdf")]);
    assert_eq!(list.default_output_path(), Some(doc("merged.pdf")));
}

#[test]
fn default_output_for_a_bare_file_name_is_a_bare_name() {
    let mut list = MergeList::default();
    list.add([PathBuf::from("a.pdf")]);
    assert_eq!(
        list.default_output_path(),
        Some(PathBuf::from("merged.pdf"))
    );
}

#[test]
fn save_suggestion_splits_folder_and_name() {
    let list = list_of(&["a.pdf", "b.pdf"]);
    assert_eq!(
        list.save_suggestion(),
        SaveSuggestion {
            folder: Some(PathBuf::from("docs")),
            file_name: Some("merged.pdf".to_owned()),
        }
    );
}

#[test]
fn save_suggestion_for_an_empty_list_is_empty() {
    assert_eq!(
        MergeList::default().save_suggestion(),
        SaveSuggestion::default()
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

// Merge results

#[test]
fn successful_merge_is_recorded_with_pages_and_output() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    list.record_merge(doc("out.pdf"), Ok(12));
    assert_eq!(
        list.status(),
        &Status::Merged {
            pages: 12,
            output: doc("out.pdf")
        }
    );
    // The list stays, so it can be changed and merged again.
    assert_eq!(list.files().len(), 2);
}

#[test]
fn failed_merge_is_recorded_with_its_whole_chain_of_causes() {
    let mut list = list_of(&["a.pdf", "b.pdf"]);
    let error = MergeError::Save {
        path: PathBuf::from("out.pdf"),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
    };
    list.record_merge(PathBuf::from("out.pdf"), Err(error));
    assert_eq!(
        list.status(),
        &Status::Failed {
            message: "cannot save out.pdf: access denied".to_owned()
        }
    );
}

#[test]
fn merge_into_does_nothing_with_fewer_than_two_files() {
    let mut list = list_of(&["a.pdf"]);
    list.merge_into(doc("out.pdf"));
    assert_eq!(list.status(), &Status::Idle);
}

#[test]
fn merge_into_reports_a_file_that_cannot_be_read() {
    let folder = std::env::temp_dir().join("danonpdf-desktop-missing-inputs");
    let mut list = MergeList::default();
    list.add([folder.join("a.pdf"), folder.join("b.pdf")]);
    list.merge_into(folder.join("out.pdf"));

    // `matches!` checks the shape of a value against a pattern, with an
    // optional `if` condition, and returns a bool.
    assert!(
        matches!(list.status(), Status::Failed { message } if message.starts_with("cannot read")),
        "unexpected status: {:?}",
        list.status()
    );
}

/// Saves a one-page PDF at `path`.
fn save_one_page_pdf(path: &Path) {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let pages = dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save(path).unwrap();
}

/// An empty folder of its own for one test.
///
/// Tests run in parallel threads of the same process, so the folder name
/// carries the test's own `name` as well as the process id: two tests never
/// share (or delete) each other's files.
fn fresh_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("danonpdf-desktop-{name}-{}", std::process::id()));
    // Left over from an earlier run that stopped half way; no error if not.
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// A list with two real one-page PDFs, `a.pdf` and `b.pdf`, in `folder`.
fn list_of_two_real_pdfs(folder: &Path) -> MergeList {
    let (first, second) = (folder.join("a.pdf"), folder.join("b.pdf"));
    save_one_page_pdf(&first);
    save_one_page_pdf(&second);
    let mut list = MergeList::default();
    list.add([first, second]);
    list
}

#[test]
fn merge_into_never_overwrites_one_of_the_inputs() {
    let folder = fresh_folder("output-is-input");
    let mut list = list_of_two_real_pdfs(&folder);
    let before = fs::read(folder.join("a.pdf")).unwrap();

    // The same file as the first input, spelled differently.
    list.merge_into(folder.join(".").join("a.pdf"));
    let after = fs::read(folder.join("a.pdf")).unwrap();
    let status = list.status().clone();
    fs::remove_dir_all(&folder).unwrap();

    assert_eq!(before, after, "the input file was changed");
    assert_eq!(
        status,
        Status::OutputIsInput {
            output: folder.join(".").join("a.pdf")
        }
    );
}

#[test]
fn merge_into_refuses_a_name_that_becomes_an_input_once_pdf_is_added() {
    // Typing "b" in the Linux file chooser means "b.pdf", which is an input.
    let folder = fresh_folder("becomes-input");
    let mut list = list_of_two_real_pdfs(&folder);
    list.merge_into(folder.join("b"));
    let status = list.status().clone();
    fs::remove_dir_all(&folder).unwrap();

    assert_eq!(
        status,
        Status::OutputIsInput {
            output: folder.join("b.pdf")
        }
    );
}

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
fn merge_into_does_not_replace_a_file_named_only_by_the_added_extension() {
    // On Linux the dialog asked about "report", which did not exist, so
    // nobody agreed to replace the unrelated "report.pdf".
    let folder = fresh_folder("added-extension-exists");
    let mut list = list_of_two_real_pdfs(&folder);
    let existing = folder.join("report.pdf");
    fs::write(&existing, b"someone else's report").unwrap();

    list.merge_into(folder.join("report"));
    let after = fs::read(&existing).unwrap();
    let bare_written = folder.join("report").exists();
    let status = list.status().clone();
    fs::remove_dir_all(&folder).unwrap();

    assert_eq!(after, b"someone else's report", "report.pdf was replaced");
    assert!(!bare_written, "a file called just `report` was written");
    assert_eq!(
        status,
        Status::ExistsAfterAddingExtension {
            output: folder.join("report.pdf")
        }
    );
}

// Links only exist in this form on Unix-like systems; creating one on
// Windows needs a different function and, often, special rights.
#[cfg(unix)]
#[test]
fn merge_into_does_not_replace_a_link_to_nothing_named_only_by_the_added_extension() {
    // "report.pdf" is a link whose target is gone. Asking whether the file
    // behind it exists says no, yet the name is in use: saving there would
    // replace the link, and nobody was asked.
    let folder = fresh_folder("added-extension-dangling-link");
    let mut list = list_of_two_real_pdfs(&folder);
    let link = folder.join("report.pdf");
    std::os::unix::fs::symlink(folder.join("gone.pdf"), &link).unwrap();

    list.merge_into(folder.join("report"));
    let still_a_link = fs::symlink_metadata(&link).is_ok_and(|entry| entry.is_symlink());
    let status = list.status().clone();
    fs::remove_dir_all(&folder).unwrap();

    assert!(still_a_link, "the link was replaced");
    assert_eq!(status, Status::ExistsAfterAddingExtension { output: link });
}

#[test]
fn merge_into_replaces_an_existing_file_chosen_by_its_full_name() {
    // The dialog asked "replace report.pdf?" for this exact name and the
    // user said yes, so the merge goes ahead.
    let folder = fresh_folder("full-name-exists");
    let mut list = list_of_two_real_pdfs(&folder);
    let existing = folder.join("report.pdf");
    fs::write(&existing, b"old report").unwrap();

    list.merge_into(existing.clone());
    let after = fs::read(&existing).unwrap();
    let status = list.status().clone();
    fs::remove_dir_all(&folder).unwrap();

    assert_ne!(after, b"old report", "report.pdf was not replaced");
    assert_eq!(
        status,
        Status::Merged {
            pages: 2,
            output: existing
        }
    );
}

#[test]
fn merge_into_adds_pdf_to_a_name_without_an_extension() {
    let folder = fresh_folder("no-extension");
    let mut list = list_of_two_real_pdfs(&folder);
    list.merge_into(folder.join("out"));
    let with_pdf = folder.join("out.pdf").exists();
    let without = folder.join("out").exists();
    fs::remove_dir_all(&folder).unwrap();

    assert!(with_pdf, "out.pdf was not written");
    assert!(!without, "a file called just `out` was written");
}

#[test]
fn merge_into_writes_the_file_and_records_the_pages() {
    let folder = std::env::temp_dir().join(format!("danonpdf-desktop-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    let (first, second, output) = (
        folder.join("a.pdf"),
        folder.join("b.pdf"),
        folder.join("out.pdf"),
    );
    save_one_page_pdf(&first);
    save_one_page_pdf(&second);

    let mut list = MergeList::default();
    list.add([first, second]);
    list.merge_into(output.clone());
    let status = list.status().clone();
    let written = output.exists();
    fs::remove_dir_all(&folder).unwrap();

    assert_eq!(status, Status::Merged { pages: 2, output });
    assert!(written);
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
