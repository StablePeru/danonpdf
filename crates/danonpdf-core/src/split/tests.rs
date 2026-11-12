//! Tests for counting, extracting and splitting pages. Every PDF they use
//! is built by the helpers in `test_support.rs`.

use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lopdf::{Document, Object, ObjectId, dictionary};

use super::*;
use crate::error_chain;
use crate::test_support::{
    ScratchDir, a4, count_of_type, encrypted, has_type, markers, root_pages_id,
    with_inherited_attributes, with_pages,
};

/// The page selection written as `text`.
fn selection(text: &str) -> PageSelection {
    text.parse().unwrap()
}

/// `count` as the "every N pages" of a split.
fn every(count: usize) -> NonZeroUsize {
    NonZeroUsize::new(count).expect("a split takes at least one page per file")
}

/// The marker text of every page of the PDF file at `path`, in page order.
fn markers_in(path: &Path) -> Vec<String> {
    markers(&Document::load(path).unwrap())
}

/// A three-page document ("A1", "A2", "A3") whose first page carries a link
/// that jumps to the third page.
fn with_a_link_from_page_1_to_page_3() -> Document {
    let mut document = with_pages("A", 3);
    let pages: Vec<ObjectId> = document.page_iter().collect();
    let link_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => a4(),
        "Dest" => vec![Object::Reference(pages[2]), "Fit".into()],
    });
    document
        .get_dictionary_mut(pages[0])
        .unwrap()
        .set("Annots", vec![Object::Reference(link_id)]);
    document
}

/// The object that the link on the first page of `document` jumps to, as
/// the link names it.
fn link_target(document: &Document) -> ObjectId {
    let first_page = document.page_iter().next().unwrap();
    let annotations = document.get_page_annotations(first_page).unwrap();
    annotations[0]
        .get(b"Dest")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap()
}

// Counting pages

#[test]
fn page_count_is_the_number_of_pages() {
    let scratch = ScratchDir::new("count");
    let input = scratch.save("a.pdf", with_pages("A", 3));

    assert_eq!(page_count(&input).unwrap(), 3);
}

#[test]
fn page_count_refuses_what_a_merge_refuses() {
    let scratch = ScratchDir::new("count-refused");
    let missing = scratch.file("missing.pdf");
    let locked = scratch.save("locked.pdf", encrypted(with_pages("A", 1), "secret"));

    match page_count(&missing) {
        Err(PdfError::Read { path, .. }) => assert_eq!(path, missing),
        other => panic!("expected a read error, got {other:?}"),
    }
    let result = page_count(&locked);
    assert!(
        matches!(result, Err(PdfError::Encrypted { input: 1 })),
        "got {result:?}"
    );
}

// Extracting pages

#[test]
fn extract_writes_the_chosen_pages_in_the_order_given() {
    let scratch = ScratchDir::new("extract-order");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    let pages = extract_file(&input, &selection("3,1"), &output).unwrap();

    assert_eq!(pages, 2);
    assert_eq!(markers_in(&output), ["A3", "A1"]);
    // The input is only read.
    assert_eq!(markers_in(&input), ["A1", "A2", "A3", "A4", "A5"]);
}

#[test]
fn extract_understands_ranges() {
    let extracted = extract_document(with_pages("A", 5), &selection("2-3,5-")).unwrap();

    assert_eq!(markers(&extracted), ["A2", "A3", "A5"]);
}

#[test]
fn extracted_file_has_one_consistent_page_tree() {
    let scratch = ScratchDir::new("extract-tree");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    extract_file(&input, &selection("4,2"), &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert_eq!(count_of_type(&reloaded, b"Catalog"), 1);
    assert_eq!(count_of_type(&reloaded, b"Pages"), 1);
    assert_eq!(count_of_type(&reloaded, b"Page"), 2);
    let root_id = root_pages_id(&reloaded);
    let root = reloaded.get_dictionary(root_id).unwrap();
    assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 2);
    for page_id in reloaded.page_iter() {
        let page = reloaded.get_dictionary(page_id).unwrap();
        assert_eq!(
            page.get(b"Parent").unwrap().as_reference().unwrap(),
            root_id
        );
    }
}

#[test]
fn max_id_of_an_extracted_document_covers_every_object() {
    let extracted = extract_document(with_pages("A", 5), &selection("2")).unwrap();

    let highest = extracted.objects.keys().map(|id| id.0).max().unwrap();
    assert_eq!(extracted.max_id, highest);
}

