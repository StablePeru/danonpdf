//! Tests for arranging, rotating and deleting pages. Every one of them goes
//! through the three public functions and reads the written file back, so
//! what is checked is what a caller gets. Every PDF they use is built by
//! the helpers in `test_support.rs`.

use std::fs;
use std::path::Path;

use lopdf::{Document, Object};

use super::*;
use crate::WhichInput;
use crate::test_support::{
    ScratchDir, count_of_type, encrypted, has_type, link_target, markers, root_pages_id, rotations,
    set_rotation, with_a_link_from_page_1_to_page_3, with_inherited_attributes, with_pages,
};

/// The page selection written as `text`.
fn selection(text: &str) -> PageSelection {
    text.parse().unwrap()
}

/// Page `page` of the input, turned by `turn`.
fn turned(page: usize, turn: Turn) -> PageEdit {
    PageEdit { page, turn }
}

/// Page `page` of the input, as it is.
fn kept(page: usize) -> PageEdit {
    turned(page, Turn::Zero)
}

/// The marker text of every page of the PDF file at `path`, in page order.
fn markers_in(path: &Path) -> Vec<String> {
    markers(&Document::load(path).unwrap())
}

/// The `/Rotate` each page of the PDF file at `path` holds itself, in page
/// order: `None` for a page without the key.
fn rotations_in(path: &Path) -> Vec<Option<i64>> {
    rotations(&Document::load(path).unwrap())
}

/// A document of `page_count` pages ("A1", "A2"...) where no page has a
/// `/Rotate` of its own: they all inherit `degrees` from the root of the
/// page tree.
fn with_rotation_on_the_page_tree(page_count: i64, degrees: i64) -> Document {
    let mut document = with_pages("A", page_count);
    let root_id = root_pages_id(&document);
    document
        .get_dictionary_mut(root_id)
        .unwrap()
        .set("Rotate", degrees);
    document
}

/// Runs `operation`, which must fail, in the three places where a failure
/// could do harm, and checks each time that nothing was written. Returns
/// the error of the first run, for the test to look at.
///
/// `operation` gets the path of a five-page input ("A1" to "A5") and the
/// path to write to. It is a closure, so one helper serves the three
/// functions and whatever arguments each test wants to give them. `Fn`
/// (and not `FnOnce`) because it is called more than once.
fn refused(
    test_name: &str,
    operation: impl Fn(&Path, &Path) -> Result<usize, PdfError>,
) -> PdfError {
    let scratch = ScratchDir::new(test_name);
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let input_before = fs::read(&input).unwrap();

    // 1. A name that is free: no file appears under it, nor a temporary
    //    one next to it.
    let error = operation(&input, &scratch.file("new.pdf")).unwrap_err();
    assert_eq!(scratch.entries(), ["a.pdf"]);

    // 2. A file that is already there keeps its bytes.
    let existing = scratch.file("existing.pdf");
    fs::write(&existing, b"what was here before").unwrap();
    operation(&input, &existing).unwrap_err();
    assert_eq!(fs::read(&existing).unwrap(), b"what was here before");

    // 3. The input itself, given as the output.
    operation(&input, &input).unwrap_err();

    assert_eq!(scratch.entries(), ["a.pdf", "existing.pdf"]);
    assert_eq!(fs::read(&input).unwrap(), input_before);
    error
}

// Arranging: the order of the pages

#[test]
fn arrange_writes_the_pages_in_the_order_given() {
    let scratch = ScratchDir::new("arrange-order");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let input_before = fs::read(&input).unwrap();
    let output = scratch.file("out.pdf");
    let edits = [kept(3), kept(1), kept(2), kept(5), kept(4)];

    let pages = arrange_file(&input, &edits, &output).unwrap();

    assert_eq!(pages, 5);
    assert_eq!(markers_in(&output), ["A3", "A1", "A2", "A5", "A4"]);
    // Moving a page does not turn it.
    assert_eq!(rotations_in(&output), [None; 5]);
    // The input is only read.
    assert_eq!(fs::read(&input).unwrap(), input_before);
    // And no temporary file is left next to the new one.
    assert_eq!(scratch.entries(), ["a.pdf", "out.pdf"]);
}

