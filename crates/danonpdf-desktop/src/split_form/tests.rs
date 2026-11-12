//! Tests for the Split view's state. None of them opens a window or a
//! dialog: a test plays the part of the drawing code and calls the form
//! with the paths a dialog or a drop would have given.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use danonpdf_core::SplitError;

use super::*;
use crate::test_support::{ScratchDir, entries_of, page_widths};

/// A form with `name`, a PDF of `page_count` pages saved in `scratch`,
/// already chosen.
fn form_with(scratch: &ScratchDir, name: &str, page_count: i64) -> SplitForm {
    let mut form = SplitForm::default();
    form.choose([scratch.pdf(name, page_count)]);
    form
}

/// The path of the chosen file. Fails the test if there is none.
fn chosen_path(form: &SplitForm) -> &Path {
    &form.file().expect("a file should be chosen").path
}

/// The message the core gives for the page text `text`.
fn syntax_error(text: &str) -> String {
    text.parse::<PageSelection>().unwrap_err().to_string()
}

// Choosing the file

#[test]
fn a_new_form_has_no_file_and_nothing_to_do() {
    let form = SplitForm::default();

    assert_eq!(form.file(), None);
    assert_eq!(form.mode(), Mode::Extract);
    assert_eq!(form.pages_text(), "");
    assert_eq!(form.pages_check(), &PagesCheck::Pending);
    assert_eq!(form.pages_per_file(), 1);
    assert_eq!(form.split_preview(), None);
    assert_eq!(form.status(), &Status::Idle);
    assert!(!form.can_extract());
    assert!(!form.can_split());
}

#[test]
fn choosing_a_pdf_reads_its_page_count() {
    let scratch = ScratchDir::new("choose");
    let path = scratch.pdf("report.pdf", 5);
    let mut form = SplitForm::default();

    form.choose([path.clone()]);

    assert_eq!(form.file(), Some(&ChosenFile { path, pages: 5 }));
    assert_eq!(form.status(), &Status::Idle);
    assert!(form.can_split());
}

#[test]
fn choosing_again_replaces_the_file() {
    let scratch = ScratchDir::new("choose-again");
    let mut form = form_with(&scratch, "first.pdf", 5);
    let second = scratch.pdf("second.pdf", 2);

    form.choose([second.clone()]);

    assert_eq!(
        form.file(),
        Some(&ChosenFile {
            path: second,
            pages: 2
        })
    );
}

#[test]
fn of_several_files_the_first_pdf_is_used_and_the_rest_are_counted() {
    let scratch = ScratchDir::new("choose-several");
    let a = scratch.pdf("a.pdf", 3);
    let b = scratch.pdf("b.pdf", 1);
    let c = scratch.pdf("c.pdf", 1);
    let mut form = SplitForm::default();

    form.choose([scratch.file("notes.txt"), a.clone(), b, c]);

    assert_eq!(chosen_path(&form), a);
    assert_eq!(
        form.status(),
        &Status::LeftOut {
            other_pdfs: 2,
            not_pdf: 1
        }
    );
}

#[test]
fn files_that_are_not_pdfs_change_nothing_and_are_counted() {
    let scratch = ScratchDir::new("choose-no-pdf");
    let mut form = form_with(&scratch, "report.pdf", 5);
    let before = form.file().cloned();

    form.choose([scratch.file("photo.jpg"), scratch.file("notes.txt")]);

    assert_eq!(form.file().cloned(), before);
    assert_eq!(form.status(), &Status::NoPdf { not_pdf: 2 });
}

#[test]
fn choosing_nothing_changes_nothing() {
    let scratch = ScratchDir::new("choose-nothing");
    let mut form = SplitForm::default();
    form.choose([scratch.file("photo.jpg")]);

    // `[PathBuf; 0]` is an array of no paths: the type has to be written
    // out because there is no element to infer it from.
    let nothing: [PathBuf; 0] = [];
    form.choose(nothing);

    assert_eq!(form.file(), None);
    assert_eq!(form.status(), &Status::NoPdf { not_pdf: 1 });
}

