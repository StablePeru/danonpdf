//! Tests for the Pages view's state. None of them opens a window, and all
//! but the last three start no thread either: a test plays both the
//! drawing code (it says which rows are on screen) and the worker (it
//! hands in the replies), so every rule is checked one step at a time.
//!
//! No file is read anywhere in this state, which is the point of it: the
//! paths below name files that do not exist.

use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use danonpdf_core::{MAX_RENDER_HEIGHT, MAX_RENDER_WIDTH, PdfError, WhichInput, error_chain};

use super::*;
use crate::thumbnail_worker::PageEngine;

/// The thumbnail box at one pixel per point.
const SIZE: ThumbnailSize = ThumbnailSize {
    width: 160,
    max_height: 226,
};

/// A path inside a folder called `docs`. Nothing has to exist there.
fn doc(name: &str) -> PathBuf {
    Path::new("docs").join(name)
}

/// A small image whose every byte is `page`, so a test can tell which
/// page an image is of.
fn image_of(page: usize) -> PageImage {
    PageImage {
        width: 2,
        height: 3,
        rgba: vec![page as u8; 2 * 3 * 4],
    }
}

/// A view in which `docs/report.pdf` is open and has `pages` pages: the
/// file was chosen, the request went out, and the worker's answer came.
fn open_document(pages: usize) -> Thumbnails {
    let mut state = Thumbnails::default();
    state.choose([doc("report.pdf")]);
    state.take_requests();
    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Ok(pages),
    });
    state
}

/// The page numbers of the page requests in `requests`, in order. Fails
/// the test if one of them is not a page request.
fn pages_of(requests: &[Request]) -> Vec<usize> {
    requests
        .iter()
        .map(|request| match request {
            Request::Page { page, .. } => page.get(),
            other => panic!("expected a page request, got {other:?}"),
        })
        .collect()
}

/// The answer a worker would give to a page request that it could draw.
fn drawn(request: &Request) -> Reply {
    match request {
        Request::Page {
            generation, page, ..
        } => Reply::Page {
            generation: *generation,
            page: *page,
            result: Ok(image_of(page.get())),
        },
        other => panic!("expected a page request, got {other:?}"),
    }
}

/// Plays the worker: takes every request that is waiting and answers it
/// with a drawn page. Returns the pages that were asked for.
fn draw_everything_asked_for(state: &mut Thumbnails) -> Vec<usize> {
    let requests = state.take_requests();
    for request in &requests {
        state.receive(drawn(request));
    }
    pages_of(&requests)
}

/// The pages that have a slot of any kind, in order.
fn pages_with_a_slot(state: &Thumbnails, pages: usize) -> Vec<usize> {
    (1..=pages)
        .filter(|page| state.slot(*page).is_some())
        .collect()
}

// Choosing the file

#[test]
fn a_new_view_shows_nothing_and_asks_for_nothing() {
    let mut state = Thumbnails::default();

    assert_eq!(state.document(), &Document::None);
    assert_eq!(state.pages(), None);
    assert_eq!(state.status(), &Status::Idle);
    assert_eq!(state.kept(), 0);
    assert!(!state.has_requests());
    assert!(state.take_requests().is_empty());
    assert!(state.take_fresh().is_empty());
}

#[test]
fn choosing_a_file_asks_the_worker_to_open_it() {
    let mut state = Thumbnails::default();
    let before = state.generation().current();

    // The file does not exist, and nothing notices: this thread does not
    // touch the disk. Whether the file can be read is the worker's answer.
    state.choose([doc("report.pdf")]);

    let generation = state.generation().current();
    assert_eq!(generation, before + 1);
    assert_eq!(
        state.document(),
        &Document::Opening {
            path: doc("report.pdf")
        }
    );
    assert_eq!(state.status(), &Status::Idle);
    assert!(state.has_requests());
    assert_eq!(
        state.take_requests(),
        [Request::Open {
            generation,
            path: doc("report.pdf")
        }]
    );
    assert!(!state.has_requests());
}

