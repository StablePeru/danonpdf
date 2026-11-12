//! Helpers shared by the tests of this crate. Every PDF the tests use is
//! built here with lopdf, so they need no fixture files and say exactly what
//! each input contains.
//!
//! The module only exists in test builds (`#[cfg(test)]` on its `mod` line
//! in `lib.rs`), so none of this reaches the library that ships.

use std::fs;
use std::path::{Path, PathBuf};

use lopdf::content::{Content, Operation};
use lopdf::{
    Dictionary, Document, EncryptionState, EncryptionVersion, Object, ObjectId, Permissions,
    Stream, dictionary,
};

/// An A4 page box: `[0 0 595 842]`.
pub(crate) fn a4() -> Object {
    vec![0.into(), 0.into(), 595.into(), 842.into()].into()
}

/// A content stream that writes `marker` (for example "A1") on the page.
pub(crate) fn marker_stream(marker: &str) -> Stream {
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal(marker)]),
            Operation::new("ET", vec![]),
        ],
    };
    Stream::new(Dictionary::new(), content.encode().unwrap())
}

/// A document with `page_count` pages. Page `n` shows the text
/// `{label}{n}`, so `with_pages("A", 2)` has pages "A1" and "A2".
pub(crate) fn with_pages(label: &str, page_count: i64) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let mut kids = Vec::new();
    for number in 1..=page_count {
        let content_id = document.add_object(marker_stream(&format!("{label}{number}")));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => a4(),
        });
        kids.push(Object::Reference(page_id));
    }

    let pages = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => page_count };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document
}

/// A one-page document whose page has no `/MediaBox` and no `/Resources`
/// of its own: it inherits them through a two-level page tree.
///
/// ```text
/// Catalog -> root Pages (Resources) -> inner Pages (MediaBox 300x500) -> page "I1"
/// ```
pub(crate) fn with_inherited_attributes() -> Document {
    let mut document = Document::with_version("1.5");
    let root_id = document.new_object_id();
    let inner_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Courier",
    });
    let content_id = document.add_object(marker_stream("I1"));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => inner_id,
        "Contents" => content_id,
    });
    document.objects.insert(
        inner_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Parent" => root_id,
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 500.into()],
        }),
    );
    document.objects.insert(
        root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(inner_id)],
            "Count" => 1,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_id });
    document.trailer.set("Root", catalog_id);
    document
}

/// The marker text of every page, in page order.
pub(crate) fn markers(document: &Document) -> Vec<String> {
    document
        .page_iter()
        .map(|page_id| {
            let content = Content::decode(&document.get_page_content(page_id)).unwrap();
            let text = content
                .operations
                .iter()
                .find(|operation| operation.operator == "Tj")
                .and_then(|operation| operation.operands.first())
                .and_then(|operand| operand.as_str().ok())
                .expect("every test page draws one marker");
            String::from_utf8_lossy(text).into_owned()
        })
        .collect()
}

/// The id of the root of the page tree, read from the catalog.
pub(crate) fn root_pages_id(document: &Document) -> ObjectId {
    document
        .catalog()
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference)
        .unwrap()
}

/// Whether `object` has `/Type` set to `type_name`.
pub(crate) fn has_type(object: &Object, type_name: &[u8]) -> bool {
    object.type_name().is_ok_and(|name| name == type_name)
}

/// How many objects in `document` have `/Type` set to `type_name`.
pub(crate) fn count_of_type(document: &Document, type_name: &[u8]) -> usize {
    document
        .objects
        .values()
        .filter(|object| has_type(object, type_name))
        .count()
}

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(crate) fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart.
        let name = format!("danonpdf-{}-{test_name}", std::process::id());
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The folder itself.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The names of everything in this folder, sorted, so a test can check
    /// that a merge left no stray file (such as a temporary one) behind.
    pub(crate) fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Saves `document` into this folder as `name` and returns its path.
    pub(crate) fn save(&self, name: &str, mut document: Document) -> PathBuf {
        let path = self.file(name);
        document.save(&path).unwrap();
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Encrypts `document` with RC4 128 bits and the given user password.
pub(crate) fn encrypted(mut document: Document, user_password: &str) -> Document {
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