#[test]
fn a_file_that_is_not_really_a_pdf_is_not_accepted() {
    let scratch = ScratchDir::new("choose-not-a-pdf");
    let mut form = form_with(&scratch, "report.pdf", 5);
    let good = chosen_path(&form).to_path_buf();
    let fake = scratch.file("notes.pdf");
    fs::write(&fake, "just some text, no PDF here").unwrap();

    form.choose([fake.clone()]);

    // The file chosen before stays: a failed choice is not a choice.
    assert_eq!(chosen_path(&form), good);
    // `matches!` checks the shape of a value against a pattern, with an
    // optional `if` condition, and returns a bool.
    assert!(
        matches!(
            form.status(),
            Status::NotAccepted { file, message }
                if file == &fake && message.starts_with("cannot read")
        ),
        "unexpected status: {:?}",
        form.status()
    );
}

#[test]
fn a_file_that_needs_a_password_or_is_missing_is_not_accepted() {
    let scratch = ScratchDir::new("choose-locked");
    let locked = scratch.locked_pdf("locked.pdf");
    let missing = scratch.file("missing.pdf");
    let mut form = SplitForm::default();

    form.choose([locked.clone()]);
    assert_eq!(form.file(), None);
    assert_eq!(
        form.status(),
        &Status::NotAccepted {
            file: locked,
            // The wording is the core's, so it is asked for and not copied:
            // a change of words there must not fail a test here.
            message: PdfError::Encrypted { input: 1 }.to_string()
        }
    );

    form.choose([missing.clone()]);
    assert_eq!(form.file(), None);
    assert!(
        matches!(form.status(), Status::NotAccepted { file, .. } if file == &missing),
        "unexpected status: {:?}",
        form.status()
    );
}

#[test]
fn when_the_first_pdf_cannot_be_read_the_next_one_is_not_tried() {
    // "The first PDF is used" stays true when it turns out to be bad: the
    // user is told about that file instead of getting another one silently.
    let scratch = ScratchDir::new("choose-first-bad");
    let fake = scratch.file("first.pdf");
    fs::write(&fake, "not a PDF").unwrap();
    let good = scratch.pdf("second.pdf", 2);
    let mut form = SplitForm::default();

    form.choose([fake.clone(), good]);

    assert_eq!(form.file(), None);
    assert!(
        matches!(form.status(), Status::NotAccepted { file, .. } if file == &fake),
        "unexpected status: {:?}",
        form.status()
    );
}

// The pages to extract

#[test]
fn pages_that_the_file_has_are_selected_and_counted() {
    let scratch = ScratchDir::new("pages-valid");
    let mut form = form_with(&scratch, "report.pdf", 5);

    form.set_pages_text("1-3,5".to_owned());

    assert_eq!(form.pages_text(), "1-3,5");
    assert_eq!(
        form.pages_check(),
        &PagesCheck::Selected {
            selection: "1-3,5".parse().unwrap(),
            pages: 4
        }
    );
    assert!(form.can_extract());
}

#[test]
fn an_open_range_runs_to_the_last_page_of_the_chosen_file() {
    let scratch = ScratchDir::new("pages-open");
    let mut form = form_with(&scratch, "report.pdf", 5);

    form.set_pages_text("2-".to_owned());

    assert!(
        matches!(form.pages_check(), PagesCheck::Selected { pages: 4, .. }),
        "unexpected check: {:?}",
        form.pages_check()
    );
}

#[test]
fn a_mistyped_selection_carries_the_message_of_the_core() {
    let scratch = ScratchDir::new("pages-syntax");
    let mut form = form_with(&scratch, "report.pdf", 5);

    // One text for each rule of the syntax.
    for text in ["1,,3", "abc", "0", "5-3", "1-5,3"] {
        form.set_pages_text(text.to_owned());

        assert_eq!(
            form.pages_check(),
            &PagesCheck::Invalid {
                message: syntax_error(text)
            },
            "{text:?}"
        );
        assert!(!form.can_extract(), "{text:?}");
    }
}