#[test]
fn of_several_files_the_first_pdf_is_opened_and_the_rest_are_counted() {
    let mut state = Thumbnails::default();

    state.choose([doc("notes.txt"), doc("a.pdf"), doc("b.pdf"), doc("c.pdf")]);

    assert_eq!(state.document(), &Document::Opening { path: doc("a.pdf") });
    assert_eq!(
        state.status(),
        &Status::LeftOut {
            other_pdfs: 2,
            not_pdf: 1
        }
    );
    assert_eq!(state.take_requests().len(), 1);
}

#[test]
fn files_that_are_not_pdfs_change_nothing_and_are_counted() {
    let mut state = open_document(5);
    let generation = state.generation().current();

    state.choose([doc("photo.jpg"), doc("notes.txt")]);

    assert_eq!(state.pages(), Some(5));
    assert_eq!(state.generation().current(), generation);
    assert_eq!(state.status(), &Status::NoPdf { not_pdf: 2 });
    assert!(!state.has_requests());
}

#[test]
fn choosing_nothing_changes_nothing() {
    let mut state = Thumbnails::default();
    state.choose([doc("photo.jpg")]);

    let nothing: [PathBuf; 0] = [];
    state.choose(nothing);

    assert_eq!(state.document(), &Document::None);
    assert_eq!(state.status(), &Status::NoPdf { not_pdf: 1 });
}

// The answer to "open this file"

#[test]
fn the_page_count_arrives_and_the_document_is_open() {
    let mut state = Thumbnails::default();
    state.choose([doc("a.pdf"), doc("b.pdf")]);
    // The request goes out; the reply below is the answer to it.
    state.take_requests();

    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Ok(12),
    });

    assert_eq!(
        state.document(),
        &Document::Open {
            path: doc("a.pdf"),
            pages: 12
        }
    );
    assert_eq!(state.pages(), Some(12));
    // What was said about the files left out is still true.
    assert_eq!(
        state.status(),
        &Status::LeftOut {
            other_pdfs: 1,
            not_pdf: 0
        }
    );
    // Opening asks for no page: which pages are wanted depends on what
    // the window shows, and only the drawing code knows that.
    assert!(!state.has_requests());
}

#[test]
fn the_answer_about_a_file_that_was_replaced_is_ignored() {
    let mut state = Thumbnails::default();
    state.choose([doc("first.pdf")]);
    let first = state.generation().current();
    state.choose([doc("second.pdf")]);

    // The worker had already started on the first file.
    state.receive(Reply::Opened {
        generation: first,
        result: Ok(99),
    });

    assert_eq!(
        state.document(),
        &Document::Opening {
            path: doc("second.pdf")
        }
    );
}

#[test]
fn a_file_that_cannot_be_opened_is_not_shown_and_the_status_says_why() {
    let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let errors = [
        RenderError::Encrypted,
        RenderError::UnsupportedProtection,
        RenderError::NotPdf {
            path: doc("report.pdf"),
        },
        RenderError::Read {
            path: doc("report.pdf"),
            source: denied(),
        },
        RenderError::Broken {
            reason: "a page cannot be read",
        },
    ];

    for error in errors {
        // The whole chain of causes, in the core's words.
        let message = error_chain(&error);
        let mut state = Thumbnails::default();
        state.choose([doc("report.pdf")]);

        state.receive(Reply::Opened {
            generation: state.generation().current(),
            result: Err(error),
        });

        assert_eq!(state.document(), &Document::None, "{message}");
        assert_eq!(
            state.status(),
            &Status::NotAccepted {
                file: doc("report.pdf"),
                message
            }
        );
    }
}

#[test]
fn a_file_that_needs_a_password_is_refused_in_the_words_of_the_other_views() {
    let mut state = Thumbnails::default();
    state.choose([doc("locked.pdf")]);

    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Err(RenderError::Encrypted),
    });

    assert_eq!(
        state.status(),
        &Status::NotAccepted {
            file: doc("locked.pdf"),
            // What the Split view says about the same file: the wording is
            // asked of the core and not copied.
            message: PdfError::Encrypted {
                input: WhichInput::Only
            }
            .to_string()
        }
    );
}

