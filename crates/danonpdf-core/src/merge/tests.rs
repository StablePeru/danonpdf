//! Tests for merging. Every PDF they use is built here with lopdf, so the
//! tests need no fixture files and say exactly what each input contains.

use std::fs;
use std::path::{Path, PathBuf};

use lopdf::content::{Content, Operation};
use lopdf::{
    Dictionary, Document, EncryptionState, EncryptionVersion, Object, ObjectId, Permissions,
    Stream, dictionary,
};

use super::*;

/// An A4 page box: `[0 0 595 842]`.
fn a4() -> Object {
    vec![0.into(), 0.into(), 595.into(), 842.into()].into()
}

/// A content stream that writes `marker` (for example "A1") on the page.
fn marker_stream(marker: &str) -> Stream {
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
fn with_pages(label: &str, page_count: i64) -> Document {
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
fn with_inherited_attributes() -> Document {
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
fn markers(document: &Document) -> Vec<String> {
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
fn root_pages_id(document: &Document) -> ObjectId {
    document
        .catalog()
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference)
        .unwrap()
}

/// Whether `object` has `/Type` set to `type_name`.
fn has_type(object: &Object, type_name: &[u8]) -> bool {
    object.type_name().is_ok_and(|name| name == type_name)
}

/// How many objects in `document` have `/Type` set to `type_name`.
fn count_of_type(document: &Document, type_name: &[u8]) -> usize {
    document
        .objects
        .values()
        .filter(|object| has_type(object, type_name))
        .count()
}

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart.
        let name = format!("danonpdf-{}-{test_name}", std::process::id());
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The names of everything in this folder, sorted, so a test can check
    /// that a merge left no stray file (such as a temporary one) behind.
    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Saves `document` into this folder as `name` and returns its path.
    fn save(&self, name: &str, mut document: Document) -> PathBuf {
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

#[test]
fn pages_keep_input_order() {
    let merged = merge_documents(vec![with_pages("A", 2), with_pages("B", 3)]).unwrap();

    assert_eq!(markers(&merged), ["A1", "A2", "B1", "B2", "B3"]);
}

#[test]
fn max_id_covers_every_object() {
    let merged = merge_documents(vec![with_pages("A", 2), with_pages("B", 3)]).unwrap();

    let highest = merged.objects.keys().map(|id| id.0).max().unwrap();
    assert_eq!(merged.max_id, highest);
}

#[test]
fn saved_file_has_one_consistent_page_tree() {
    let scratch = ScratchDir::new("consistent-tree");
    let a = scratch.save("a.pdf", with_pages("A", 2));
    let b = scratch.save("b.pdf", with_pages("B", 3));
    let output = scratch.file("merged.pdf");

    let page_count = merge_files(&[a, b], &output).unwrap();

    assert_eq!(page_count, 5);
    let reloaded = Document::load(&output).unwrap();
    let root_id = root_pages_id(&reloaded);
    let root = reloaded.get_dictionary(root_id).unwrap();
    assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 5);
    assert_eq!(root.get(b"Kids").unwrap().as_array().unwrap().len(), 5);
    for page_id in reloaded.page_iter() {
        let page = reloaded.get_dictionary(page_id).unwrap();
        assert_eq!(
            page.get(b"Parent").unwrap().as_reference().unwrap(),
            root_id
        );
    }
    assert_eq!(markers(&reloaded), ["A1", "A2", "B1", "B2", "B3"]);
}

#[test]
fn pages_keep_inherited_attributes() {
    let merged = merge_documents(vec![with_pages("A", 1), with_inherited_attributes()]).unwrap();

    let page_id = merged.page_iter().nth(1).unwrap();
    let page = merged.get_dictionary(page_id).unwrap();
    let media_box = page.get(b"MediaBox").unwrap();
    assert_eq!(
        media_box,
        &Object::from(vec![0.into(), 0.into(), 300.into(), 500.into()])
    );
    let resources = page
        .get_deref(b"Resources", &merged)
        .unwrap()
        .as_dict()
        .unwrap();
    let fonts = resources
        .get_deref(b"Font", &merged)
        .unwrap()
        .as_dict()
        .unwrap();
    let font = fonts.get_deref(b"F1", &merged).unwrap().as_dict().unwrap();
    assert_eq!(
        font.get(b"BaseFont").unwrap().as_name().unwrap(),
        b"Courier"
    );
}

#[test]
fn single_input_is_copied() {
    let merged = merge_documents(vec![with_pages("A", 3)]).unwrap();

    assert_eq!(markers(&merged), ["A1", "A2", "A3"]);
}

#[test]
fn empty_input_list_is_rejected() {
    assert!(matches!(
        merge_documents(Vec::new()),
        Err(MergeError::NoInputs)
    ));

    let scratch = ScratchDir::new("no-inputs");
    let output = scratch.file("merged.pdf");
    let no_paths: &[PathBuf] = &[];
    assert!(matches!(
        merge_files(no_paths, &output),
        Err(MergeError::NoInputs)
    ));
    assert!(!output.exists(), "nothing should be written");
}

#[test]
fn unreadable_input_names_its_path() {
    let scratch = ScratchDir::new("unreadable");
    let good = scratch.save("good.pdf", with_pages("A", 1));
    let not_a_pdf = scratch.file("notes.pdf");
    fs::write(&not_a_pdf, "just some text, no PDF here").unwrap();
    let missing = scratch.file("missing.pdf");

    for bad in [not_a_pdf, missing] {
        let result = merge_files(&[good.clone(), bad.clone()], &scratch.file("merged.pdf"));
        match result {
            Err(MergeError::Read { path, .. }) => assert_eq!(path, bad),
            other => panic!("expected a read error for {}, got {other:?}", bad.display()),
        }
    }
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

#[test]
fn encrypted_input_is_rejected() {
    let scratch = ScratchDir::new("encrypted");
    let plain = scratch.save("plain.pdf", with_pages("A", 1));
    let locked = scratch.save("locked.pdf", encrypted(with_pages("B", 1), "secret"));

    let result = merge_files(&[plain, locked], &scratch.file("merged.pdf"));

    assert!(
        matches!(result, Err(MergeError::Encrypted { input: 2 })),
        "got {result:?}"
    );
}

#[test]
fn encrypted_document_in_memory_is_rejected() {
    let locked = encrypted(with_pages("B", 1), "secret");

    let result = merge_documents(vec![locked]);

    assert!(
        matches!(result, Err(MergeError::Encrypted { input: 1 })),
        "got {result:?}"
    );
}

#[test]
fn empty_password_input_is_merged() {
    // lopdf opens files whose user password is empty by itself, as every
    // PDF reader does, so they merge like any other file.
    let scratch = ScratchDir::new("empty-password");
    let a = scratch.save("a.pdf", with_pages("A", 1));
    let b = scratch.save("b.pdf", encrypted(with_pages("B", 1), ""));
    let output = scratch.file("merged.pdf");

    merge_files(&[a, b], &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert!(!reloaded.trailer.has(b"Encrypt"));
    assert_eq!(markers(&reloaded), ["A1", "B1"]);
}

#[test]
fn input_without_page_tree_is_broken() {
    let mut no_tree = with_pages("B", 1);
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

    let result = merge_documents(vec![with_pages("A", 1), no_tree]);

    assert!(
        matches!(
            result,
            Err(MergeError::Broken {
                input: 2,
                reason: "it has no page tree"
            })
        ),
        "got {result:?}"
    );
}

#[test]
fn input_without_catalog_is_broken() {
    // Two ways to lose the catalog: the trailer does not name one, or it
    // names an object that is not there.
    let mut unnamed = with_pages("B", 1);
    unnamed.trailer.remove(b"Root");
    let mut missing = with_pages("B", 1);
    let catalog_id = missing
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    missing.objects.remove(&catalog_id);

    for no_catalog in [unnamed, missing] {
        let result = merge_documents(vec![with_pages("A", 1), no_catalog]);

        assert!(
            matches!(
                result,
                Err(MergeError::Broken {
                    input: 2,
                    reason: "it has no catalog"
                })
            ),
            "got {result:?}"
        );
    }
}

#[test]
fn input_with_empty_page_tree_is_broken() {
    let mut empty = with_pages("B", 1);
    let root_id = root_pages_id(&empty);
    let root = empty.get_dictionary_mut(root_id).unwrap();
    root.set("Kids", Vec::<Object>::new());
    root.set("Count", 0);

    let result = merge_documents(vec![with_pages("A", 1), empty]);

    assert!(
        matches!(
            result,
            Err(MergeError::Broken {
                input: 2,
                reason: "its page tree has no pages"
            })
        ),
        "got {result:?}"
    );
}

#[test]
fn merged_version_is_the_newest_input_version() {
    // The newest one sits in the middle, so taking the first or the last
    // version by mistake would fail this test.
    let versions = ["1.4", "1.7", "1.5"];
    let documents = versions
        .iter()
        .map(|&version| {
            let mut document = with_pages("A", 1);
            document.version = version.to_owned();
            document
        })
        .collect();

    let merged = merge_documents(documents).unwrap();

    assert_eq!(merged.version, "1.7");
}

#[test]
fn output_may_be_one_of_the_inputs() {
    let scratch = ScratchDir::new("output-is-input");
    let a = scratch.save("a.pdf", with_pages("A", 2));
    let b = scratch.save("b.pdf", with_pages("B", 1));

    let page_count = merge_files(&[a.clone(), b], &a).unwrap();

    assert_eq!(page_count, 3);
    assert_eq!(markers(&Document::load(&a).unwrap()), ["A1", "A2", "B1"]);
    assert_eq!(scratch.entries(), ["a.pdf", "b.pdf"]);
}

#[test]
fn successful_save_leaves_no_temporary_file() {
    let scratch = ScratchDir::new("no-leftovers");
    let a = scratch.save("a.pdf", with_pages("A", 1));
    let b = scratch.save("b.pdf", with_pages("B", 1));

    merge_files(&[a, b], &scratch.file("merged.pdf")).unwrap();

    assert_eq!(scratch.entries(), ["a.pdf", "b.pdf", "merged.pdf"]);
}

#[test]
fn save_into_missing_folder_names_the_output() {
    let scratch = ScratchDir::new("missing-folder");
    let a = scratch.save("a.pdf", with_pages("A", 1));
    let output = scratch.file("missing").join("merged.pdf");

    match merge_files(&[a], &output) {
        Err(MergeError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn failed_save_removes_its_temporary_file() {
    // A folder already holds the output name. Writing the temporary file
    // works, but putting it in the folder's place fails, so this is the case
    // where there is a temporary file to clean up.
    let scratch = ScratchDir::new("output-is-folder");
    let a = scratch.save("a.pdf", with_pages("A", 1));
    let output = scratch.file("taken");
    fs::create_dir(&output).unwrap();

    match merge_files(&[a], &output) {
        Err(MergeError::Save { path, .. }) => assert_eq!(path, output),
        other => panic!("expected a save error, got {other:?}"),
    }
    assert_eq!(scratch.entries(), ["a.pdf", "taken"]);
}

#[test]
fn old_catalogs_and_page_trees_are_dropped() {
    let scratch = ScratchDir::new("orphans");
    let inputs = [
        with_pages("A", 2),
        with_pages("B", 3),
        with_inherited_attributes(),
    ];
    let input_objects: usize = inputs.iter().map(|document| document.objects.len()).sum();
    // `into_iter` on an array hands out the documents themselves (a move),
    // so `save` can take each one by value.
    let paths: Vec<PathBuf> = inputs
        .into_iter()
        .enumerate()
        .map(|(index, document)| scratch.save(&format!("{index}.pdf"), document))
        .collect();
    let output = scratch.file("merged.pdf");

    merge_files(&paths, &output).unwrap();

    let reloaded = Document::load(&output).unwrap();
    assert_eq!(count_of_type(&reloaded, b"Catalog"), 1);
    assert_eq!(count_of_type(&reloaded, b"Pages"), 1);
    assert!(reloaded.objects.len() <= input_objects);
    // Pruning again finds nothing in use by no one. The one exception is the
    // file's own cross-reference stream: lopdf writes the object index as a
    // stream object and reads it back, but nothing refers to it.
    let unused = reloaded.clone().prune_objects();
    let orphans: Vec<ObjectId> = unused
        .into_iter()
        .filter(|&id| !has_type(reloaded.get_object(id).unwrap(), b"XRef"))
        .collect();
    assert_eq!(orphans, Vec::new());
}

#[test]
fn error_messages_name_the_problem() {
    let read = MergeError::Read {
        path: Path::new("a.pdf").to_path_buf(),
        source: lopdf::Error::InvalidPassword,
    };
    assert_eq!(read.to_string(), "cannot read a.pdf as a PDF");
    assert_eq!(
        MergeError::Encrypted { input: 2 }.to_string(),
        "input 2 is encrypted and needs a password"
    );
}
