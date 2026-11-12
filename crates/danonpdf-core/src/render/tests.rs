//! Tests for drawing pages. Every PDF they use is built with lopdf by the
//! helpers in `test_support.rs`, and drawn by the real PDFium library: the
//! build image unpacks it and points `DANONPDF_PDFIUM_DIR` at it.
//!
//! A process can load PDFium once, and `cargo test` runs every test of this
//! crate as threads of one process. So the tests share one engine through
//! [`renderer`], which is what `Renderer::open` gives when it is asked for
//! the same folder again.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::num::NonZeroUsize;
use std::path::Path;
use std::thread;

use lopdf::{Document, Object, dictionary};

use super::*;
use crate::test_support::{
    ScratchDir, encrypted, root_pages_id, with_blank_page, with_pages, with_red_bottom_half,
};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

/// The renderer every test draws with.
fn renderer() -> Renderer {
    let folder = default_library_folder().unwrap();
    Renderer::open(&folder).unwrap_or_else(|error| {
        panic!(
            "these tests need the PDFium library; the build image sets {PDFIUM_DIR_VARIABLE} \
             to its folder: {}",
            crate::error_chain(&error)
        )
    })
}

/// Page number `number`, counted from 1.
fn page(number: usize) -> NonZeroUsize {
    NonZeroUsize::new(number).expect("tests ask for page 1 or above")
}

/// The pixels of `image`, as four bytes each instead of one long row of
/// bytes. `as_chunks` only changes how the same memory is looked at; it
/// also returns the bytes left over at the end, and there must be none.
fn pixels(image: &PageImage) -> &[[u8; 4]] {
    let (pixels, rest) = image.rgba.as_chunks::<4>();
    assert!(rest.is_empty(), "the image does not end on a whole pixel");
    pixels
}

/// The pixel in column `x` and row `y`, counted from the top left corner.
fn pixel(image: &PageImage, x: u32, y: u32) -> [u8; 4] {
    pixels(image)[y as usize * image.width as usize + x as usize]
}

/// Changes the dictionary of every page of `document` with `change`.
fn change_pages(document: &mut Document, change: impl Fn(&mut lopdf::Dictionary)) {
    let page_ids: Vec<_> = document.page_iter().collect();
    for page_id in page_ids {
        let page = document.get_object_mut(page_id).unwrap();
        change(page.as_dict_mut().unwrap());
    }
}

/// Saves `document` into `scratch` as `cut.pdf` without its end, and
/// returns the path: everything from the table that says where each object
/// is (the cross-reference data) is left out, so the table, the trailer
/// and the pointer to the table are all gone.
///
/// The cut is read from the file itself and not counted in bytes from the
/// end. A PDF ends with the keyword `startxref`, the position of the table
/// as a number, and `%%EOF`; that number is where the cut goes. A fixture
/// with one more page, or a version of lopdf that writes the table in
/// another form, moves the table, and the tests go on cutting exactly
/// this.
fn cut_short(scratch: &ScratchDir, document: Document) -> PathBuf {
    const KEYWORD: &[u8] = b"startxref";
    let whole = scratch.save("whole.pdf", document);
    let bytes = fs::read(&whole).unwrap();
    // `windows` slides over the bytes, `KEYWORD.len()` at a time, and
    // `rposition` searches from the end: where the last keyword starts.
    let keyword = bytes
        .windows(KEYWORD.len())
        .rposition(|window| window == KEYWORD)
        .expect("the fixture ends with startxref");
    // The number is plain text: it is read as text and parsed.
    let after = String::from_utf8_lossy(&bytes[keyword + KEYWORD.len()..]);
    let table: usize = after
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
        .expect("a position follows startxref");
    assert!(table < keyword, "the table should come before its pointer");

    let path = scratch.file("cut.pdf");
    fs::write(&path, &bytes[..table]).unwrap();
    path
}

/// Reads a PNG file back: its width, its height and its RGBA bytes.
fn decode_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert_eq!(frame.bit_depth, png::BitDepth::Eight);
    pixels.truncate(frame.buffer_size());
    (frame.width, frame.height, pixels)
}

// --- The size of the image -------------------------------------------------