#[test]
fn extracted_pages_keep_inherited_attributes() {
    let extracted = extract_document(with_inherited_attributes(), &selection("1")).unwrap();

    let page_id = extracted.page_iter().next().unwrap();
    let page = extracted.get_dictionary(page_id).unwrap();
    assert_eq!(
        page.get(b"MediaBox").unwrap(),
        &Object::from(vec![0.into(), 0.into(), 300.into(), 500.into()])
    );
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    let font = fonts
        .get_deref(b"F1", &extracted)
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        font.get(b"BaseFont").unwrap().as_name().unwrap(),
        b"Courier"
    );
}

#[test]
fn extract_leaves_behind_what_only_other_pages_use() {
    let extracted = extract_document(with_pages("A", 5), &selection("2")).unwrap();

    // The page, its content, the resources and the font they name, and the
    // new page tree and catalog. The content of the four other pages, the
    // old page tree and the old catalog are gone.
    assert_eq!(extracted.objects.len(), 6);
    assert_eq!(count_of_type(&extracted, b"Page"), 1);
}

#[test]
fn extract_does_not_follow_a_link_to_a_page_left_out() {
    let scratch = ScratchDir::new("extract-link-out");
    let input = scratch.save("a.pdf", with_a_link_from_page_1_to_page_3());
    let output = scratch.file("out.pdf");

    extract_file(&input, &selection("1"), &output).unwrap();

    // Page 3 was not asked for. If the link dragged it along, its content
    // would sit in the file although no reader shows it: the opposite of
    // what someone who extracts one page to share it expects.
    let reloaded = Document::load(&output).unwrap();
    assert_eq!(markers(&reloaded), ["A1"]);
    assert_eq!(count_of_type(&reloaded, b"Page"), 1);
    // The link is still on the page, and now points at nothing.
    assert!(!reloaded.has_object(link_target(&reloaded)));
}

#[test]
fn a_link_between_two_extracted_pages_still_works() {
    let extracted =
        extract_document(with_a_link_from_page_1_to_page_3(), &selection("1,3")).unwrap();

    let last_page = extracted.page_iter().last().unwrap();
    assert_eq!(markers(&extracted), ["A1", "A3"]);
    assert_eq!(link_target(&extracted), last_page);
}

#[test]
fn extract_keeps_the_pdf_version_of_the_input() {
    let mut input = with_pages("A", 2);
    input.version = "1.7".to_owned();

    let extracted = extract_document(input, &selection("1")).unwrap();

    assert_eq!(extracted.version, "1.7");
}