#[test]
fn a_page_past_the_end_carries_the_message_of_the_core() {
    let scratch = ScratchDir::new("pages-past-end");
    let mut form = form_with(&scratch, "report.pdf", 5);

    form.set_pages_text("2,9".to_owned());

    assert_eq!(
        form.pages_check(),
        &PagesCheck::Invalid {
            message: PdfError::PageOutOfRange { page: 9, pages: 5 }.to_string()
        }
    );
    assert!(!form.can_extract());
}

#[test]
fn no_text_is_not_a_mistake_yet() {
    let scratch = ScratchDir::new("pages-blank");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("1".to_owned());

    for blank in ["", "   "] {
        form.set_pages_text(blank.to_owned());

        assert_eq!(form.pages_check(), &PagesCheck::Pending, "{blank:?}");
        assert!(!form.can_extract(), "{blank:?}");
    }
}

#[test]
fn text_typed_without_a_file_is_kept_and_checked_once_there_is_one() {
    let scratch = ScratchDir::new("pages-before-file");
    let mut form = SplitForm::default();

    form.set_pages_text("1-2".to_owned());
    // Nothing to check it against, so no verdict either way.
    assert_eq!(form.pages_check(), &PagesCheck::Pending);
    assert!(!form.can_extract());

    form.choose([scratch.pdf("report.pdf", 5)]);
    assert_eq!(form.pages_text(), "1-2");
    assert!(form.can_extract());
}

#[test]
fn choosing_another_file_checks_the_text_against_its_pages() {
    let scratch = ScratchDir::new("pages-recheck");
    let mut form = form_with(&scratch, "long.pdf", 5);
    form.set_pages_text("4-5".to_owned());
    assert!(form.can_extract());

    form.choose([scratch.pdf("short.pdf", 2)]);

    assert_eq!(form.pages_text(), "4-5");
    assert_eq!(
        form.pages_check(),
        &PagesCheck::Invalid {
            message: PdfError::PageOutOfRange { page: 5, pages: 2 }.to_string()
        }
    );
}

#[test]
fn the_extract_button_knows_what_it_is_still_waiting_for() {
    let scratch = ScratchDir::new("extract-needs");
    let mut form = SplitForm::default();

    // Without a file, that is what is missing, whatever has been typed.
    assert_eq!(form.extract_needs(), Some(ExtractNeeds::File));
    form.set_pages_text("1".to_owned());
    assert_eq!(form.extract_needs(), Some(ExtractNeeds::File));
    assert!(!form.can_extract());

    form.choose([scratch.pdf("report.pdf", 5)]);
    assert_eq!(form.extract_needs(), None);
    assert!(form.can_extract());

    // With a file, the pages are what is missing: wrong ones, or none.
    for text in ["9", ""] {
        form.set_pages_text(text.to_owned());
        assert_eq!(form.extract_needs(), Some(ExtractNeeds::Pages), "{text:?}");
        assert!(!form.can_extract(), "{text:?}");
    }
}

// Mode, and what makes an old status stale

#[test]
fn choosing_a_file_replaces_the_result_of_an_earlier_operation() {
    // A line about pages extracted from the old file, or about a failure
    // with it, says nothing true about the file that is chosen now.
    let scratch = ScratchDir::new("choose-clears-status");
    let next = scratch.pdf("next.pdf", 2);
    // Three ways to leave a result behind. A closure that uses nothing
    // around it can stand in for a plain function, which is what lets the
    // three share one type (`fn(&mut SplitForm)`) and sit in one array.
    let results: [fn(&mut SplitForm); 3] = [
        |form| form.record_extract(PathBuf::from("out.pdf"), Ok(2)),
        |form| {
            let written = vec![Path::new("parts").join("report-001.pdf")];
            form.record_split(PathBuf::from("parts"), 5, Ok(written));
        },
        |form| form.record_extract(PathBuf::from("out.pdf"), Err(PdfError::NoInputs)),
    ];

    for leave_a_result in results {
        let mut form = form_with(&scratch, "report.pdf", 5);
        leave_a_result(&mut form);
        assert_ne!(form.status(), &Status::Idle);

        form.choose([next.clone()]);

        assert_eq!(chosen_path(&form), next);
        assert_eq!(form.status(), &Status::Idle);
    }
}