#[test]
fn a_page_that_is_not_listed_is_deleted() {
    let scratch = ScratchDir::new("arrange-delete");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    let pages = arrange_file(&input, &[kept(1), kept(3), kept(5)], &output).unwrap();

    assert_eq!(pages, 3);
    assert_eq!(markers_in(&output), ["A1", "A3", "A5"]);
    assert_eq!(rotations_in(&output), [None; 3]);
}

#[test]
fn arrange_moves_turns_and_deletes_in_one_go() {
    let scratch = ScratchDir::new("arrange-mixed");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");
    // Page 4 comes first and lies on its side, page 1 follows untouched,
    // page 2 ends upside down; pages 3 and 5 are gone.
    let edits = [turned(4, Turn::Quarter), kept(1), turned(2, Turn::Half)];

    let pages = arrange_file(&input, &edits, &output).unwrap();

    assert_eq!(pages, 3);
    assert_eq!(markers_in(&output), ["A4", "A1", "A2"]);
    assert_eq!(rotations_in(&output), [Some(90), None, Some(180)]);
}

#[test]
fn arranged_file_has_one_consistent_page_tree() {
    let scratch = ScratchDir::new("arrange-tree");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[turned(4, Turn::Quarter), kept(2)], &output).unwrap();

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

// Arranging: the turns

#[test]
fn each_turn_is_written_as_its_degrees() {
    let scratch = ScratchDir::new("arrange-turns");
    let input = scratch.save("a.pdf", with_pages("A", 4));
    let output = scratch.file("out.pdf");
    let edits = [
        turned(1, Turn::Zero),
        turned(2, Turn::Quarter),
        turned(3, Turn::Half),
        turned(4, Turn::ThreeQuarters),
    ];

    let pages = arrange_file(&input, &edits, &output).unwrap();

    assert_eq!(pages, 4);
    assert_eq!(markers_in(&output), ["A1", "A2", "A3", "A4"]);
    // The page that was not turned did not get a `/Rotate` for it.
    assert_eq!(
        rotations_in(&output),
        [None, Some(90), Some(180), Some(270)]
    );
}

#[test]
fn a_turn_is_added_to_the_rotation_the_page_already_has() {
    let scratch = ScratchDir::new("arrange-relative");
    let mut document = with_pages("A", 3);
    set_rotation(&mut document, 1, 90);
    set_rotation(&mut document, 2, 180);
    set_rotation(&mut document, 3, 270);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");
    let edits = [
        turned(1, Turn::Quarter),
        turned(2, Turn::Quarter),
        // Past a full turn: 270 and 180 make 450, which is 90.
        turned(3, Turn::Half),
    ];

    arrange_file(&input, &edits, &output).unwrap();

    assert_eq!(rotations_in(&output), [Some(180), Some(270), Some(90)]);
}

#[test]
fn three_quarter_turns_and_one_more_leave_the_page_upright() {
    let scratch = ScratchDir::new("arrange-full-turn");
    let mut document = with_pages("A", 2);
    set_rotation(&mut document, 1, 270);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[turned(1, Turn::Quarter), kept(2)], &output).unwrap();

    // Written out as 0: neither the old 270 nor 360, and the key is there
    // to say so.
    assert_eq!(rotations_in(&output), [Some(0), None]);
}

#[test]
fn a_turn_is_added_to_a_rotation_inherited_from_the_page_tree() {
    let scratch = ScratchDir::new("arrange-inherited");
    // No page has a `/Rotate` of its own, except the third: its own 270
    // wins over the 90 of the page tree.
    let mut document = with_rotation_on_the_page_tree(3, 90);
    set_rotation(&mut document, 3, 270);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");
    let edits = [turned(1, Turn::Quarter), kept(2), turned(3, Turn::Quarter)];

    arrange_file(&input, &edits, &output).unwrap();

    // Page 1: the inherited 90 and a quarter. Page 2 was not turned, and
    // must still show at 90 in a file whose new page tree has no `/Rotate`:
    // it now holds the value itself. Page 3: its own 270 and a quarter.
    assert_eq!(rotations_in(&output), [Some(180), Some(90), Some(0)]);
    let reloaded = Document::load(&output).unwrap();
    let root = reloaded.get_dictionary(root_pages_id(&reloaded)).unwrap();
    assert!(!root.has(b"Rotate"));
}