#[test]
fn a_file_that_fails_to_open_also_closes_the_one_shown_before() {
    // Choosing a file closes the one on screen at once, before anything
    // is known about the new one: its pages must not go on being drawn.
    // So a failed choice leaves the view empty, unlike in the Split view,
    // where the file is read before the old one is let go.
    let mut state = open_document(5);

    state.choose([doc("locked.pdf")]);
    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Err(RenderError::Encrypted),
    });

    assert_eq!(state.document(), &Document::None);
    assert_eq!(state.pages(), None);
}

#[test]
fn a_file_without_pages_is_not_shown() {
    let mut state = Thumbnails::default();
    state.choose([doc("empty.pdf")]);

    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Ok(0),
    });

    assert_eq!(state.document(), &Document::None);
    assert_eq!(
        state.status(),
        &Status::NotAccepted {
            file: doc("empty.pdf"),
            message: texts::NO_PAGES.to_owned()
        }
    );
}

#[test]
fn a_missing_library_gives_one_message_that_names_the_file_looked_for() {
    let library = Path::new("app").join("pdfium.dll");
    let mut state = Thumbnails::default();
    state.choose([doc("report.pdf")]);
    state.take_requests();

    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Err(RenderError::LibraryMissing {
            path: library.clone(),
        }),
    });

    assert_eq!(state.document(), &Document::None);
    // About the program, not about the PDF: the status does not blame the
    // file that was chosen, and names the one that is missing.
    let Status::NoEngine { message } = state.status() else {
        panic!("unexpected status: {:?}", state.status());
    };
    assert_eq!(
        message,
        &format!("the PDFium library is missing: {}", library.display())
    );
    assert!(!state.has_requests());
}

#[test]
fn every_way_for_the_library_not_to_load_is_the_same_kind_of_status() {
    let library = || Path::new("app").join("pdfium.dll");
    let errors = [
        RenderError::LibraryFolderUnknown {
            source: io::Error::other("no such process"),
        },
        RenderError::LibraryMissing { path: library() },
        RenderError::LibraryFailed {
            path: library(),
            source: "not a valid library".into(),
        },
        RenderError::LibraryAlreadyLoaded {
            loaded: Path::new("other").join("pdfium.dll"),
            asked: library(),
        },
    ];

    for error in errors {
        let message = error_chain(&error);
        let mut state = Thumbnails::default();
        state.choose([doc("report.pdf")]);

        state.receive(Reply::Opened {
            generation: state.generation().current(),
            result: Err(error),
        });

        assert_eq!(state.status(), &Status::NoEngine { message });
    }
}

// Which pages are asked for

#[test]
fn only_the_rows_on_screen_and_a_margin_are_asked_for() {
    let mut state = open_document(100);
    let generation = state.generation().current();

    // Four thumbnails per row, and rows 5, 6 and 7 on screen (counted
    // from 0): pages 21 to 32.
    state.show_rows(5..8, 4, SIZE);

    let requests = state.take_requests();
    // The pages on screen first, then one row above and one below, so
    // that a little scrolling finds its pages already drawn.
    let mut expected: Vec<usize> = (21..=32).collect();
    expected.extend([17, 18, 19, 20, 33, 34, 35, 36]);
    assert_eq!(pages_of(&requests), expected);
    // 20 pages of 100: the rest of the document costs nothing.
    assert_eq!(
        pages_with_a_slot(&state, 100),
        (17..=36).collect::<Vec<_>>()
    );
    assert_eq!(state.slot(21), Some(&Slot::Requested));
    assert_eq!(state.slot(16), None);
    assert_eq!(state.slot(37), None);
    // Every request says which file, in which generation and how large.
    assert_eq!(
        requests[0],
        Request::Page {
            generation,
            path: doc("report.pdf"),
            page: NonZeroUsize::new(21).unwrap(),
            size: SIZE
        }
    );
}

#[test]
fn a_page_is_asked_for_once() {
    let mut state = open_document(100);
    state.show_rows(5..8, 4, SIZE);
    state.take_requests();

    // The next frame shows the same rows.
    state.show_rows(5..8, 4, SIZE);

    assert!(!state.has_requests());
}