#[test]
fn an_a4_page_keeps_its_proportions() {
    let scratch = ScratchDir::new("render-a4");
    let input = scratch.save("a.pdf", with_pages("A", 1));

    let image = renderer().render_page(&input, page(1), 100).unwrap();

    // A4 is 595 by 842 points: 100 * 842 / 595 = 141.5, rounded to 142.
    assert_eq!((image.width, image.height), (100, 142));
    assert_eq!(image.rgba.len(), 100 * 142 * 4);
}

#[test]
fn the_height_follows_the_page_within_its_limits() {
    // The page sizes are in points, as PDFium reports them.
    assert_eq!(image_size(595.0, 842.0, 100), Some((100, 142)));
    assert_eq!(image_size(842.0, 595.0, 100), Some((100, 71)));
    assert_eq!(
        image_size(595.0, 842.0, MAX_RENDER_WIDTH),
        Some((4096, 5796))
    );
    // So wide that the height would round to no pixels at all: one row.
    assert_eq!(image_size(2000.0, 10.0, 50), Some((50, 1)));
    // So tall that the height would pass the limit: the whole page is
    // made smaller, so the image is narrower than what was asked for.
    assert_eq!(image_size(10.0, 2000.0, 100), Some((41, MAX_RENDER_HEIGHT)));
    assert_eq!(
        image_size(1.0, 100_000.0, 100),
        Some((1, MAX_RENDER_HEIGHT))
    );
}

#[test]
fn a_page_without_a_usable_size_has_no_image_size() {
    assert_eq!(image_size(0.0, 842.0, 100), None);
    assert_eq!(image_size(595.0, 0.0, 100), None);
    assert_eq!(image_size(-595.0, 842.0, 100), None);
    assert_eq!(image_size(f32::NAN, 842.0, 100), None);
    assert_eq!(image_size(595.0, f32::INFINITY, 100), None);
}

#[test]
fn a_very_tall_page_is_drawn_narrower_than_asked() {
    let scratch = ScratchDir::new("render-tall");
    let input = scratch.save("tall.pdf", with_blank_page(10, 2000));

    let image = renderer().render_page(&input, page(1), 100).unwrap();

    assert_eq!((image.width, image.height), (41, MAX_RENDER_HEIGHT));
    assert_eq!(image.rgba.len(), 41 * MAX_RENDER_HEIGHT as usize * 4);
}

#[test]
fn a_very_wide_page_still_gets_one_row() {
    let scratch = ScratchDir::new("render-wide");
    let input = scratch.save("wide.pdf", with_blank_page(2000, 10));

    let image = renderer().render_page(&input, page(1), 50).unwrap();

    assert_eq!((image.width, image.height), (50, 1));
}

#[test]
fn a_rotated_page_is_drawn_rotated() {
    let scratch = ScratchDir::new("render-rotated");
    let mut document = with_pages("A", 1);
    change_pages(&mut document, |page| page.set("Rotate", 90));
    let input = scratch.save("rotated.pdf", document);

    let image = renderer().render_page(&input, page(1), 100).unwrap();

    // An A4 page on its side: 100 * 595 / 842 = 70.7, rounded to 71.
    assert_eq!((image.width, image.height), (100, 71));
}

#[test]
fn a_page_without_a_size_is_drawn_as_us_letter() {
    // The size of a page is its /MediaBox. PDFium does not refuse a page
    // that has none: it takes 612 by 792 points, the US Letter sheet.
    let scratch = ScratchDir::new("render-no-size");
    let mut document = with_pages("A", 1);
    change_pages(&mut document, |page| {
        page.remove(b"MediaBox");
    });
    let input = scratch.save("no-size.pdf", document);

    let image = renderer().render_page(&input, page(1), 100).unwrap();

    // 100 * 792 / 612 = 129.4, rounded to 129.
    assert_eq!((image.width, image.height), (100, 129));
}

#[test]
fn a_width_of_zero_or_above_the_maximum_is_refused() {
    let scratch = ScratchDir::new("render-width");
    let input = scratch.save("a.pdf", with_pages("A", 1));

    for width in [0, MAX_RENDER_WIDTH + 1, u32::MAX] {
        let result = renderer().render_page(&input, page(1), width);

        match result {
            Err(RenderError::WidthOutOfRange {
                width: refused,
                maximum,
            }) => {
                assert_eq!((refused, maximum), (width, MAX_RENDER_WIDTH));
            }
            other => panic!("expected a width error for {width}, got {other:?}"),
        }
    }
}