#[test]
fn a_turned_page_keeps_the_other_attributes_it_inherited() {
    let scratch = ScratchDir::new("arrange-inherited-box");
    let input = scratch.save("a.pdf", with_inherited_attributes());
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[turned(1, Turn::Quarter)], &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    let page_id = reloaded.page_iter().next().unwrap();
    let page = reloaded.get_dictionary(page_id).unwrap();
    assert_eq!(page.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    assert_eq!(
        page.get(b"MediaBox").unwrap(),
        &Object::from(vec![0.into(), 0.into(), 300.into(), 500.into()])
    );
    assert!(page.has(b"Resources"));
}

#[test]
fn a_rotation_outside_0_to_270_counts_as_the_turn_it_shows() {
    let scratch = ScratchDir::new("arrange-negative");
    let mut document = with_pages("A", 3);
    // -90 shows as 270, 450 as 90 and -270 as 90.
    set_rotation(&mut document, 1, -90);
    set_rotation(&mut document, 2, 450);
    set_rotation(&mut document, 3, -270);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");
    let edits = [
        turned(1, Turn::Quarter),
        turned(2, Turn::Quarter),
        turned(3, Turn::Half),
    ];

    arrange_file(&input, &edits, &output).unwrap();

    assert_eq!(rotations_in(&output), [Some(0), Some(180), Some(270)]);
}

#[test]
fn a_rotation_the_specification_does_not_allow_counts_as_0_when_turned() {
    let scratch = ScratchDir::new("arrange-invalid-rotation");
    let mut document = with_pages("A", 2);
    // Not a multiple of 90.
    set_rotation(&mut document, 1, 45);
    // Not a whole number at all.
    let second = document.page_iter().nth(1).unwrap();
    document
        .get_dictionary_mut(second)
        .unwrap()
        .set("Rotate", Object::Name(b"Sideways".to_vec()));
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    arrange_file(
        &input,
        &[turned(1, Turn::Quarter), turned(2, Turn::Half)],
        &output,
    )
    .unwrap();

    assert_eq!(rotations_in(&output), [Some(90), Some(180)]);
}

#[test]
fn a_page_that_is_not_turned_keeps_its_rotation_exactly_as_it_was() {
    let scratch = ScratchDir::new("arrange-untouched-rotation");
    let mut document = with_pages("A", 3);
    set_rotation(&mut document, 1, 90);
    // Neither of these is tidied up behind the caller's back: nobody asked
    // for those pages to change.
    set_rotation(&mut document, 2, -90);
    set_rotation(&mut document, 3, 45);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[kept(1), kept(2), kept(3)], &output).unwrap();

    assert_eq!(rotations_in(&output), [Some(90), Some(-90), Some(45)]);
}

// Arranging: what travels

#[test]
fn a_deleted_page_takes_its_content_with_it() {
    let scratch = ScratchDir::new("arrange-lean");
    let input = scratch.save("a.pdf", with_pages("A", 4));
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[kept(4), turned(1, Turn::Quarter)], &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert_eq!(count_of_type(&reloaded, b"Page"), 2);
    // Two content streams, one per page that stayed: the content of pages
    // 2 and 3 is not in the file. (Reading a file back may add its
    // cross-reference stream, which is the file's own bookkeeping.)
    let streams = reloaded
        .objects
        .values()
        .filter(|object| matches!(object, Object::Stream(_)) && !has_type(object, b"XRef"))
        .count();
    assert_eq!(streams, 2);
    // The font every page shares travels once.
    assert_eq!(count_of_type(&reloaded, b"Font"), 1);
}

#[test]
fn a_link_to_a_deleted_page_does_not_keep_that_page_in_the_file() {
    let scratch = ScratchDir::new("arrange-link-out");
    let input = scratch.save("a.pdf", with_a_link_from_page_1_to_page_3());
    let output = scratch.file("out.pdf");

    arrange_file(&input, &[kept(1), kept(2)], &output).unwrap();

    // Page 3 was deleted. If the link on page 1 dragged it along, its
    // content would still sit in the file although no reader shows it: the
    // opposite of what someone who deletes a page expects.
    let reloaded = Document::load(&output).unwrap();
    assert_eq!(markers(&reloaded), ["A1", "A2"]);
    assert_eq!(count_of_type(&reloaded, b"Page"), 2);
    // The link is still on its page, and now points at nothing.
    assert!(!reloaded.has_object(link_target(&reloaded)));
}