#[test]
fn the_rows_at_both_ends_stay_inside_the_document() {
    // 10 pages, four per row: rows 0 and 1 are full, row 2 has two pages.
    let mut state = open_document(10);

    state.show_rows(0..2, 4, SIZE);
    assert_eq!(
        pages_of(&state.take_requests()),
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    );

    // Asked for rows that the document does not have: nothing is wanted,
    // and nothing panics.
    let mut state = open_document(10);
    state.show_rows(7..9, 4, SIZE);
    assert!(!state.has_requests());
}

#[test]
fn nothing_is_asked_for_before_the_document_is_open() {
    let mut state = Thumbnails::default();
    state.show_rows(0..3, 4, SIZE);
    assert!(!state.has_requests());

    state.choose([doc("report.pdf")]);
    state.take_requests();
    state.show_rows(0..3, 4, SIZE);
    assert!(!state.has_requests());
}

// Replies about pages

#[test]
fn a_drawn_page_fills_its_slot_and_its_image_waits_for_the_view() {
    let mut state = open_document(10);
    state.show_rows(0..1, 4, SIZE);
    let requests = state.take_requests();

    // Page 3 arrives before page 1: each reply says which page it is.
    state.receive(drawn(&requests[2]));

    assert_eq!(state.slot(3), Some(&Slot::Ready));
    assert_eq!(state.slot(1), Some(&Slot::Requested));
    assert_eq!(state.kept(), 1);
    assert_eq!(state.take_fresh(), [(3, image_of(3))]);
    // The image is handed over once: the view turns it into a texture.
    assert!(state.take_fresh().is_empty());
    assert_eq!(state.slot(3), Some(&Slot::Ready));
}

#[test]
fn a_page_that_cannot_be_drawn_is_marked_with_the_reason() {
    let mut state = open_document(10);
    state.show_rows(0..1, 4, SIZE);
    state.take_requests();
    let broken = RenderError::Broken {
        reason: "a page cannot be read",
    };
    let reason = error_chain(&broken);

    state.receive(Reply::Page {
        generation: state.generation().current(),
        page: NonZeroUsize::new(2).unwrap(),
        result: Err(broken),
    });

    assert_eq!(state.slot(2), Some(&Slot::Failed { reason }));
    // The other pages are untouched, and there is no image to hand over.
    assert_eq!(state.slot(1), Some(&Slot::Requested));
    assert!(state.take_fresh().is_empty());
    // The document stays open: one bad page does not close the file.
    assert_eq!(state.pages(), Some(10));
}

#[test]
fn a_reply_of_an_older_generation_is_ignored() {
    let mut state = open_document(10);
    state.show_rows(0..1, 4, SIZE);
    let old_requests = state.take_requests();

    // Another file is chosen and opened while page 1 of the first one is
    // still being drawn. Its page 1 is asked for too.
    state.choose([doc("other.pdf")]);
    state.take_requests();
    state.receive(Reply::Opened {
        generation: state.generation().current(),
        result: Ok(10),
    });
    state.show_rows(0..1, 4, SIZE);

    // Page 1 of the old file arrives. It must not be shown as page 1 of
    // the new one.
    state.receive(drawn(&old_requests[0]));

    assert_eq!(state.slot(1), Some(&Slot::Requested));
    assert!(state.take_fresh().is_empty());
}

#[test]
fn a_reply_about_a_page_that_is_not_waited_for_is_ignored() {
    let mut state = open_document(10);
    state.show_rows(0..1, 4, SIZE);
    state.take_requests();
    let generation = state.generation().current();
    let reply = |page: usize| Reply::Page {
        generation,
        page: NonZeroUsize::new(page).unwrap(),
        result: Ok(image_of(page)),
    };

    // Never asked for, and past the end of the document.
    state.receive(reply(9));
    state.receive(reply(11));
    // Answered twice: the second answer changes nothing.
    state.receive(reply(1));
    state.take_fresh();
    state.receive(reply(1));

    assert_eq!(state.slot(9), None);
    assert_eq!(state.slot(11), None);
    assert!(state.take_fresh().is_empty());
}

// Another file