#[test]
fn changing_the_form_clears_an_old_result() {
    // "Extracted 2 pages" would describe a form that no longer exists.
    let scratch = ScratchDir::new("stale-status");
    let mut form = form_with(&scratch, "report.pdf", 5);
    let done = |form: &mut SplitForm| form.record_extract(PathBuf::from("out.pdf"), Ok(2));

    done(&mut form);
    form.set_pages_text("1".to_owned());
    assert_eq!(form.status(), &Status::Idle, "after typing");

    done(&mut form);
    form.set_mode(Mode::Split);
    assert_eq!(form.mode(), Mode::Split);
    assert_eq!(form.status(), &Status::Idle, "after changing mode");

    done(&mut form);
    form.set_pages_per_file(2);
    assert_eq!(
        form.status(),
        &Status::Idle,
        "after changing pages per file"
    );
}

#[test]
fn setting_the_form_to_what_it_already_is_keeps_the_status() {
    // egui reports a click on the radio button that is already selected;
    // that must not wipe the line that says what just happened.
    let scratch = ScratchDir::new("status-kept");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.record_extract(PathBuf::from("out.pdf"), Ok(2));
    let before = form.status().clone();

    form.set_mode(Mode::Extract);
    form.set_pages_per_file(1);
    form.set_pages_text(String::new());

    assert_eq!(form.status(), &before);
}

// Pages per file, and the preview of a split

#[test]
fn the_preview_names_the_first_and_the_last_file() {
    let scratch = ScratchDir::new("preview");
    let mut form = form_with(&scratch, "report.pdf", 5);

    // One page per file to start with.
    assert_eq!(
        form.split_preview(),
        Some(&SplitPreview {
            pages: 5,
            files: 5,
            first: "report-001.pdf".to_owned(),
            last: "report-005.pdf".to_owned(),
        })
    );

    form.set_pages_per_file(2);
    assert_eq!(form.pages_per_file(), 2);
    assert_eq!(
        form.split_preview(),
        Some(&SplitPreview {
            pages: 5,
            files: 3,
            first: "report-001.pdf".to_owned(),
            last: "report-003.pdf".to_owned(),
        })
    );
}

#[test]
fn pages_per_file_stays_between_1_and_the_page_count() {
    let scratch = ScratchDir::new("per-file-range");
    let mut form = form_with(&scratch, "report.pdf", 5);
    assert_eq!(form.pages_per_file_range(), 1..=5);

    form.set_pages_per_file(0);
    assert_eq!(form.pages_per_file(), 1);

    // More pages per file than there are pages means the same as all of
    // them, so the number stops there.
    form.set_pages_per_file(80);
    assert_eq!(form.pages_per_file(), 5);
    assert_eq!(
        form.split_preview(),
        Some(&SplitPreview {
            pages: 5,
            files: 1,
            first: "report-001.pdf".to_owned(),
            last: "report-001.pdf".to_owned(),
        })
    );
}

#[test]
fn without_a_file_pages_per_file_stays_at_1_and_there_is_no_preview() {
    let mut form = SplitForm::default();

    form.set_pages_per_file(4);

    assert_eq!(form.pages_per_file_range(), 1..=1);
    assert_eq!(form.pages_per_file(), 1);
    assert_eq!(form.split_preview(), None);
}

#[test]
fn choosing_a_shorter_file_brings_pages_per_file_down_to_its_length() {
    let scratch = ScratchDir::new("per-file-shorter");
    let mut form = form_with(&scratch, "long.pdf", 5);
    form.set_pages_per_file(4);

    form.choose([scratch.pdf("short.pdf", 2)]);

    assert_eq!(form.pages_per_file(), 2);
    assert_eq!(
        form.split_preview(),
        Some(&SplitPreview {
            pages: 2,
            files: 1,
            first: "short-001.pdf".to_owned(),
            last: "short-001.pdf".to_owned(),
        })
    );
}

