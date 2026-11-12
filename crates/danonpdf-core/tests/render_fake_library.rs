//! A file that has the name of the PDFium library and is not one.
//!
//! The system is only asked to load a library while the process has none:
//! once PDFium is loaded, another file is refused before the system sees
//! it. So this needs a process of its own, which a file in `tests/` is (see
//! `common/mod.rs`).

mod common;

use std::fs;

use danonpdf_core::{RenderError, Renderer, error_chain};

use common::{ScratchDir, missing_library_path, real_library_folder};

#[test]
fn a_file_that_is_not_a_library_fails_to_load_and_spoils_nothing() {
    let scratch = ScratchDir::new("fake");
    let fake = missing_library_path(scratch.path());
    fs::write(&fake, "this is not a library").unwrap();

    let result = Renderer::open(scratch.path());

    match result {
        Err(error @ RenderError::LibraryFailed { .. }) => {
            // What the system said about the file follows the path, so the
            // whole message is longer than its first sentence. Shown with
            // `cargo test -- --nocapture`.
            let message = error_chain(&error);
            println!("{message}");
            assert!(
                message.len() > error.to_string().len(),
                "the reason is missing: {message}"
            );
            let RenderError::LibraryFailed { path, .. } = error else {
                unreachable!()
            };
            assert_eq!(path, fake);
        }
        other => panic!("expected a library that fails to load, got {other:?}"),
    }

    // A load that failed has not used up the one load of the process: the
    // real library still loads afterwards.
    let renderer = Renderer::open(&real_library_folder()).unwrap();
    assert!(
        renderer.library().is_file(),
        "{}",
        renderer.library().display()
    );
}