#[test]
fn choosing_another_file_starts_a_new_generation_from_nothing() {
    let mut state = open_document(100);
    state.show_rows(0..2, 4, SIZE);
    let requests = state.take_requests();
    state.receive(drawn(&requests[0]));
    state.receive(drawn(&requests[1]));
    state.show_rows(0..3, 4, SIZE);
    let before = state.generation().current();
    // At this point: pages drawn, pages waited for, an image the view has
    // not taken yet and requests that have not gone out yet.
    assert!(state.kept() > 0 && state.has_requests());

    state.choose([doc("other.pdf")]);

    let generation = state.generation().current();
    assert_eq!(generation, before + 1);
    assert_eq!(
        state.document(),
        &Document::Opening {
            path: doc("other.pdf")
        }
    );
    assert_eq!(pages_with_a_slot(&state, 100), Vec::<usize>::new());
    assert_eq!(state.kept(), 0);
    assert!(state.take_fresh().is_empty());
    // The requests of the old file that were still waiting to go out are
    // gone: only the new file is asked about.
    assert_eq!(
        state.take_requests(),
        [Request::Open {
            generation,
            path: doc("other.pdf")
        }]
    );
}

// Scrolling, leaving and coming back

#[test]
fn scrolling_away_cancels_what_is_waited_for_and_keeps_what_arrived() {
    let mut state = open_document(100);
    state.show_rows(0..2, 4, SIZE);
    let old_requests = state.take_requests();
    let old = state.generation().current();
    // Pages 1 to 4 arrive; 5 to 12 are still in the worker's queue.
    for request in &old_requests[..4] {
        state.receive(drawn(request));
    }

    // The user drags the scroll bar far down.
    state.show_rows(20..22, 4, SIZE);

    // A new generation: the worker will skip pages 5 to 12, and the pages
    // on screen now do not wait behind them.
    let generation = state.generation().current();
    assert_eq!(generation, old + 1);
    let requests = state.take_requests();
    let mut expected: Vec<usize> = (81..=88).collect();
    expected.extend([77, 78, 79, 80, 89, 90, 91, 92]);
    assert_eq!(pages_of(&requests), expected);
    assert!(
        requests
            .iter()
            .all(|request| request.generation() == generation)
    );
    // What had arrived is kept: scrolling back up shows it at once.
    assert_eq!(state.slot(1), Some(&Slot::Ready));
    assert_eq!(state.slot(4), Some(&Slot::Ready));
    // What had not is forgotten, and a late answer does not bring it back.
    assert_eq!(state.slot(5), None);
    state.receive(drawn(&old_requests[4]));
    assert_eq!(state.slot(5), None);
}

#[test]
fn scrolling_a_little_cancels_nothing() {
    let mut state = open_document(100);
    state.show_rows(0..2, 4, SIZE);
    state.take_requests();
    let generation = state.generation().current();

    // One row down: everything asked for so far is still on screen or in
    // the margin.
    state.show_rows(1..3, 4, SIZE);

    assert_eq!(state.generation().current(), generation);
    // Only the row that came into the margin is new.
    assert_eq!(pages_of(&state.take_requests()), [13, 14, 15, 16]);
}

