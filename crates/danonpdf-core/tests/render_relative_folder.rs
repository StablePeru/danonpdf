//! The library folder given as a relative path, and the current folder
//! changing afterwards.
//!
//! The current folder belongs to the whole process, so changing it would
//! pull the ground from under every other test running at that moment.
//! This file holds one test, and a file in `tests/` is a process of its own
//! (see `common/mod.rs`): nothing else runs next to it.

mod common;

use std::env;
use std::path::{self, Path};

use danonpdf_core::Renderer;

use common::{ScratchDir, missing_library_path, real_library_folder};

#[test]
fn a_relative_folder_is_remembered_as_a_full_path() {
    // The real folder, cut in two: where to stand, and the one word that
    // leads from there to the library ("lib", in the build image).
    let real = path::absolute(real_library_folder()).unwrap();
    let above = real.parent().expect("the library folder has a parent");
    let relative = Path::new(real.file_name().expect("the library folder has a name"));
    env::set_current_dir(above).unwrap();

    let first = Renderer::open(relative).unwrap();

    // Remembered in full: the path means the same file wherever the
    // process stands later.
    assert!(
        first.library().is_absolute(),
        "{}",
        first.library().display()
    );
    assert_eq!(first.library().parent(), Some(real.as_path()));
    // Asked again, in the same words and in full: the same engine.
    assert_eq!(Renderer::open(relative).unwrap().library(), first.library());
    assert_eq!(Renderer::open(&real).unwrap().library(), first.library());

    // The process moves to another folder.
    let elsewhere = ScratchDir::new("relative");
    env::set_current_dir(elsewhere.path()).unwrap();

    // The loaded library is still recognised by its full path...
    let after_the_move = Renderer::open(&real);
    assert!(
        after_the_move.is_ok(),
        "expected the loaded library, got {after_the_move:?}"
    );
    // ...and the relative words now name another place, where there is no
    // library. The error says which place, in full.
    let missing = missing_library_path(relative);
    let here = env::current_dir().unwrap();
    assert_eq!(missing.parent(), Some(here.join(relative).as_path()));
}
