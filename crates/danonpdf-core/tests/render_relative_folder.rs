//! The library folder given as a relative path, and the current folder
//! changing afterwards.
//!
//! The current folder belongs to the whole process, so changing it would
//! pull the ground from under every other test running at that moment.
//! This file holds one test, and a file in `tests/` is a process of its own
//! (see `common/mod.rs`): nothing else runs next to it.

mod common;

use std::env;
use std::path::{self, Path, PathBuf};

use danonpdf_core::Renderer;

use common::{ScratchDir, missing_library_path, real_library_folder};

/// Takes the process back to a folder when it is dropped.
///
/// The test below moves into a scratch folder, and Windows does not delete
/// a folder that is the current folder of a running process. Leaving it in
/// `Drop` makes that happen however the test ends, a failed assertion
/// included.
struct BackTo(PathBuf);

impl Drop for BackTo {
    fn drop(&mut self) {
        // Nothing to do about a failure here: the test is over, and a
        // scratch folder that stays behind is in the temp folder.
        let _ = env::set_current_dir(&self.0);
    }
}

#[test]
fn a_relative_folder_is_remembered_as_a_full_path() {
    // The real folder, cut in two: where to stand, and the one word that
    // leads from there to the library ("lib", in the build image).
    let real = path::absolute(real_library_folder()).unwrap();
    let above = real.parent().expect("the library folder has a parent");
    let relative = Path::new(real.file_name().expect("the library folder has a name"));
    env::set_current_dir(above).unwrap();
    // Where the process says it stands now. Not `above` itself: the system
    // answers with the real folder, which is spelled differently when
    // `above` is reached through a symbolic link or holds a `..`. The core
    // builds the full path from this same answer, so this is what a correct
    // core gives, however the folder that was set is spelled.
    let expected = env::current_dir().unwrap().join(relative);

    let first = Renderer::open(relative).unwrap();

    // Remembered in full: the path means the same file wherever the
    // process stands later.
    assert!(
        first.library().is_absolute(),
        "{}",
        first.library().display()
    );
    assert_eq!(first.library().parent(), Some(expected.as_path()));
    // Asked again, in the same words and in full: the same engine.
    assert_eq!(Renderer::open(relative).unwrap().library(), first.library());
    assert_eq!(Renderer::open(&real).unwrap().library(), first.library());

    // The process moves to another folder.
    let elsewhere = ScratchDir::new("relative");
    // Declared after the scratch folder on purpose. Values are dropped in
    // the reverse order of their declaration, so the process leaves the
    // scratch folder first, and only then is the folder deleted.
    let _back = BackTo(above.to_path_buf());
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