// Extracting: where to, and what is refused

#[test]
fn the_proposed_name_is_the_input_name_with_pages_added() {
    let scratch = ScratchDir::new("extract-proposal");
    let form = form_with(&scratch, "report.pdf", 5);

    assert_eq!(
        form.default_extract_path(),
        Some(scratch.file("report-pages.pdf"))
    );
    assert_eq!(
        form.extract_suggestion(),
        SaveSuggestion {
            folder: Some(scratch.path().to_path_buf()),
            file_name: Some("report-pages.pdf".to_owned()),
        }
    );
}

#[test]
fn a_name_that_is_taken_is_never_proposed() {
    let scratch = ScratchDir::new("extract-proposal-taken");
    let form = form_with(&scratch, "report.pdf", 5);
    fs::write(scratch.file("report-pages.pdf"), b"an earlier extraction").unwrap();
    assert_eq!(
        form.default_extract_path(),
        Some(scratch.file("report-pages (2).pdf"))
    );

    // A folder holds a name just as well as a file does.
    fs::create_dir(scratch.file("report-pages (2).pdf")).unwrap();
    assert_eq!(
        form.default_extract_path(),
        Some(scratch.file("report-pages (3).pdf"))
    );
}

#[test]
fn without_a_file_there_is_nothing_to_propose() {
    let form = SplitForm::default();

    assert_eq!(form.default_extract_path(), None);
    assert_eq!(form.extract_suggestion(), SaveSuggestion::default());
    assert_eq!(form.split_folder_suggestion(), None);
}

#[test]
fn extract_into_does_nothing_without_a_valid_selection() {
    let scratch = ScratchDir::new("extract-not-ready");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("9".to_owned());

    form.extract_into(scratch.file("out.pdf"));

    assert_eq!(form.status(), &Status::Idle);
    assert_eq!(scratch.entries(), ["report.pdf"]);
}

#[test]
fn extract_into_writes_the_pages_in_the_order_typed() {
    let scratch = ScratchDir::new("extract");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("3,1".to_owned());
    let output = scratch.file("out.pdf");

    form.extract_into(output.clone());

    assert_eq!(page_widths(&output), [503, 501]);
    assert_eq!(form.status(), &Status::Extracted { pages: 2, output });
    // The input is only read, and the form stays as it was, ready for
    // another extraction.
    assert_eq!(page_widths(chosen_path(&form)).len(), 5);
    assert_eq!(form.pages_text(), "3,1");
}

#[test]
fn extract_into_adds_pdf_to_a_name_without_an_extension() {
    let scratch = ScratchDir::new("extract-no-extension");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("2".to_owned());

    form.extract_into(scratch.file("out"));

    assert_eq!(scratch.entries(), ["out.pdf", "report.pdf"]);
    assert_eq!(
        form.status(),
        &Status::Extracted {
            pages: 1,
            output: scratch.file("out.pdf")
        }
    );
}

#[test]
fn extract_into_never_writes_over_the_input() {
    let scratch = ScratchDir::new("extract-onto-input");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("1".to_owned());
    let before = fs::read(chosen_path(&form)).unwrap();

    // The same file as the input, spelled differently; then a name that
    // only becomes the input once ".pdf" is added.
    let respelled = scratch.path().join(".").join("report.pdf");
    for (typed, refused) in [
        (respelled.clone(), respelled),
        (scratch.file("report"), scratch.file("report.pdf")),
    ] {
        form.extract_into(typed);

        assert_eq!(
            form.status(),
            &Status::OutputIsInput {
                mode: Mode::Extract,
                output: refused
            }
        );
        assert_eq!(fs::read(chosen_path(&form)).unwrap(), before);
    }
    assert_eq!(scratch.entries(), ["report.pdf"]);
}