#[test]
fn extract_refuses_a_page_past_the_end_and_writes_nothing() {
    let scratch = ScratchDir::new("extract-past-end");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    let result = extract_file(&input, &selection("2,9"), &output);

    assert!(
        matches!(result, Err(PdfError::PageOutOfRange { page: 9, pages: 5 })),
        "got {result:?}"
    );
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn extract_refuses_unreadable_and_encrypted_inputs() {
    let scratch = ScratchDir::new("extract-refused");
    let not_a_pdf = scratch.file("notes.pdf");
    fs::write(&not_a_pdf, "just some text, no PDF here").unwrap();
    let locked = scratch.save("locked.pdf", encrypted(with_pages("A", 1), "secret"));
    let output = scratch.file("out.pdf");

    match extract_file(&not_a_pdf, &selection("1"), &output) {
        Err(PdfError::Read { path, .. }) => assert_eq!(path, not_a_pdf),
        other => panic!("expected a read error, got {other:?}"),
    }
    let result = extract_file(&locked, &selection("1"), &output);
    assert!(
        matches!(result, Err(PdfError::Encrypted { input: 1 })),
        "got {result:?}"
    );
    assert!(!output.exists(), "nothing should be written");
}

#[test]
fn extract_from_an_input_with_an_empty_password_is_not_encrypted() {
    let scratch = ScratchDir::new("extract-empty-password");
    let input = scratch.save("a.pdf", encrypted(with_pages("A", 2), ""));
    let output = scratch.file("out.pdf");

    extract_file(&input, &selection("2"), &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert!(!reloaded.trailer.has(b"Encrypt"));
    assert_eq!(markers(&reloaded), ["A2"]);
}

#[test]
fn a_damaged_input_is_broken_as_input_1() {
    let mut no_tree = with_pages("A", 1);
    let catalog_id = no_tree
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    no_tree
        .get_dictionary_mut(catalog_id)
        .unwrap()
        .remove(b"Pages");

    let result = extract_document(no_tree, &selection("1"));

    assert!(
        matches!(
            result,
            Err(PdfError::Broken {
                input: 1,
                reason: "it has no page tree"
            })
        ),
        "got {result:?}"
    );
}

#[test]
fn a_page_tree_that_names_a_missing_page_is_broken() {
    // The tree lists three pages, but the object of the second one is gone.
    let mut document = with_pages("A", 3);
    let second = document.page_iter().nth(1).unwrap();
    document.objects.remove(&second);

    // Asking for another page does not help: the document is refused as a
    // whole, before any page is taken, as a merge refuses it.
    let result = extract_document(document, &selection("1"));

    assert!(
        matches!(
            result,
            Err(PdfError::Broken {
                input: 1,
                reason: "a page in its page tree is missing"
            })
        ),
        "got {result:?}"
    );
}

#[test]
fn extract_may_write_over_its_own_input() {
    // The core allows it, as it does for a merge: everything is read before
    // anything is written, and the file is replaced in one step. The front
    // ends are the ones that refuse it, to protect a caller from itself.
    let scratch = ScratchDir::new("extract-over-input");
    let input = scratch.save("a.pdf", with_pages("A", 3));

    extract_file(&input, &selection("2"), &input).unwrap();

    assert_eq!(markers_in(&input), ["A2"]);
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn extract_into_a_missing_folder_names_the_output() {
    let scratch = ScratchDir::new("extract-missing-folder");
    let input = scratch.save("a.pdf", with_pages("A", 1));
    let output = scratch.file("missing").join("out.pdf");

    match extract_file(&input, &selection("1"), &output) {
        Err(PdfError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

// Naming the parts of a split: no file is touched

#[test]
fn parts_are_named_after_the_input_and_numbered_from_1() {
    let names = split_output_paths(Path::new("docs/report.pdf"), Path::new("out"), 3);

    assert_eq!(
        names,
        ["report-001.pdf", "report-002.pdf", "report-003.pdf"]
            .map(|name| Path::new("out").join(name))
    );
}

#[test]
fn part_numbers_grow_wider_from_1000_parts_on() {
    let input = Path::new("report.pdf");
    let folder = Path::new("out");

    let under = split_output_paths(input, folder, 999);
    let over = split_output_paths(input, folder, 1000);

    assert_eq!(under.first().unwrap(), &folder.join("report-001.pdf"));
    assert_eq!(under.last().unwrap(), &folder.join("report-999.pdf"));
    // Every name of one split has the same width, so the files sort by name
    // in page order.
    assert_eq!(over.first().unwrap(), &folder.join("report-0001.pdf"));
    assert_eq!(over.last().unwrap(), &folder.join("report-1000.pdf"));
}

#[test]
fn only_the_last_extension_of_the_input_is_dropped() {
    let folder = Path::new("out");
    let cases = [
        ("report.final.pdf", "report.final-001.pdf"),
        ("SCAN.PDF", "SCAN-001.pdf"),
        ("scan", "scan-001.pdf"),
        ("notes.txt", "notes-001.pdf"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            split_output_paths(Path::new(input), folder, 1),
            [folder.join(expected)],
            "{input}"
        );
    }
}

#[test]
fn no_parts_need_no_names() {
    let names = split_output_paths(Path::new("report.pdf"), Path::new("out"), 0);

    assert_eq!(names, Vec::<PathBuf>::new());
}

// Linux file names are bytes, not text, so they can be invalid UTF-8.
// Windows names are built differently, so this test only exists on
// Unix-like systems.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_is_kept_as_it_is() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    // 0xE9 is "é" in Latin-1, but on its own it is not valid UTF-8.
    let input = Path::new(OsStr::from_bytes(b"caf\xe9.pdf"));

    let names = split_output_paths(input, Path::new("out"), 1);

    let expected = Path::new("out").join(OsStr::from_bytes(b"caf\xe9-001.pdf"));
    assert_eq!(names, [expected]);
}

// Splitting

#[test]
fn split_writes_one_file_per_page() {
    let scratch = ScratchDir::new("split-single");
    let input = scratch.save("a.pdf", with_pages("A", 3));
    let folder = scratch.file("parts");

    let outputs = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap();

    assert_eq!(
        outputs,
        ["a-001.pdf", "a-002.pdf", "a-003.pdf"].map(|name| folder.join(name))
    );
    let pages: Vec<Vec<String>> = outputs.iter().map(|path| markers_in(path)).collect();
    assert_eq!(pages, [["A1"], ["A2"], ["A3"]]);
    // The input is only read.
    assert_eq!(markers_in(&input), ["A1", "A2", "A3"]);
}

#[test]
fn split_every_n_pages_leaves_a_shorter_last_part() {
    let scratch = ScratchDir::new("split-every-2");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let folder = scratch.file("parts");

    let outputs = plan_split(&input, every(2), &folder)
        .unwrap()
        .write()
        .unwrap();

    assert_eq!(outputs.len(), 3);
    assert_eq!(markers_in(&outputs[0]), ["A1", "A2"]);
    assert_eq!(markers_in(&outputs[1]), ["A3", "A4"]);
    assert_eq!(markers_in(&outputs[2]), ["A5"]);
}

#[test]
fn split_every_more_pages_than_there_are_gives_one_part() {
    let scratch = ScratchDir::new("split-every-10");
    let input = scratch.save("a.pdf", with_pages("A", 3));
    let folder = scratch.file("parts");

    let outputs = plan_split(&input, every(10), &folder)
        .unwrap()
        .write()
        .unwrap();

    assert_eq!(outputs, [folder.join("a-001.pdf")]);
    assert_eq!(markers_in(&outputs[0]), ["A1", "A2", "A3"]);
}

#[test]
fn a_plan_names_its_files_before_anything_is_written() {
    let scratch = ScratchDir::new("split-plan");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let folder = scratch.file("parts");

    let plan = plan_split(&input, every(2), &folder).unwrap();

    assert_eq!(plan.pages(), 5);
    assert_eq!(
        plan.outputs(),
        ["a-001.pdf", "a-002.pdf", "a-003.pdf"].map(|name| folder.join(name))
    );
    // Not even the folder exists yet: a caller can still say no.
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn split_creates_the_folder_and_what_is_missing_above_it() {
    let scratch = ScratchDir::new("split-new-folder");
    let input = scratch.save("a.pdf", with_pages("A", 2));
    let folder = scratch.file("new").join("deeper");

    let outputs = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap();

    assert!(outputs.iter().all(|path| path.is_file()));
}

#[test]
fn split_leaves_no_temporary_file_behind() {
    let scratch = ScratchDir::new("split-no-leftovers");
    let input = scratch.save("a.pdf", with_pages("A", 2));

    // Into the folder of the input itself: the names cannot collide.
    plan_split(&input, every(1), scratch.path())
        .unwrap()
        .write()
        .unwrap();

    assert_eq!(scratch.entries(), ["a-001.pdf", "a-002.pdf", "a.pdf"]);
}

#[test]
fn each_part_holds_only_what_its_own_pages_use() {
    let scratch = ScratchDir::new("split-lean-parts");
    let input = scratch.save("a.pdf", with_pages("A", 4));
    let folder = scratch.file("parts");

    let outputs = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap();

    for output in &outputs {
        let part = Document::load(output).unwrap();
        assert_eq!(count_of_type(&part, b"Catalog"), 1);
        assert_eq!(count_of_type(&part, b"Pages"), 1);
        assert_eq!(count_of_type(&part, b"Page"), 1);
        // One content stream: the page's own. The font is shared by every
        // page of the input, so each part gets its own copy of it.
        // (Reading a file back may add its cross-reference stream, which is
        // part of the file's own bookkeeping, not of any page.)
        let streams = part
            .objects
            .values()
            .filter(|object| matches!(object, Object::Stream(_)) && !has_type(object, b"XRef"))
            .count();
        assert_eq!(streams, 1, "{}", output.display());
        assert_eq!(count_of_type(&part, b"Font"), 1);
    }
}

#[test]
fn parts_keep_inherited_attributes() {
    let scratch = ScratchDir::new("split-inherited");
    let input = scratch.save("a.pdf", with_inherited_attributes());
    let folder = scratch.file("parts");

    let outputs = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap();

    let part = Document::load(&outputs[0]).unwrap();
    let page_id = part.page_iter().next().unwrap();
    let page = part.get_dictionary(page_id).unwrap();
    assert_eq!(
        page.get(b"MediaBox").unwrap(),
        &Object::from(vec![0.into(), 0.into(), 300.into(), 500.into()])
    );
    assert!(page.has(b"Resources"));
}

#[test]
fn split_refuses_unreadable_and_encrypted_inputs_before_planning() {
    let scratch = ScratchDir::new("split-refused");
    let missing = scratch.file("missing.pdf");
    let locked = scratch.save("locked.pdf", encrypted(with_pages("A", 2), "secret"));
    let folder = scratch.file("parts");

    // `SplitPlan` has no `Debug`, so the results are narrowed to their
    // errors before they are printed.
    match plan_split(&missing, every(1), &folder).err() {
        Some(PdfError::Read { path, .. }) => assert_eq!(path, missing),
        other => panic!("expected a read error, got {other:?}"),
    }
    let error = plan_split(&locked, every(1), &folder).err();
    assert!(
        matches!(error, Some(PdfError::Encrypted { input: 1 })),
        "got {error:?}"
    );
    assert!(!folder.exists(), "nothing should be written");
}

#[test]
fn a_split_that_stops_midway_says_where_and_keeps_what_it_wrote() {
    let scratch = ScratchDir::new("split-midway");
    let input = scratch.save("a.pdf", with_pages("A", 3));
    let folder = scratch.file("parts");
    // A folder already holds the name of the second part, so that part
    // cannot be put in place. The first one can.
    fs::create_dir_all(folder.join("a-002.pdf")).unwrap();

    let error = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap_err();

    match &error.error {
        PdfError::Save { path, .. } => assert_eq!(path, &folder.join("a-002.pdf")),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(error.written, [folder.join("a-001.pdf")]);
    assert_eq!(error.parts, 3);
    // The part written before the failure is a complete file and stays.
    assert_eq!(markers_in(&folder.join("a-001.pdf")), ["A1"]);
    // It stopped there: the third part was not tried, and the temporary
    // file of the second one was removed.
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["a-001.pdf", "a-002.pdf"]);

    let message = error_chain(&error);
    let start = format!(
        "the split stopped after 1 of 3 files: cannot save {}: ",
        folder.join("a-002.pdf").display()
    );
    assert!(message.starts_with(&start), "unexpected message: {message}");
}

#[test]
fn a_folder_that_cannot_be_created_stops_the_split_before_any_file() {
    let scratch = ScratchDir::new("split-folder-is-file");
    let input = scratch.save("a.pdf", with_pages("A", 2));
    // A file holds the name the folder should have.
    let folder = scratch.file("parts");
    fs::write(&folder, b"not a folder").unwrap();

    let error = plan_split(&input, every(1), &folder)
        .unwrap()
        .write()
        .unwrap_err();

    match &error.error {
        PdfError::CreateFolder { path, .. } => assert_eq!(path, &folder),
        other => panic!("expected a folder error, got {other:?}"),
    }
    assert_eq!(error.written, Vec::<PathBuf>::new());
    assert_eq!(fs::read(&folder).unwrap(), b"not a folder");
}

/// Not a check but a measurement, so it does not run with the others:
///
/// ```text
/// cargo test --release -p danonpdf-core -- --ignored --nocapture splitting_300
/// ```
///
/// The pages are tiny, so this times the work per file (find what the page
/// uses, copy it, write it, wait for the disk, rename) and not the copying
/// of big images. Nearly all of that time is the wait for the disk: see
/// "Cost" on `SplitPlan::write`.
#[test]
#[ignore = "a timing, not a check: run it by name, in release mode"]
fn splitting_300_pages_into_300_files_is_timed() {
    let scratch = ScratchDir::new("split-timing");
    let input = scratch.save("big.pdf", with_pages("P", 300));
    let folder = scratch.file("parts");

    let start = Instant::now();
    let plan = plan_split(&input, every(1), &folder).unwrap();
    let planned = start.elapsed();
    let outputs = plan.write().unwrap();
    let total = start.elapsed();

    assert_eq!(outputs.len(), 300);
    assert_eq!(markers_in(&outputs[299]), ["P300"]);
    println!("split 300 pages into 300 files in {total:.1?} (reading and planning: {planned:.1?})");
}