// --- What is in the image --------------------------------------------------

#[test]
fn the_marker_is_drawn_dark_on_a_white_opaque_page() {
    let scratch = ScratchDir::new("render-marker");
    let input = scratch.save("a.pdf", with_pages("A", 1));

    let image = renderer().render_page(&input, page(1), 200).unwrap();

    // The four corners are paper.
    let (right, bottom) = (image.width - 1, image.height - 1);
    for (x, y) in [(0, 0), (right, 0), (0, bottom), (right, bottom)] {
        assert_eq!(pixel(&image, x, y), WHITE, "corner {x},{y}");
    }
    // The marker "A1" is black text: somewhere a pixel is dark.
    let dark = pixels(&image)
        .iter()
        .filter(|[red, green, blue, _]| *red < 96 && *green < 96 && *blue < 96)
        .count();
    assert!(dark > 0, "no dark pixel: the text was not drawn");
    // The page is drawn on white paper, so nothing is see-through.
    assert!(
        pixels(&image).iter().all(|[.., alpha]| *alpha == 255),
        "a pixel is not opaque"
    );
}

#[test]
fn the_image_is_the_right_way_up_and_red_is_red() {
    let scratch = ScratchDir::new("render-red");
    let input = scratch.save("red.pdf", with_red_bottom_half());

    let image = renderer().render_page(&input, page(1), 100).unwrap();

    // Row 0 is the top of the page, and the first byte of a pixel is red.
    let (right, bottom) = (image.width - 1, image.height - 1);
    assert_eq!(pixel(&image, 0, 0), WHITE);
    assert_eq!(pixel(&image, right, 0), WHITE);
    assert_eq!(pixel(&image, 0, bottom), RED);
    assert_eq!(pixel(&image, right, bottom), RED);
}

#[test]
fn each_page_gives_its_own_image_and_the_same_one_every_time() {
    let scratch = ScratchDir::new("render-pages");
    let input = scratch.save("a.pdf", with_pages("A", 3));
    let renderer = renderer();

    let first = renderer.render_page(&input, page(1), 200).unwrap();
    let second = renderer.render_page(&input, page(2), 200).unwrap();
    let second_again = renderer.render_page(&input, page(2), 200).unwrap();

    assert_eq!((first.width, first.height), (second.width, second.height));
    assert!(first != second, "pages 1 and 2 gave the same image");
    assert!(second == second_again, "page 2 gave two different images");
}