#[test]
fn extract_into_does_not_replace_a_file_named_only_by_the_added_extension() {
    // On Linux the dialog asked about "out", which did not exist, so
    // nobody agreed to replace the unrelated "out.pdf".
    let scratch = ScratchDir::new("extract-added-extension");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("1".to_owned());
    let existing = scratch.file("out.pdf");
    fs::write(&existing, b"someone else's file").unwrap();

    form.extract_into(scratch.file("out"));

    assert_eq!(fs::read(&existing).unwrap(), b"someone else's file");
    assert_eq!(
        form.status(),
        &Status::ExistsAfterAddingExtension { output: existing }
    );
    assert_eq!(scratch.entries(), ["out.pdf", "report.pdf"]);
}

#[test]
fn extract_into_replaces_an_existing_file_chosen_by_its_full_name() {
    // The dialog asked "replace out.pdf?" for this exact name and the user
    // said yes, so the extraction goes ahead.
    let scratch = ScratchDir::new("extract-replace");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("4".to_owned());
    let existing = scratch.file("out.pdf");
    fs::write(&existing, b"an old file").unwrap();

    form.extract_into(existing.clone());

    assert_eq!(page_widths(&existing), [504]);
    assert_eq!(
        form.status(),
        &Status::Extracted {
            pages: 1,
            output: existing
        }
    );
}

#[test]
fn a_failed_extraction_is_recorded_with_its_whole_chain_of_causes() {
    let mut form = SplitForm::default();
    let error = PdfError::Save {
        path: PathBuf::from("out.pdf"),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
    };

    form.record_extract(PathBuf::from("out.pdf"), Err(error));

    assert_eq!(
        form.status(),
        &Status::Failed {
            mode: Mode::Extract,
            message: "cannot save out.pdf: access denied".to_owned()
        }
    );
}

#[test]
fn a_file_that_went_away_after_it_was_chosen_fails_the_extraction() {
    // The page count was read when the file was chosen. The file is read
    // again to extract, and by then it may be gone or changed.
    let scratch = ScratchDir::new("extract-file-gone");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_text("1".to_owned());
    fs::remove_file(chosen_path(&form)).unwrap();

    form.extract_into(scratch.file("out.pdf"));

    assert!(
        matches!(
            form.status(),
            Status::Failed { mode: Mode::Extract, message } if message.starts_with("cannot read")
        ),
        "unexpected status: {:?}",
        form.status()
    );
    assert_eq!(scratch.entries(), Vec::<String>::new());
}

// Splitting: where to, and what is refused

#[test]
fn the_folder_dialog_starts_where_the_input_is() {
    let scratch = ScratchDir::new("split-folder-proposal");
    let form = form_with(&scratch, "report.pdf", 5);

    assert_eq!(
        form.split_folder_suggestion(),
        Some(scratch.path().to_path_buf())
    );
}

#[test]
fn split_into_does_nothing_without_a_file() {
    let scratch = ScratchDir::new("split-no-file");
    let mut form = SplitForm::default();

    form.split_into(scratch.file("parts"));

    assert_eq!(form.status(), &Status::Idle);
    assert_eq!(scratch.entries(), Vec::<String>::new());
}

#[test]
fn split_into_writes_the_files_and_says_how_many() {
    let scratch = ScratchDir::new("split");
    let mut form = form_with(&scratch, "report.pdf", 5);
    form.set_pages_per_file(2);
    // A folder that does not exist yet: the split creates it.
    let folder = scratch.file("new").join("parts");

    form.split_into(folder.clone());

    assert_eq!(
        entries_of(&folder),
        ["report-001.pdf", "report-002.pdf", "report-003.pdf"]
    );
    assert_eq!(page_widths(&folder.join("report-001.pdf")), [501, 502]);
    assert_eq!(page_widths(&folder.join("report-002.pdf")), [503, 504]);
    assert_eq!(page_widths(&folder.join("report-003.pdf")), [505]);
    assert_eq!(
        form.status(),
        &Status::SplitDone {
            files: 3,
            pages: 5,
            folder
        }
    );
    // The input is only read.
    assert_eq!(page_widths(chosen_path(&form)).len(), 5);
}

