//! Tests for page selections. No test here opens a file: reading the text
//! needs no document, and checking it against one only needs a page count.

use super::*;

/// Reads `text` as a page selection.
fn parse(text: &str) -> Result<PageSelection, PageSyntaxError> {
    text.parse()
}

/// The pages that `text` selects in a document of `page_count` pages.
fn pages(text: &str, page_count: usize) -> Vec<usize> {
    parse(text).unwrap().resolve(page_count).unwrap()
}

// What a selection may say

#[test]
fn a_single_number_selects_that_page() {
    assert_eq!(pages("3", 5), [3]);
}

#[test]
fn a_range_includes_both_of_its_ends() {
    assert_eq!(pages("2-4", 5), [2, 3, 4]);
}

#[test]
fn a_range_of_one_page_is_allowed() {
    assert_eq!(pages("3-3", 5), [3]);
}

#[test]
fn an_open_range_runs_to_the_last_page() {
    assert_eq!(pages("3-", 5), [3, 4, 5]);
    // The same text means more pages in a longer document: that is why it
    // can only be turned into numbers once the page count is known.
    assert_eq!(pages("3-", 7), [3, 4, 5, 6, 7]);
}

#[test]
fn the_last_page_is_still_inside_the_document() {
    assert_eq!(pages("5", 5), [5]);
    assert_eq!(pages("5-", 5), [5]);
    assert_eq!(pages("1-5", 5), [1, 2, 3, 4, 5]);
}

#[test]
fn items_keep_the_order_they_were_written_in() {
    assert_eq!(pages("3,1", 5), [3, 1]);
    assert_eq!(pages("4-5,1-2", 5), [4, 5, 1, 2]);
    assert_eq!(pages("4-,1", 5), [4, 5, 1]);
}

#[test]
fn the_documented_example_reads_as_documented() {
    assert_eq!(pages("1-3,7,10-", 12), [1, 2, 3, 7, 10, 11, 12]);
}

#[test]
fn spaces_around_an_item_are_ignored() {
    assert_eq!(pages(" 1-2 , 4 ", 5), [1, 2, 4]);
    assert_eq!(pages("\t3\n", 5), [3]);
}

#[test]
fn ranges_that_only_touch_do_not_repeat_a_page() {
    assert_eq!(pages("1-2,3-4", 4), [1, 2, 3, 4]);
    assert_eq!(pages("3-,1-2", 5), [3, 4, 5, 1, 2]);
}

#[test]
fn leading_zeros_do_not_change_the_number() {
    assert_eq!(pages("007", 9), [7]);
}

// What is refused when the text is read

#[test]
fn empty_text_is_refused() {
    for text in ["", "   ", "\t"] {
        assert_eq!(parse(text), Err(PageSyntaxError::Empty), "{text:?}");
    }
}

#[test]
fn an_empty_item_is_refused_with_its_position() {
    let cases = [("1,,3", 2), ("1,", 2), (",1", 1), ("1, ,3", 2), (",", 1)];
    for (text, position) in cases {
        assert_eq!(
            parse(text),
            Err(PageSyntaxError::EmptyItem { position }),
            "{text:?}"
        );
    }
}

#[test]
fn what_is_not_a_number_is_refused_and_names_the_item() {
    // Each of these is one item, and none is `N`, `A-B` or `A-`.
    let items = [
        "abc", "1.5", "+2", "-3", "-", "1-2-3", "1--3", "a-3", "3-b", "2 - 3", "1 2", "all",
    ];
    for item in items {
        assert_eq!(
            parse(item),
            Err(PageSyntaxError::NotANumber {
                item: item.to_owned()
            }),
            "{item:?}"
        );
    }
}

#[test]
fn the_item_named_is_the_wrong_one_without_its_spaces() {
    assert_eq!(
        parse("1, x ,3"),
        Err(PageSyntaxError::NotANumber {
            item: "x".to_owned()
        })
    );
}

#[test]
fn a_number_too_big_to_be_a_page_is_refused() {
    // Twenty-six digits: more than any `usize` holds.
    let item = "99999999999999999999999999";
    assert_eq!(
        parse(item),
        Err(PageSyntaxError::NotANumber {
            item: item.to_owned()
        })
    );
}