#[test]
fn leaving_the_view_cancels_what_is_waited_for_and_coming_back_asks_again() {
    let mut state = open_document(100);
    state.show_rows(0..1, 4, SIZE);
    let requests = state.take_requests();
    state.receive(drawn(&requests[0]));
    let before = state.generation().current();

    state.hide();

    assert_eq!(state.generation().current(), before + 1);
    // The document stays open and the page that arrived stays drawn.
    assert_eq!(state.pages(), Some(100));
    assert_eq!(state.slot(1), Some(&Slot::Ready));
    assert_eq!(state.slot(2), None);
    assert!(!state.has_requests());

    // Back in the view: the pages that were cancelled are asked for again.
    state.show_rows(0..1, 4, SIZE);
    assert_eq!(pages_of(&state.take_requests()), [2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn leaving_while_a_file_is_opening_does_not_lose_the_file() {
    let mut state = Thumbnails::default();
    state.choose([doc("report.pdf")]);
    let generation = state.generation().current();

    state.hide();
    state.receive(Reply::Opened {
        generation,
        result: Ok(7),
    });

    // Opening is one short call, and its answer is wanted when the user
    // comes back: leaving does not cancel it.
    assert_eq!(state.generation().current(), generation);
    assert_eq!(state.pages(), Some(7));
}

#[test]
fn leaving_an_empty_view_does_nothing() {
    let mut state = Thumbnails::default();

    state.hide();

    assert_eq!(state.generation().current(), 0);
    assert_eq!(state.status(), &Status::Idle);
}

#[test]
fn another_scale_draws_every_thumbnail_again() {
    let mut state = open_document(100);
    state.show_rows(0..1, 4, SIZE);
    draw_everything_asked_for(&mut state);
    let before = state.generation().current();
    // The window moves to a screen with twice the pixels per point.
    let sharper = thumbnail_size(2.0);

    state.show_rows(0..1, 4, sharper);

    // The thumbnails drawn for the other screen would look blurred here:
    // all of them are forgotten, the images the view had not taken too.
    assert_eq!(state.generation().current(), before + 1);
    assert_eq!(state.kept(), 0);
    assert!(state.take_fresh().is_empty());
    let requests = state.take_requests();
    assert_eq!(pages_of(&requests), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(requests.iter().all(|request| matches!(
        request,
        Request::Page { size, .. } if *size == sharper
    )));
}

// The limit on memory

#[test]
fn only_the_thumbnails_nearest_to_the_window_are_kept() {
    // One thumbnail per row, and ten rows on screen: the user pages down
    // through 500 of the 1000 pages, and every page asked for is drawn.
    let mut state = open_document(1000);
    for first_row in (0..500).step_by(10) {
        state.show_rows(first_row..first_row + 10, 1, SIZE);
        draw_everything_asked_for(&mut state);
        assert!(state.kept() <= MAX_KEPT, "{} kept", state.kept());
    }

    // The window ends on rows 490 to 499: pages 491 to 500, and with the
    // margin 490 to 501. The 120 pages kept are the nearest to those.
    assert_eq!(state.kept(), MAX_KEPT);
    assert_eq!(
        pages_with_a_slot(&state, 1000),
        (382..=501).collect::<Vec<_>>()
    );
    // The images that the view never came to take belong to pages that
    // are still kept: nothing waits here for a page that was forgotten.
    let fresh = state.take_fresh();
    assert!(fresh.len() <= MAX_KEPT);
    assert!(fresh.iter().all(|(page, _)| *page >= 382));
}

#[test]
fn a_forgotten_thumbnail_is_asked_for_again_when_it_comes_back() {
    let mut state = open_document(1000);
    for first_row in (0..500).step_by(10) {
        state.show_rows(first_row..first_row + 10, 1, SIZE);
        draw_everything_asked_for(&mut state);
    }
    assert_eq!(state.slot(1), None);

    // Back to the top.
    state.show_rows(0..10, 1, SIZE);

    assert_eq!(
        draw_everything_asked_for(&mut state),
        (1..=11).collect::<Vec<_>>()
    );
    assert_eq!(state.slot(1), Some(&Slot::Ready));
    assert_eq!(state.kept(), MAX_KEPT);
}

#[test]
fn what_is_on_screen_is_never_forgotten() {
    // A very large window: 20 thumbnails per row and 10 rows, which with
    // the margin is 220 pages, more than the limit.
    let mut state = open_document(1000);

    state.show_rows(0..10, 20, SIZE);
    let asked = draw_everything_asked_for(&mut state);
    state.show_rows(0..10, 20, SIZE);

    // The limit gives way: forgetting a page that is on screen would only
    // have it drawn again, for ever.
    assert_eq!(asked.len(), 220);
    assert_eq!(state.kept(), 220);
    assert!(!state.has_requests());
}

#[test]
fn a_page_that_failed_counts_towards_the_limit_like_any_other() {
    let mut state = open_document(1000);
    for first_row in (0..300).step_by(10) {
        state.show_rows(first_row..first_row + 10, 1, SIZE);
        // The worker cannot draw any page of this file.
        for request in state.take_requests() {
            let Request::Page {
                generation, page, ..
            } = request
            else {
                panic!("expected a page request");
            };
            state.receive(Reply::Page {
                generation,
                page,
                result: Err(RenderError::Broken {
                    reason: "a page cannot be read",
                }),
            });
        }
    }

    assert_eq!(state.kept(), MAX_KEPT);
}

// The numbers the drawing code asks for

#[test]
fn a_thumbnail_is_160_points_wide_at_any_scale() {
    assert_eq!(thumbnail_size(1.0), SIZE);
    assert_eq!(
        thumbnail_size(2.0),
        ThumbnailSize {
            width: 320,
            max_height: 452
        }
    );
    assert_eq!(
        thumbnail_size(1.25),
        ThumbnailSize {
            width: 200,
            max_height: 283
        }
    );
    // A scale that makes no sense still gives a size that can be drawn.
    assert_eq!(
        thumbnail_size(1000.0),
        ThumbnailSize {
            width: MAX_RENDER_WIDTH,
            max_height: MAX_RENDER_HEIGHT
        }
    );
    for nonsense in [0.0, -1.0, f32::NAN] {
        assert_eq!(
            thumbnail_size(nonsense),
            ThumbnailSize {
                width: 1,
                max_height: 1
            }
        );
    }
}

#[test]
fn as_many_columns_as_fit_and_never_none() {
    // Cells 160 wide with 8 between them: three take 496, four take 664.
    assert_eq!(columns_for(495.0, 160.0, 8.0), 2);
    assert_eq!(columns_for(496.0, 160.0, 8.0), 3);
    assert_eq!(columns_for(663.0, 160.0, 8.0), 3);
    assert_eq!(columns_for(664.0, 160.0, 8.0), 4);
    // A window narrower than one thumbnail still shows one per row.
    assert_eq!(columns_for(100.0, 160.0, 8.0), 1);
    assert_eq!(columns_for(0.0, 160.0, 8.0), 1);
    assert_eq!(columns_for(f32::NAN, 160.0, 8.0), 1);
}

#[test]
fn rows_are_counted_with_the_last_one_that_is_not_full() {
    assert_eq!(row_count(12, 4), 3);
    assert_eq!(row_count(13, 4), 4);
    assert_eq!(row_count(1, 4), 1);
    assert_eq!(row_count(0, 4), 0);
    // No columns is treated as one, so nothing divides by zero.
    assert_eq!(row_count(5, 0), 5);
}

#[test]
fn an_image_is_fitted_into_the_box_keeping_its_shape() {
    // The same shape as the box: it fills it.
    assert_eq!(fit_within([160, 226], 160.0, 226.0), (160.0, 226.0));
    // Twice the pixels (a sharper screen): the same size on screen.
    assert_eq!(fit_within([320, 452], 160.0, 226.0), (160.0, 226.0));
    // A page on its side fills the width.
    assert_eq!(fit_within([160, 113], 160.0, 226.0), (160.0, 113.0));
    // A tall strip fills the height.
    assert_eq!(fit_within([22, 226], 160.0, 226.0), (22.0, 226.0));
    // A small image is made bigger: thumbnails are all the same size.
    assert_eq!(fit_within([80, 113], 160.0, 226.0), (160.0, 226.0));
    // An image without pixels takes no room and divides nothing by zero.
    assert_eq!(fit_within([0, 0], 160.0, 226.0), (0.0, 0.0));
}

// With a real worker thread
//
// The three tests below join the two halves: this state on one side, and
// a worker thread with a small fake engine on the other. They wait for
// the worker the way the window does, by being woken, and never by
// sleeping (see the tests of `thumbnail_worker.rs`).

/// The longest a test waits for the worker before it fails.
const GUARD: Duration = Duration::from_secs(30);

/// An engine for which every file has two pages, drawn as small images.
struct TwoPages;

impl PageEngine for TwoPages {
    fn page_count(&self, _path: &Path) -> Result<usize, RenderError> {
        Ok(2)
    }

    fn render_page(
        &self,
        _path: &Path,
        page: NonZeroUsize,
        _width: u32,
    ) -> Result<PageImage, RenderError> {
        Ok(image_of(page.get()))
    }
}

/// An engine that panics when it is asked anything, as a bug in the
/// library that binds to PDFium would.
struct Panics;

impl PageEngine for Panics {
    fn page_count(&self, _path: &Path) -> Result<usize, RenderError> {
        panic!("the engine broke (this panic is part of a test)");
    }

    fn render_page(
        &self,
        _path: &Path,
        _page: NonZeroUsize,
        _width: u32,
    ) -> Result<PageImage, RenderError> {
        panic!("the engine broke (this panic is part of a test)");
    }
}

#[test]
fn a_file_goes_from_chosen_to_drawn_through_a_worker_thread() {
    let (woken_sender, woken) = mpsc::channel();
    let mut state = Thumbnails::default();
    let worker = Worker::spawn(
        state.generation().clone(),
        || Ok(TwoPages),
        Box::new(move || {
            let _ = woken_sender.send(());
        }),
    )
    .unwrap();

    // Frame 1: the user chooses a file, and the request goes out.
    state.choose([doc("report.pdf")]);
    assert!(state.exchange(&worker));
    assert_eq!(
        state.document(),
        &Document::Opening {
            path: doc("report.pdf")
        }
    );

    // The worker wakes the window: the next frame finds the page count.
    woken.recv_timeout(GUARD).unwrap();
    assert!(state.exchange(&worker));
    assert_eq!(state.pages(), Some(2));

    // That frame draws the grid, which asks for the pages on screen.
    state.show_rows(0..1, 4, SIZE);
    assert!(state.exchange(&worker));
    assert_eq!(state.slot(1), Some(&Slot::Requested));

    // Two more wake-ups, one for each page.
    woken.recv_timeout(GUARD).unwrap();
    woken.recv_timeout(GUARD).unwrap();
    assert!(state.exchange(&worker));
    assert_eq!(state.slot(1), Some(&Slot::Ready));
    assert_eq!(state.slot(2), Some(&Slot::Ready));
    assert_eq!(state.take_fresh(), [(1, image_of(1)), (2, image_of(2))]);
}

#[test]
fn a_worker_that_died_is_noticed_and_the_view_says_so() {
    let mut state = open_document(10);
    state.show_rows(0..1, 4, SIZE);
    state.take_requests();
    let mut worker =
        Worker::spawn(state.generation().clone(), || Ok(Panics), Box::new(|| {})).unwrap();

    // The request makes the engine panic, which ends the worker thread.
    // (The panic prints its message in the output of this test.)
    state.choose([doc("other.pdf")]);
    assert!(state.exchange(&worker));
    assert!(worker.join().is_err());

    // The thread is gone, and with it its end of the channels.
    assert!(!state.exchange(&worker));

    assert_eq!(state.status(), &Status::WorkerStopped);
    assert_eq!(state.document(), &Document::None);
    assert_eq!(state.kept(), 0);
    assert!(!state.has_requests());
}

#[test]
fn after_a_worker_died_a_new_one_serves_the_next_file() {
    let (woken_sender, woken) = mpsc::channel();
    let mut state = Thumbnails::default();
    let mut dead =
        Worker::spawn(state.generation().clone(), || Ok(Panics), Box::new(|| {})).unwrap();
    state.choose([doc("report.pdf")]);
    assert!(state.exchange(&dead));
    assert!(dead.join().is_err());
    assert!(!state.exchange(&dead));
    drop(dead);

    // The window starts another worker when there is something to send.
    let worker = Worker::spawn(
        state.generation().clone(),
        || Ok(TwoPages),
        Box::new(move || {
            let _ = woken_sender.send(());
        }),
    )
    .unwrap();
    state.choose([doc("report.pdf")]);
    assert!(state.exchange(&worker));
    woken.recv_timeout(GUARD).unwrap();
    assert!(state.exchange(&worker));

    assert_eq!(state.pages(), Some(2));
    assert_eq!(state.status(), &Status::Idle);
}

#[test]
fn a_worker_that_cannot_start_is_a_status_and_not_a_crash() {
    let mut state = Thumbnails::default();
    state.choose([doc("report.pdf")]);
    let error = io::Error::other("no more threads");

    state.worker_did_not_start(&error);

    assert_eq!(
        state.status(),
        &Status::WorkerNotStarted {
            message: "no more threads".to_owned()
        }
    );
    assert_eq!(state.document(), &Document::None);
    // The request is dropped, so the window does not try again on every
    // frame: the next attempt is the next file the user chooses.
    assert!(!state.has_requests());
}