#[test]
fn split_into_the_folder_of_the_input_works() {
    // The names of the parts never spell the input, so its own folder is
    // as good a place as any.
    let scratch = ScratchDir::new("split-same-folder");
    let mut form = form_with(&scratch, "report.pdf", 2);

    form.split_into(scratch.path().to_path_buf());

    assert_eq!(
        scratch.entries(),
        ["report-001.pdf", "report-002.pdf", "report.pdf"]
    );
}

#[test]
fn split_into_writes_nothing_when_one_of_its_names_is_taken() {
    let scratch = ScratchDir::new("split-name-taken");
    let mut form = form_with(&scratch, "report.pdf", 3);
    let folder = scratch.file("parts");
    fs::create_dir(&folder).unwrap();
    // The second and the third name are in use; the first one is free.
    fs::write(folder.join("report-002.pdf"), b"someone else's file").unwrap();
    fs::write(folder.join("report-003.pdf"), b"another one").unwrap();

    form.split_into(folder.clone());

    // The first name that is taken is the one reported.
    assert_eq!(
        form.status(),
        &Status::NameTaken {
            output: folder.join("report-002.pdf")
        }
    );
    // Not even the file whose name was free: all or nothing.
    assert_eq!(entries_of(&folder), ["report-002.pdf", "report-003.pdf"]);
    assert_eq!(
        fs::read(folder.join("report-002.pdf")).unwrap(),
        b"someone else's file"
    );
}

// Links only exist in this form on Unix-like systems; creating one on
// Windows needs a different function and, often, special rights.
#[cfg(unix)]
#[test]
fn split_into_never_writes_over_the_input() {
    // The names of a split never spell the input, so the only way for one
    // of them to be the input is a link.
    let scratch = ScratchDir::new("split-onto-input");
    let mut form = form_with(&scratch, "report.pdf", 2);
    let before = fs::read(chosen_path(&form)).unwrap();
    let folder = scratch.file("parts");
    fs::create_dir(&folder).unwrap();
    let link = folder.join("report-002.pdf");
    std::os::unix::fs::symlink(chosen_path(&form), &link).unwrap();

    form.split_into(folder.clone());

    // The link's name is also taken, so this proves the order of the two
    // checks too: "it is the input" comes first, as in the command line.
    assert_eq!(
        form.status(),
        &Status::OutputIsInput {
            mode: Mode::Split,
            output: link
        }
    );
    assert_eq!(fs::read(chosen_path(&form)).unwrap(), before);
    assert_eq!(entries_of(&folder), ["report-002.pdf"]);
}

#[test]
fn a_split_that_stops_midway_says_how_far_it_got() {
    // Every name is checked before the first file is written, so a split
    // that fails halfway is hard to cause with real files. The outcome is
    // fed in directly instead, as the core would report it.
    let mut form = SplitForm::default();
    let folder = PathBuf::from("parts");
    let stopped = SplitError {
        error: PdfError::Save {
            path: folder.join("report-002.pdf"),
            source: io::Error::new(io::ErrorKind::StorageFull, "no space left"),
        },
        written: vec![folder.join("report-001.pdf")],
        parts: 3,
    };
    let reason = format!(
        "cannot save {}: no space left",
        folder.join("report-002.pdf").display()
    );

    form.record_split(folder.clone(), 3, Err(stopped));

    assert_eq!(
        form.status(),
        &Status::SplitStopped {
            written: 1,
            parts: 3,
            folder,
            message: reason
        }
    );
}

#[test]
fn a_file_that_went_away_after_it_was_chosen_fails_the_split_before_any_folder() {
    let scratch = ScratchDir::new("split-file-gone");
    let mut form = form_with(&scratch, "report.pdf", 5);
    fs::remove_file(chosen_path(&form)).unwrap();

    form.split_into(scratch.file("parts"));

    assert!(
        matches!(
            form.status(),
            Status::Failed { mode: Mode::Split, message } if message.starts_with("cannot read")
        ),
        "unexpected status: {:?}",
        form.status()
    );
    assert_eq!(scratch.entries(), Vec::<String>::new());
}
