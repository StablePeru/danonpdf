//! What the test programs in this folder share.
//!
//! Every file directly in `tests/` is built as a program of its own, and a
//! process can load PDFium once. That is why these tests are not with the
//! others in `src/render/tests.rs`, which all run as threads of one process
//! and share one engine: each test here needs a process in which nothing
//! has loaded PDFium yet, and decides by itself what is loaded first.
//!
//! This file is in a folder (`common/mod.rs`) so that Cargo does not take
//! it for one more test program. Each test file pulls it in with
//! `mod common;`.
//!
//! These programs see the core from the outside, as a front end does: only
//! what `lib.rs` makes public.

use std::fs;
use std::path::{Path, PathBuf};

use danonpdf_core::{RenderError, Renderer, default_library_folder};

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart.
        let name = format!("danonpdf-library-{}-{test_name}", std::process::id());
        let path = std::env::temp_dir().join(name);
        // Left over from an earlier run that was killed; no error if not.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The folder itself.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The folder of the real PDFium library: the build image unpacks it and
/// names its folder in `DANONPDF_PDFIUM_DIR`.
pub fn real_library_folder() -> PathBuf {
    default_library_folder().expect("the folder of the PDFium library should be known")
}

/// Asks for the engine from `folder`, which must hold no library, and
/// returns the path the core looked at.
///
/// The name of the library file depends on the system (`libpdfium.so`,
/// `pdfium.dll`), and only the core knows it. A missing library is reported
/// with its whole path, so asking an empty folder is how a test learns
/// where to put a file of its own.
pub fn missing_library_path(folder: &Path) -> PathBuf {
    match Renderer::open(folder) {
        Err(RenderError::LibraryMissing { path }) => path,
        other => panic!(
            "expected a missing library in {}, got {other:?}",
            folder.display()
        ),
    }
}
