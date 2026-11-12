//! Tests for merging. Every PDF they use is built with lopdf by the helpers
//! in `test_support.rs`, so the tests need no fixture files and say exactly
//! what each input contains.

use std::fs;
use std::path::PathBuf;

use lopdf::{Document, Object, ObjectId};

use super::*;
use crate::test_support::{
    ScratchDir, count_of_type, encrypted, has_type, markers, root_pages_id,
    with_inherited_attributes, with_pages,
};

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
        Err(PdfError::NoInputs)
    ));

    let scratch = ScratchDir::new("no-inputs");
    let output = scratch.file("merged.pdf");
    let no_paths: &[PathBuf] = &[];
    assert!(matches!(
        merge_files(no_paths, &output),
        Err(PdfError::NoInputs)
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
            Err(PdfError::Read { path, .. }) => assert_eq!(path, bad),
            other => panic!("expected a read error for {}, got {other:?}", bad.display()),
        }
    }
}

#[test]
fn encrypted_input_is_rejected() {
    let scratch = ScratchDir::new("encrypted");
    let plain = scratch.save("plain.pdf", with_pages("A", 1));
    let locked = scratch.save("locked.pdf", encrypted(with_pages("B", 1), "secret"));

    let result = merge_files(&[plain, locked], &scratch.file("merged.pdf"));

    assert!(
        matches!(
            result,
            Err(PdfError::Encrypted {
                input: WhichInput::Numbered(2)
            })
        ),
        "got {result:?}"
    );
}

#[test]
fn encrypted_document_in_memory_is_rejected() {
    let locked = encrypted(with_pages("B", 1), "secret");

    let result = merge_documents(vec![locked]);

    assert!(
        matches!(
            result,
            Err(PdfError::Encrypted {
                input: WhichInput::Numbered(1)
            })
        ),
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
            Err(PdfError::Broken {
                input: WhichInput::Numbered(2),
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
                Err(PdfError::Broken {
                    input: WhichInput::Numbered(2),
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
            Err(PdfError::Broken {
                input: WhichInput::Numbered(2),
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
        Err(PdfError::Save { path, .. }) => assert_eq!(path, output),
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
        Err(PdfError::Save { path, .. }) => assert_eq!(path, output),
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