#[test]
fn a_link_to_a_page_that_moved_still_reaches_it() {
    let scratch = ScratchDir::new("arrange-link-moved");
    let input = scratch.save("a.pdf", with_a_link_from_page_1_to_page_3());
    let output = scratch.file("out.pdf");

    // Page 3, the target of the link, goes to the front and is turned.
    arrange_file(&input, &[turned(3, Turn::Half), kept(2), kept(1)], &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert_eq!(markers(&reloaded), ["A3", "A2", "A1"]);
    let pages: Vec<_> = reloaded.page_iter().collect();
    // The link sits on what is now the last page.
    let annotations = reloaded.get_page_annotations(pages[2]).unwrap();
    let target = annotations[0]
        .get(b"Dest")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap();
    assert_eq!(target, pages[0]);
}

// Arranging: what is refused

#[test]
fn an_empty_list_is_refused_and_nothing_is_written() {
    let error = refused("arrange-empty", |input, output| {
        arrange_file(input, &[], output)
    });

    assert!(matches!(error, PdfError::NoPagesLeft), "got {error:?}");
}

#[test]
fn a_page_listed_twice_is_refused_and_nothing_is_written() {
    let error = refused("arrange-repeated", |input, output| {
        // The second time with another turn: it is still the same page.
        let edits = [kept(2), kept(4), turned(2, Turn::Half)];
        arrange_file(input, &edits, output)
    });

    assert!(
        matches!(error, PdfError::PageRepeated { page: 2 }),
        "got {error:?}"
    );
}

#[test]
fn a_page_past_the_end_is_refused_and_nothing_is_written() {
    let error = refused("arrange-past-end", |input, output| {
        arrange_file(input, &[kept(2), kept(9)], output)
    });

    assert!(
        matches!(error, PdfError::PageOutOfRange { page: 9, pages: 5 }),
        "got {error:?}"
    );
}

#[test]
fn page_0_is_refused_and_nothing_is_written() {
    // Pages are counted from 1. Without a check, 0 would be looked up at
    // position -1 of the list of pages.
    let error = refused("arrange-page-0", |input, output| {
        arrange_file(input, &[kept(0), kept(1)], output)
    });

    assert!(
        matches!(error, PdfError::PageOutOfRange { page: 0, pages: 5 }),
        "got {error:?}"
    );
}

#[test]
fn the_first_edit_that_breaks_a_rule_is_the_one_reported() {
    let scratch = ScratchDir::new("arrange-first-error");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let output = scratch.file("out.pdf");

    // Page 9 does not exist and page 1 is repeated: whichever of the two
    // comes first in the list is the one named.
    let past_end_first = arrange_file(&input, &[kept(1), kept(9), kept(1)], &output);
    let repeat_first = arrange_file(&input, &[kept(1), kept(1), kept(9)], &output);

    assert!(
        matches!(
            past_end_first,
            Err(PdfError::PageOutOfRange { page: 9, pages: 5 })
        ),
        "got {past_end_first:?}"
    );
    assert!(
        matches!(repeat_first, Err(PdfError::PageRepeated { page: 1 })),
        "got {repeat_first:?}"
    );
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn unreadable_and_encrypted_inputs_are_refused_by_all_three() {
    let scratch = ScratchDir::new("arrange-refused-inputs");
    let not_a_pdf = scratch.file("notes.pdf");
    fs::write(&not_a_pdf, "just some text, no PDF here").unwrap();
    let locked = scratch.save("locked.pdf", encrypted(with_pages("A", 2), "secret"));
    let output = scratch.file("out.pdf");
    let first = selection("1");

    let unreadable = [
        arrange_file(&not_a_pdf, &[kept(1)], &output),
        rotate_file(&not_a_pdf, &first, Turn::Quarter, &output),
        delete_file(&not_a_pdf, &first, &output),
    ];
    for result in unreadable {
        match result {
            Err(PdfError::Read { path, .. }) => assert_eq!(path, not_a_pdf),
            other => panic!("expected a read error, got {other:?}"),
        }
    }

    let encrypted = [
        arrange_file(&locked, &[kept(1)], &output),
        rotate_file(&locked, &first, Turn::Quarter, &output),
        delete_file(&locked, &first, &output),
    ];
    for result in encrypted {
        // One file is read, so it is "the file" and not "input 1".
        assert!(
            matches!(
                result,
                Err(PdfError::Encrypted {
                    input: WhichInput::Only
                })
            ),
            "got {result:?}"
        );
    }
    assert!(!output.exists(), "nothing should be written");
}

#[test]
fn a_save_that_fails_names_the_output_and_leaves_the_input_alone() {
    let scratch = ScratchDir::new("arrange-missing-folder");
    let input = scratch.save("a.pdf", with_pages("A", 2));
    let input_before = fs::read(&input).unwrap();
    let output = scratch.file("missing").join("out.pdf");

    match arrange_file(&input, &[turned(2, Turn::Quarter)], &output) {
        Err(PdfError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), ["a.pdf"]);
    assert_eq!(fs::read(&input).unwrap(), input_before);
}

#[test]
fn arrange_may_write_over_its_own_input() {
    // The core allows it, as it does for an extraction: everything is read
    // before anything is written, and the file is replaced in one step.
    // The front ends are the ones that refuse it or ask first.
    let scratch = ScratchDir::new("arrange-over-input");
    let input = scratch.save("a.pdf", with_pages("A", 3));

    let pages = arrange_file(&input, &[turned(3, Turn::Quarter), kept(1)], &input).unwrap();

    assert_eq!(pages, 2);
    assert_eq!(markers_in(&input), ["A3", "A1"]);
    assert_eq!(rotations_in(&input), [Some(90), None]);
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

// Rotating

#[test]
fn rotate_turns_the_chosen_pages_and_keeps_every_page_in_its_place() {
    let scratch = ScratchDir::new("rotate-chosen");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let input_before = fs::read(&input).unwrap();
    let output = scratch.file("out.pdf");

    let pages = rotate_file(&input, &selection("2,4-"), Turn::Quarter, &output).unwrap();

    assert_eq!(pages, 5);
    assert_eq!(markers_in(&output), ["A1", "A2", "A3", "A4", "A5"]);
    assert_eq!(
        rotations_in(&output),
        [None, Some(90), None, Some(90), Some(90)]
    );
    assert_eq!(fs::read(&input).unwrap(), input_before);
    assert_eq!(scratch.entries(), ["a.pdf", "out.pdf"]);
}

#[test]
fn the_order_of_a_selection_does_not_move_the_pages_it_turns() {
    let scratch = ScratchDir::new("rotate-selection-order");
    let input = scratch.save("a.pdf", with_pages("A", 3));
    let output = scratch.file("out.pdf");

    // For an extraction, "3,1" means page 3 and then page 1.
    rotate_file(&input, &selection("3,1"), Turn::Half, &output).unwrap();

    assert_eq!(markers_in(&output), ["A1", "A2", "A3"]);
    assert_eq!(rotations_in(&output), [Some(180), None, Some(180)]);
}

#[test]
fn rotate_adds_to_the_rotation_a_page_has_or_inherits() {
    let scratch = ScratchDir::new("rotate-relative");
    let mut document = with_rotation_on_the_page_tree(3, 90);
    set_rotation(&mut document, 2, 180);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    rotate_file(&input, &selection("1-2"), Turn::ThreeQuarters, &output).unwrap();

    // Page 1: the inherited 90 and 270 make a full turn. Page 2: its own
    // 180 and 270 make 450, which is 90. Page 3 was not chosen and keeps
    // the 90 it inherited.
    assert_eq!(rotations_in(&output), [Some(0), Some(90), Some(90)]);
}

#[test]
fn rotating_by_no_turn_writes_the_pages_as_they_are() {
    let scratch = ScratchDir::new("rotate-zero");
    let mut document = with_pages("A", 2);
    set_rotation(&mut document, 2, -90);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    let pages = rotate_file(&input, &selection("1-"), Turn::Zero, &output).unwrap();

    assert_eq!(pages, 2);
    assert_eq!(markers_in(&output), ["A1", "A2"]);
    assert_eq!(rotations_in(&output), [None, Some(-90)]);
}

#[test]
fn rotate_refuses_a_page_past_the_end_and_writes_nothing() {
    let error = refused("rotate-past-end", |input, output| {
        rotate_file(input, &selection("2,9"), Turn::Quarter, output)
    });

    assert!(
        matches!(error, PdfError::PageOutOfRange { page: 9, pages: 5 }),
        "got {error:?}"
    );
}

#[test]
fn rotate_may_write_over_its_own_input() {
    let scratch = ScratchDir::new("rotate-over-input");
    let input = scratch.save("a.pdf", with_pages("A", 2));

    rotate_file(&input, &selection("2"), Turn::Quarter, &input).unwrap();

    assert_eq!(markers_in(&input), ["A1", "A2"]);
    assert_eq!(rotations_in(&input), [None, Some(90)]);
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

// Deleting

#[test]
fn delete_removes_the_chosen_pages_and_keeps_the_rest_in_order() {
    let scratch = ScratchDir::new("delete-chosen");
    let input = scratch.save("a.pdf", with_pages("A", 5));
    let input_before = fs::read(&input).unwrap();
    let output = scratch.file("out.pdf");

    // Written backwards on purpose: the pages that stay keep their order.
    let pages = delete_file(&input, &selection("4,2"), &output).unwrap();

    assert_eq!(pages, 3);
    assert_eq!(markers_in(&output), ["A1", "A3", "A5"]);
    assert_eq!(fs::read(&input).unwrap(), input_before);
    assert_eq!(scratch.entries(), ["a.pdf", "out.pdf"]);
}

#[test]
fn delete_leaves_the_rotation_of_the_other_pages_alone() {
    let scratch = ScratchDir::new("delete-rotations");
    let mut document = with_rotation_on_the_page_tree(4, 90);
    set_rotation(&mut document, 3, 270);
    let input = scratch.save("a.pdf", document);
    let output = scratch.file("out.pdf");

    delete_file(&input, &selection("1"), &output).unwrap();

    // Pages 2 and 4 still show at the 90 they inherited, page 3 at its own.
    assert_eq!(markers_in(&output), ["A2", "A3", "A4"]);
    assert_eq!(rotations_in(&output), [Some(90), Some(270), Some(90)]);
}

#[test]
fn the_content_of_a_deleted_page_is_not_in_the_new_file() {
    let scratch = ScratchDir::new("delete-lean");
    let input = scratch.save("a.pdf", with_a_link_from_page_1_to_page_3());
    let output = scratch.file("out.pdf");

    delete_file(&input, &selection("3"), &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert_eq!(markers(&reloaded), ["A1", "A2"]);
    // The link on page 1 pointed at page 3: it did not keep it alive.
    assert_eq!(count_of_type(&reloaded, b"Page"), 2);
    assert!(!reloaded.has_object(link_target(&reloaded)));
    let streams = reloaded
        .objects
        .values()
        .filter(|object| matches!(object, Object::Stream(_)) && !has_type(object, b"XRef"))
        .count();
    assert_eq!(streams, 2);
}

#[test]
fn deleting_every_page_is_refused_and_nothing_is_written() {
    // Once naming each page, once with a range that takes them all.
    for pages in ["1,2,3,4,5", "1-"] {
        let error = refused("delete-all", |input, output| {
            delete_file(input, &selection(pages), output)
        });

        assert!(
            matches!(error, PdfError::NoPagesLeft),
            "{pages}: got {error:?}"
        );
    }
}

#[test]
fn delete_refuses_a_page_past_the_end_and_writes_nothing() {
    let error = refused("delete-past-end", |input, output| {
        delete_file(input, &selection("1,9"), output)
    });

    assert!(
        matches!(error, PdfError::PageOutOfRange { page: 9, pages: 5 }),
        "got {error:?}"
    );
}

#[test]
fn delete_may_write_over_its_own_input() {
    let scratch = ScratchDir::new("delete-over-input");
    let input = scratch.save("a.pdf", with_pages("A", 3));

    delete_file(&input, &selection("2"), &input).unwrap();

    assert_eq!(markers_in(&input), ["A1", "A3"]);
    assert_eq!(scratch.entries(), ["a.pdf"]);
}
