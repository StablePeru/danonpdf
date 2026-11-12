//! The library file disappearing after it was loaded.
//!
//! It needs a process whose engine comes from a file the test may delete,
//! so it is a program of its own (see `common/mod.rs`).
//!
//! Unix only: there a file can be deleted while a process has it loaded
//! (the name goes, the content stays until the last user lets go). Windows
//! refuses to delete a DLL that is loaded, so the case cannot be set up.
#![cfg(unix)]

mod common;

use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;

use danonpdf_core::{RenderError, Renderer};
use lopdf::{Document, Object, dictionary};

use common::{ScratchDir, missing_library_path, real_library_folder};

/// Saves a PDF at `path` with one empty page, 200 by 100 points.
fn save_blank_pdf(path: &Path) {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 100.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1,
    };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save(path).unwrap();
}

#[test]
fn the_engine_outlives_its_file() {
    let real = real_library_folder();
    let scratch = ScratchDir::new("removed");
    // A copy of the real library, loaded from a folder of the test's own.
    let copy = missing_library_path(scratch.path());
    let name = copy.file_name().unwrap();
    fs::copy(real.join(name), &copy).unwrap();
    let first = Renderer::open(scratch.path()).unwrap();
    assert_eq!(first.library(), copy);

    fs::remove_file(&copy).unwrap();

    // The same folder gives the same engine: PDFium is in memory, and the
    // file is not read again.
    let again = Renderer::open(scratch.path());
    let again = again.unwrap_or_else(|error| panic!("expected the loaded library, got {error:?}"));
    assert_eq!(again.library(), copy);
    // And that engine still draws.
    let input = scratch.path().join("blank.pdf");
    save_blank_pdf(&input);
    let image = again.render_page(&input, NonZeroUsize::MIN, 50).unwrap();
    assert_eq!((image.width, image.height), (50, 25));

    // Every other folder is judged by what it holds, as before: nothing in
    // one, another library file in the other.
    let empty = ScratchDir::new("removed-empty");
    missing_library_path(empty.path());
    match Renderer::open(&real) {
        Err(RenderError::LibraryAlreadyLoaded { loaded, asked }) => {
            assert_eq!(loaded, copy);
            assert_eq!(asked, real.join(name));
        }
        other => panic!("expected an already loaded library, got {other:?}"),
    }
}
