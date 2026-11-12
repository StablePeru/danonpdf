//! Rules about file paths that every front end must apply the same way.
//!
//! The window and the command-line tool both refuse to write the merged file
//! over one of the files being merged. If each kept its own idea of "the same
//! file", they would drift apart; here there is one, and both call it.

use std::fs;
use std::path::Path;

/// Whether `a` and `b` name the same file.
///
/// Comparing the paths as text is not enough: `docs/./a.pdf` and
/// `docs/a.pdf` are the same file, and on Windows so are `C:\Docs\A.PDF`
/// and `c:\docs\a.pdf`, because Windows ignores case in file names.
/// `fs::canonicalize` asks the system for the one full path of each file,
/// with `.`, `..` and links resolved and, on Windows, the case as stored on
/// disk. The tests run on Linux, so they cover the first kind only; the
/// case rule comes from Windows itself.
///
/// `canonicalize` fails for a path that does not exist. Then the paths are
/// compared as given: a file that does not exist yet cannot be one of the
/// inputs anyway, since the merge needs to read every input.
///
/// Known limits, accepted for now:
///
/// - A hard link is a second name for the same data, with its own path.
///   `canonicalize` gives each name its own path, so a hard link to an
///   input is not recognised as that input.
/// - `canonicalize` can also fail for a file that does exist, for example
///   when a folder on the way cannot be read. The paths are then compared
///   as given too, so another spelling of the same file slips through.
///
/// Catching both would mean comparing file identities (device and inode on
/// Linux, volume and file index on Windows), which std does not offer in a
/// portable way.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    // `super::*` also brings in what the parent module imported (`fs` and
    // `Path`), so only the extra names are listed above.
    use super::*;

    /// An empty folder of its own for one test.
    ///
    /// Tests run in parallel threads of the same process, so the folder name
    /// carries the test's own `name` as well as the process id: two tests
    /// never share (or delete) each other's files.
    fn fresh_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("danonpdf-paths-{name}-{}", std::process::id()));
        // Left over from an earlier run that stopped half way; no error if not.
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn same_file_sees_through_different_spellings() {
        let folder = fresh_folder("same-file");
        let file = folder.join("a.pdf");
        fs::write(&file, b"not really a pdf").unwrap();
        fs::write(folder.join("b.pdf"), b"another").unwrap();

        let dotted = folder.join(".").join("a.pdf");
        let up_and_back = folder
            .join("..")
            .join(folder.file_name().unwrap())
            .join("a.pdf");
        let results = (
            same_file(&file, &dotted),
            same_file(&file, &up_and_back),
            same_file(&file, &folder.join("b.pdf")),
        );
        fs::remove_dir_all(&folder).unwrap();

        assert_eq!(results, (true, true, false));
    }

    #[test]
    fn same_file_compares_paths_that_do_not_exist_as_given() {
        // Nothing is on disk here, so the system cannot help: equal text means
        // the same file, and different text means different files.
        let docs = Path::new("docs");
        assert!(same_file(&docs.join("a.pdf"), &docs.join("a.pdf")));
        assert!(!same_file(&docs.join("a.pdf"), &docs.join("b.pdf")));
    }

    #[test]
    fn a_missing_output_is_never_the_same_as_an_existing_input() {
        let folder = fresh_folder("missing-output");
        let input = folder.join("a.pdf");
        fs::write(&input, b"x").unwrap();
        let result = same_file(&input, &folder.join("new.pdf"));
        fs::remove_dir_all(&folder).unwrap();

        assert!(!result);
    }
}
