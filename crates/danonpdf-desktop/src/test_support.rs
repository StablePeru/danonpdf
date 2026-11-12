//! Helpers for the tests of this crate that need real PDF files on disk.
//!
//! The module only exists in test builds (`#[cfg(test)]` on its `mod` line
//! in `main.rs`), so none of this reaches the program that ships. The core
//! has helpers like these for its own tests, but a crate's test helpers are
//! private to it, so each crate builds its PDFs itself.

use std::fs;
use std::path::{Path, PathBuf};

use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, dictionary};

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart; the
        // test name keeps apart the tests of one run, which share a process.
        let name = format!("danonpdf-desktop-{}-{test_name}", std::process::id());
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

    /// The path of `name` inside this folder. Nothing is created.
    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Saves a PDF with `page_count` pages into this folder as `name` and
    /// returns its path.
    pub fn pdf(&self, name: &str, page_count: i64) -> PathBuf {
        let path = self.file(name);
        with_pages(page_count).save(&path).unwrap();
        path
    }

    /// Saves a one-page PDF that needs the password "secret" into this
    /// folder as `name` and returns its path.
    pub fn locked_pdf(&self, name: &str) -> PathBuf {
        let path = self.file(name);
        encrypted(with_pages(1), "secret").save(&path).unwrap();
        path
    }

    /// The names of everything in this folder, sorted, so a test can check
    /// that an operation that was refused wrote nothing at all.
    pub fn entries(&self) -> Vec<String> {
        entries_of(&self.0)
    }
}

// `Drop` is the trait Rust calls when a value goes away, here at the end of
// the test, whether it passed or panicked.
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The names of everything in `folder`, sorted.
pub fn entries_of(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A document with `page_count` empty pages. Page `n` is `500 + n` points
/// wide, so a test can tell which pages ended up where (see
/// [`page_widths`]).
fn with_pages(page_count: i64) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let kids: Vec<Object> = (1..=page_count)
        .map(|number| {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), (500 + number).into(), 842.into()],
            });
            Object::Reference(page_id)
        })
        .collect();
    let pages = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => page_count };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document
}

/// Encrypts `document` with RC4 128 bits and the given user password.
fn encrypted(mut document: Document, user_password: &str) -> Document {
    // Encryption keys are derived from the file identifier, so set one.
    let id = Object::string_literal("0123456789abcdef");
    document.trailer.set("ID", vec![id.clone(), id]);
    let version = EncryptionVersion::V2 {
        document: &document,
        owner_password: "owner",
        user_password,
        key_length: 128,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).unwrap();
    document.encrypt(&state).unwrap();
    document
}

/// The width of every page of the PDF at `path`, in page order. For a file
/// made by [`ScratchDir::pdf`], `[503, 501]` means: page 3 of the input,
/// then page 1.
pub fn page_widths(path: &Path) -> Vec<i64> {
    let document = Document::load(path).unwrap();
    document
        .page_iter()
        .map(|page_id| {
            let page = document.get_dictionary(page_id).unwrap();
            let media_box = page.get(b"MediaBox").unwrap().as_array().unwrap();
            media_box[2].as_i64().unwrap()
        })
        .collect()
}