#[test]
fn threads_can_draw_at_the_same_time() {
    let scratch = ScratchDir::new("render-threads");
    let input = scratch.save("a.pdf", with_pages("A", 4));
    let renderer = renderer();
    let alone: Vec<PageImage> = (1..=4)
        .map(|number| renderer.render_page(&input, page(number), 120).unwrap())
        .collect();

    // `thread::scope` lets the threads borrow `input` and `renderer`: it
    // does not return until all of them have finished, so what they borrow
    // is certain to outlive them.
    let together: Vec<PageImage> = thread::scope(|scope| {
        let workers: Vec<_> = (1..=4)
            .map(|number| {
                let input = &input;
                scope.spawn(move || renderer.render_page(input, page(number), 120).unwrap())
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect()
    });

    assert!(
        alone == together,
        "a page drawn in a thread came out different"
    );
}

#[test]
fn what_crosses_threads_is_send_and_sync() {
    // Nothing runs here: the test is that this compiles. A worker thread
    // holds a `Renderer` and sends back a `PageImage` or a `RenderError`.
    fn crosses_threads<T: Send + Sync>() {}
    crosses_threads::<Renderer>();
    crosses_threads::<PageImage>();
    crosses_threads::<RenderError>();
}

// --- Files that cannot be drawn ---------------------------------------------

#[test]
fn a_page_past_the_end_is_refused_with_the_page_count() {
    let scratch = ScratchDir::new("render-past-end");
    let input = scratch.save("a.pdf", with_pages("A", 3));

    let result = renderer().render_page(&input, page(4), 100);

    match result {
        Err(RenderError::PageOutOfRange { page, pages }) => assert_eq!((page, pages), (4, 3)),
        other => panic!("expected a page out of range, got {other:?}"),
    }
}

#[test]
fn a_file_that_does_not_exist_cannot_be_read() {
    let scratch = ScratchDir::new("render-no-file");
    let input = scratch.file("nothing.pdf");

    let result = renderer().render_page(&input, page(1), 100);

    match result {
        Err(RenderError::Read { path, source }) => {
            assert_eq!(path, input);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("expected a read error, got {other:?}"),
    }
}

#[test]
fn a_file_that_is_not_a_pdf_is_refused() {
    let scratch = ScratchDir::new("render-not-pdf");
    let text = scratch.file("notes.pdf");
    fs::write(&text, "just some text, not a PDF\n").unwrap();
    let empty = scratch.file("empty.pdf");
    fs::write(&empty, "").unwrap();

    for input in [text, empty] {
        let result = renderer().render_page(&input, page(1), 100);

        match result {
            Err(RenderError::NotPdf { path }) => assert_eq!(path, input),
            other => panic!("expected not a PDF for {}, got {other:?}", input.display()),
        }
    }
}

#[test]
fn a_file_that_needs_a_password_is_refused() {
    let scratch = ScratchDir::new("render-locked");
    let input = scratch.save("locked.pdf", encrypted(with_pages("A", 1), "secret"));

    let result = renderer().render_page(&input, page(1), 100);

    assert!(
        matches!(result, Err(RenderError::Encrypted)),
        "expected an encrypted error, got {result:?}"
    );
}

#[test]
fn an_encrypted_file_with_an_empty_password_is_drawn() {
    // Such a file is encrypted (here with RC4, 128 bits) and opens for
    // anyone: its user password is the empty one. PDFium tries that
    // password by itself, as the rest of the core does through lopdf.
    let scratch = ScratchDir::new("render-empty-password");
    let plain = scratch.save("plain.pdf", with_pages("A", 1));
    let input = scratch.save("open.pdf", encrypted(with_pages("A", 1), ""));
    let renderer = renderer();

    let image = renderer.render_page(&input, page(1), 200).unwrap();

    // Decrypted for real, not drawn as an empty page: it is the image of
    // the same page saved without encryption.
    let expected = renderer.render_page(&plain, page(1), 200).unwrap();
    assert!(
        image == expected,
        "the decrypted page is not the plain page"
    );
}

#[test]
fn a_document_without_pages_has_no_page_1() {
    let scratch = ScratchDir::new("render-no-pages");
    let input = scratch.save("none.pdf", with_pages("A", 0));

    let result = renderer().render_page(&input, page(1), 100);

    match result {
        Err(RenderError::PageOutOfRange { page, pages }) => assert_eq!((page, pages), (1, 0)),
        other => panic!("expected a page out of range, got {other:?}"),
    }
}

#[test]
fn a_page_that_is_missing_from_the_file_is_damage() {
    // The page tree lists two pages, and neither object exists.
    let scratch = ScratchDir::new("render-missing-page");
    let mut document = with_pages("A", 2);
    let pages_id = root_pages_id(&document);
    let missing = vec![Object::Reference((998, 0)), Object::Reference((997, 0))];
    document
        .get_object_mut(pages_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Kids", missing);
    let input = scratch.save("missing-page.pdf", document);

    let result = renderer().render_page(&input, page(1), 100);

    match result {
        Err(RenderError::Broken { reason }) => assert_eq!(reason, "a page cannot be read"),
        other => panic!("expected a broken file, got {other:?}"),
    }
}

// --- Where PDFium and the rest of the core disagree -------------------------
//
// Drawing reads the file with PDFium; counting pages, merging and splitting
// read it with lopdf. These tests pin down the differences that were found,
// so a change in either library that moves one of them is noticed.

#[test]
fn a_file_cut_short_can_be_counted_and_cannot_be_drawn() {
    // The end of the file holds the table that says where each object is.
    // lopdf finds the objects without it; PDFium gives up.
    let scratch = ScratchDir::new("render-cut-short");
    let input = cut_short(&scratch, with_pages("A", 2));

    let counted = crate::page_count(&input);
    let drawn = renderer().render_page(&input, page(1), 100);

    assert_eq!(counted.unwrap(), 2);
    match drawn {
        Err(RenderError::NotPdf { path }) => assert_eq!(path, input),
        other => panic!("expected not a PDF, got {other:?}"),
    }
}

#[test]
fn a_catalog_without_a_page_tree_is_damage_to_count_and_no_pages_to_draw() {
    let scratch = ScratchDir::new("render-no-page-tree");
    let mut document = with_pages("A", 1);
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let bare_catalog = Object::Dictionary(dictionary! { "Type" => "Catalog" });
    document.objects.insert(catalog_id, bare_catalog);
    let input = scratch.save("no-page-tree.pdf", document);

    let counted = crate::page_count(&input);
    let drawn = renderer().render_page(&input, page(1), 100);

    assert!(
        matches!(counted, Err(crate::PdfError::Broken { .. })),
        "expected a broken file, got {counted:?}"
    );
    match drawn {
        Err(RenderError::PageOutOfRange { page, pages }) => assert_eq!((page, pages), (1, 0)),
        other => panic!("expected a page out of range, got {other:?}"),
    }
}

// --- Finding the library ----------------------------------------------------

#[test]
fn the_variable_names_the_library_folder_when_it_is_set() {
    let folder = library_folder_from(
        Some(OsString::from("/opt/pdfium/lib")),
        Ok(PathBuf::from("/app/bin/danonpdf")),
    );

    assert_eq!(folder.unwrap(), Path::new("/opt/pdfium/lib"));
}

#[test]
fn the_library_folder_is_the_folder_of_the_program_otherwise() {
    // Not set, and set to nothing: both mean "no override".
    for variable in [None, Some(OsString::new())] {
        let folder = library_folder_from(variable, Ok(PathBuf::from("/app/bin/danonpdf")));

        assert_eq!(folder.unwrap(), Path::new("/app/bin"));
    }
}

#[test]
fn a_program_that_cannot_find_itself_has_no_library_folder() {
    let lost = io::Error::new(io::ErrorKind::NotFound, "no such process");

    let result = library_folder_from(None, Err(lost));

    assert!(
        matches!(result, Err(RenderError::LibraryFolderUnknown { .. })),
        "expected an unknown folder, got {result:?}"
    );
}

#[test]
fn a_folder_without_the_library_is_an_error_and_not_a_panic() {
    let scratch = ScratchDir::new("render-no-library");

    // Asked before and after the real library is loaded: the answer does
    // not depend on which test ran first.
    let before = Renderer::open(scratch.path());
    renderer();
    let after = Renderer::open(scratch.path());

    for result in [before, after] {
        match result {
            Err(RenderError::LibraryMissing { path }) => {
                assert_eq!(path.parent(), Some(scratch.path()));
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                assert!(name.contains("pdfium"), "unexpected library name {name}");
            }
            other => panic!("expected a missing library, got {other:?}"),
        }
    }
}

#[test]
fn a_second_library_cannot_replace_the_one_that_is_loaded() {
    let scratch = ScratchDir::new("render-second-library");
    let loaded = pdfium::library_path(&default_library_folder().unwrap());
    // A copy is the same library in another file.
    let copy = pdfium::library_path(scratch.path());
    fs::copy(&loaded, &copy).unwrap();
    renderer();

    let result = Renderer::open(scratch.path());

    match result {
        Err(RenderError::LibraryAlreadyLoaded { asked, .. }) => assert_eq!(asked, copy),
        other => panic!("expected an already loaded library, got {other:?}"),
    }
}

#[test]
fn the_same_folder_spelled_differently_is_the_same_library() {
    let folder = default_library_folder().unwrap();
    renderer();

    let result = Renderer::open(&folder.join("."));

    assert!(
        result.is_ok(),
        "expected the loaded library, got {result:?}"
    );
}

#[test]
fn the_same_folder_asked_twice_gives_the_same_library() {
    let folder = default_library_folder().unwrap();

    let first = Renderer::open(&folder).unwrap();
    let second = Renderer::open(&folder).unwrap();

    assert_eq!(first.library(), second.library());
    // Both are handles to the one engine of the process, not two engines
    // that happen to come from one file. `ptr::eq` compares the two
    // addresses, not what is stored there.
    assert!(std::ptr::eq(first.engine, second.engine));
}

#[test]
fn the_loaded_library_is_known_by_its_absolute_path() {
    // Two lines and not `renderer().library()`: the path is borrowed from
    // the renderer, which therefore needs a name to live under until the
    // last use of the path. Without one it is a temporary, dropped at the
    // end of its own statement (E0716).
    let renderer = renderer();
    let library = renderer.library();

    assert!(library.is_absolute(), "{}", library.display());
    assert!(library.is_file(), "{}", library.display());
}

// Three cases are not here, because each needs a process of its own, and
// every test in this file shares one process and one engine. They are the
// programs in `tests/` of this crate:
//
// - `render_fake_library.rs`: a file that is not a library fails to load.
//   It can only be seen before PDFium is loaded: afterwards, another file
//   is refused before the system is asked to load anything (see
//   `a_second_library_cannot_replace_the_one_that_is_loaded` above).
// - `render_relative_folder.rs`: the folder given as a relative path, and
//   the process moving to another folder afterwards.
// - `render_removed_library.rs`: the library file deleted after loading.

// --- Writing a PNG -----------------------------------------------------------

#[test]
fn a_png_holds_the_same_pixels_and_no_temporary_file_is_left() {
    let scratch = ScratchDir::new("render-png");
    let input = scratch.save("a.pdf", with_pages("A", 1));
    let output = scratch.file("page.png");
    let image = renderer().render_page(&input, page(1), 120).unwrap();

    write_png(&image, &output).unwrap();

    let bytes = fs::read(&output).unwrap();
    // Every PNG file starts with these eight bytes.
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let (width, height, pixels) = decode_png(&output);
    assert_eq!((width, height), (image.width, image.height));
    assert!(
        pixels == image.rgba,
        "the PNG does not hold the same pixels"
    );
    assert_eq!(scratch.entries(), ["a.pdf", "page.png"]);
}

#[test]
fn a_png_replaces_the_file_that_was_there() {
    // Whether a file may be replaced is decided by the front end, before
    // it calls the core: this layer writes where it is told, as a merge
    // does.
    let scratch = ScratchDir::new("render-png-replace");
    let output = scratch.file("page.png");
    fs::write(&output, "what was here before").unwrap();
    let image = PageImage {
        width: 2,
        height: 1,
        rgba: [RED, WHITE].concat(),
    };

    write_png(&image, &output).unwrap();

    assert_eq!(decode_png(&output), (2, 1, [RED, WHITE].concat()));
    assert_eq!(scratch.entries(), ["page.png"]);
}

#[test]
fn a_png_that_cannot_be_written_names_the_path_and_leaves_nothing_behind() {
    let scratch = ScratchDir::new("render-png-failed");
    // The folder the file should go into does not exist.
    let output = scratch.file("missing").join("page.png");
    let image = PageImage {
        width: 1,
        height: 1,
        rgba: WHITE.to_vec(),
    };

    let result = write_png(&image, &output);

    match result {
        Err(RenderError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), Vec::<String>::new());
}

#[test]
fn an_image_whose_bytes_do_not_match_its_size_is_not_written() {
    // The fields are public, so nothing stops a caller from building an
    // image that contradicts itself. It must not become a file.
    let scratch = ScratchDir::new("render-png-mismatch");
    let output = scratch.file("page.png");
    let image = PageImage {
        width: 2,
        height: 2,
        rgba: WHITE.to_vec(),
    };

    let result = write_png(&image, &output);

    match result {
        Err(RenderError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), Vec::<String>::new());
}

#[test]
fn printing_an_image_for_debugging_does_not_print_its_pixels() {
    let image = PageImage {
        width: 1,
        height: 1,
        rgba: WHITE.to_vec(),
    };

    assert_eq!(
        format!("{image:?}"),
        "PageImage { width: 1, height: 1, rgba: 4 bytes }"
    );
}