#[test]
fn page_zero_is_refused() {
    for text in ["0", "000", "0-3", "3-0", "0-", "1,0"] {
        assert_eq!(parse(text), Err(PageSyntaxError::PageZero), "{text:?}");
    }
}

#[test]
fn a_backwards_range_is_refused() {
    assert_eq!(
        parse("5-3"),
        Err(PageSyntaxError::BackwardsRange { first: 5, last: 3 })
    );
}

#[test]
fn a_page_listed_twice_is_refused() {
    let cases = [
        // The example from the documentation: 3 is already inside 1-5.
        ("1-5,3", 3),
        ("2,2", 2),
        ("3,1-5", 3),
        ("1-3,3-5", 3),
        ("5-9,1-6", 5),
        // An open range holds every page from its start on, so a later page
        // is in it whatever the document.
        ("4-,6", 6),
        ("6,4-", 6),
        // Two open ranges share everything from the later start on.
        ("2-,4-", 4),
    ];
    for (text, page) in cases {
        assert_eq!(
            parse(text),
            Err(PageSyntaxError::RepeatedPage { page }),
            "{text:?}"
        );
    }
}

#[test]
fn the_lowest_repeated_page_is_the_one_reported() {
    assert_eq!(
        parse("7,7,2,2"),
        Err(PageSyntaxError::RepeatedPage { page: 2 })
    );
}

#[test]
fn the_first_mistake_in_the_text_is_the_one_reported() {
    // Item 1 is backwards and item 2 is not a number: the first one wins.
    assert_eq!(
        parse("5-3,x"),
        Err(PageSyntaxError::BackwardsRange { first: 5, last: 3 })
    );
}

#[test]
fn syntax_messages_say_what_to_fix() {
    let cases = [
        (PageSyntaxError::Empty, "no pages were given"),
        (
            PageSyntaxError::EmptyItem { position: 2 },
            "item 2 of the page list is empty",
        ),
        (
            PageSyntaxError::NotANumber {
                item: "abc".to_owned(),
            },
            "\"abc\" is not a page (N) or a range of pages (A-B or A-)",
        ),
        (
            PageSyntaxError::PageZero,
            "pages are counted from 1: there is no page 0",
        ),
        (
            PageSyntaxError::BackwardsRange { first: 5, last: 3 },
            "the range 5-3 runs backwards: a range goes from the lower page to the higher one",
        ),
        (
            PageSyntaxError::RepeatedPage { page: 3 },
            "page 3 is listed more than once",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

// What is refused once the page count is known

#[test]
fn a_page_past_the_end_is_refused_when_resolved() {
    // (selection, the page it names that the 5-page document lacks)
    let cases = [("9", 9), ("3-9", 9), ("7-", 7), ("1,6", 6), ("6-", 6)];
    for (text, page) in cases {
        // The text itself is fine: only the document makes it wrong.
        let selection = parse(text).unwrap();

        let result = selection.resolve(5);

        assert!(
            matches!(result, Err(PdfError::PageOutOfRange { page: p, pages: 5 }) if p == page),
            "{text:?} gave {result:?}"
        );
    }
}

#[test]
fn the_first_item_past_the_end_is_the_one_reported() {
    let result = parse("9,7").unwrap().resolve(5);

    assert!(
        matches!(result, Err(PdfError::PageOutOfRange { page: 9, pages: 5 })),
        "got {result:?}"
    );
}

#[test]
fn a_huge_range_is_refused_without_being_listed() {
    // A range of 2^64 pages on a 64-bit system. Building the list first and
    // checking it afterwards would run out of memory; the check looks at
    // the two ends only.
    let text = format!("1-{}", usize::MAX);
    let selection = parse(&text).unwrap();

    let result = selection.resolve(5);

    assert!(
        matches!(result, Err(PdfError::PageOutOfRange { pages: 5, .. })),
        "got {result:?}"
    );
}

#[test]
fn a_selection_can_be_resolved_against_several_documents() {
    // `resolve` only borrows the selection, so one parsed selection serves
    // any number of documents.
    let selection = parse("2-").unwrap();

    assert_eq!(selection.resolve(3).unwrap(), [2, 3]);
    assert_eq!(selection.resolve(4).unwrap(), [2, 3, 4]);
    assert!(selection.resolve(1).is_err());
}
