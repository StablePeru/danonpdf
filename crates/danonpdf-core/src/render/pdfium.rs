//! The one file of DanonPDF that knows PDFium.
//!
//! PDFium is a C++ library with a C interface: plain functions such as
//! `FPDF_LoadPage`, which take and return raw pointers. Rust cannot check
//! what happens behind such a function, so every call to one is `unsafe`.
//! The `pdfium-render` crate makes those calls and wraps the pointers in
//! types that free them when they are dropped; this file only uses its safe
//! types, and holds no `unsafe` of its own.
//!
//! Nothing outside this file names a type of that crate. To draw pages with
//! another engine, this is the file to replace: what the rest of the core
//! needs from it is [`library_path`], [`load`], [`Engine::page_count`] and
//! [`Engine::render_page`].

use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use pdfium_render::prelude::{
    PdfPageIndex, PdfRenderConfig, Pdfium, PdfiumError, PdfiumInternalError, Pixels,
};

use super::errors::Cause;
use super::{PageImage, RenderError, image_size};
use crate::same_file;

/// PDFium, loaded, and the file it was loaded from.
pub(super) struct Engine {
    /// PDFium keeps state of its own between calls and must never be
    /// entered by two threads at once. `pdfium-render` already takes a lock
    /// around every single call (its `thread_safe` feature). That is not
    /// enough for an operation made of several calls: asking why a file
    /// could not be opened is a second call, after the one that failed, and
    /// another thread could open its own file in between and leave its own
    /// answer there.
    ///
    /// So the whole of an operation runs under this lock. A `Mutex` owns
    /// what it protects: the only way to reach the `Pdfium` inside is to
    /// lock it, so no code path can forget to.
    pdfium: Mutex<Pdfium>,
    /// The library file, as an absolute path: the one the system was given
    /// to load. A relative path would mean another file as soon as the
    /// process changed its current folder, and this one has to name the
    /// same file for as long as the program runs.
    library: PathBuf,
}

/// The engine of this process, once it is loaded.
///
/// A `static` lives for the whole run of the program and is seen by every
/// thread, so Rust only accepts types there that are safe to share between
/// threads (`Sync`). `OnceLock` is a cell that starts empty, can be filled
/// exactly once and never changes afterwards: after that, every thread
/// reads it without waiting, and what it holds can be borrowed for as long
/// as the program runs (`&'static`).
///
/// It is filled once because PDFium can be started once in a process:
/// `pdfium-render` refuses to bind to a library a second time.
static ENGINE: OnceLock<Engine> = OnceLock::new();

/// Held while the library is being loaded, so two threads that ask for the
/// engine at the same moment do not both try to load it. `OnceLock` alone
/// cannot do it: its `get_or_init` takes a function that cannot fail, and
/// loading a library can.
static LOADING: Mutex<()> = Mutex::new(());

/// The path of the PDFium library inside `folder`, as relative or as
/// absolute as `folder` is. The file name depends on the system:
/// `pdfium.dll` on Windows, `libpdfium.so` on Linux and `libpdfium.dylib`
/// on macOS.
pub(super) fn library_path(folder: &Path) -> PathBuf {
    Pdfium::pdfium_platform_library_name_at_path(folder)
}

/// The engine, loading PDFium from `folder` if this process has not loaded
/// it yet.
///
/// Three questions, in this order, and the order is the rule:
///
/// 1. Is the file asked for the one that is loaded? Then that engine is
///    the answer, whether or not the file is still on the disk: PDFium is
///    in memory, and the file is not read again.
/// 2. Is there a library file in `folder` at all? If not, it is missing,
///    whatever is loaded: the answer for a folder without the library does
///    not depend on who called first.
/// 3. Is another library loaded already? Then this one cannot be.
pub(super) fn load(folder: &Path) -> Result<&'static Engine, RenderError> {
    let as_given = library_path(folder);
    // The absolute path, worked out once and used for everything below:
    // the comparison with what is loaded, the errors and the load itself.
    //
    // The system must be given a full path. Given a bare file name, Windows
    // would search for it: in the folders of the system, in the current
    // folder and along PATH. The current folder may be a downloads folder,
    // and whatever is called pdfium.dll there would be loaded and run.
    //
    // `path::absolute` and not `fs::canonicalize`. It only puts the current
    // folder in front of a relative path and never touches the disk, so it
    // also works for a file that is not there, which is how a missing
    // library gets its full path into the error. And on Windows,
    // `canonicalize` answers in the `\\?\C:\...` form, which reads badly in
    // a message. It fails only when the system cannot say what the current
    // folder is.
    let asked = std::path::absolute(&as_given).map_err(|error| RenderError::LibraryFailed {
        path: as_given,
        source: error.into(),
    })?;

    // A lock is "poisoned" when a thread panicked while holding it, and
    // `lock()` then returns an error with the guard inside. This lock
    // protects no data (its content is `()`), so there is nothing a panic
    // could have left half done: `into_inner` takes the guard anyway.
    let _one_at_a_time = LOADING.lock().unwrap_or_else(PoisonError::into_inner);

    let loaded = ENGINE.get();
    // `if let ... && ...`: the pattern has to match and the test has to
    // hold, or the block is skipped.
    if let Some(engine) = loaded
        && engine.was_loaded_from(&asked)
    {
        return Ok(engine);
    }
    if !asked.is_file() {
        return Err(RenderError::LibraryMissing { path: asked });
    }
    if let Some(engine) = loaded {
        return Err(RenderError::LibraryAlreadyLoaded {
            loaded: engine.library.clone(),
            asked,
        });
    }

    // This is where the foreign code comes in: the system maps the file
    // into the process, and `pdfium-render` looks up every function it
    // needs by name. A file that is not a library, or a PDFium of another
    // build that lacks one of the functions, fails here. A failure leaves
    // the process as it was: a later call can still load a good library.
    //
    // A `match` and not `map_err` with a closure: the error takes `asked`
    // for itself, and so does the engine below. A closure that moves
    // `asked` into the error owns it from the moment it is written, even
    // if it never runs, and the compiler then refuses the second use
    // (E0382). The arms of a `match` are plain code paths: the compiler
    // sees that only one of them gives `asked` away, and that it returns.
    let bindings = match Pdfium::bind_to_library(&asked) {
        Ok(bindings) => bindings,
        Err(error) => {
            return Err(RenderError::LibraryFailed {
                path: asked,
                source: load_failure(error),
            });
        }
    };
    let engine = Engine {
        // `Pdfium::new` starts the library (`FPDF_InitLibrary`).
        pdfium: Mutex::new(Pdfium::new(bindings)),
        library: asked,
    };
    // The cell is empty: it was checked above, under the lock that every
    // loader holds. So this stores `engine` and returns it borrowed for
    // the rest of the program.
    Ok(ENGINE.get_or_init(move || engine))
}

impl Engine {
    /// The library file this engine was loaded from, as an absolute path.
    pub(super) fn library(&self) -> &Path {
        &self.library
    }

    /// Whether `asked`, an absolute path, is the file this engine was
    /// loaded from.
    fn was_loaded_from(&self, asked: &Path) -> bool {
        // Compared as paths first. That needs no file, so it still holds
        // when the library was deleted or moved away after it was loaded.
        // `same_file` then asks the disk, and catches the spellings that a
        // plain comparison cannot: a link to the folder, a `..` on the way,
        // another letter case on Windows.
        self.library == asked || same_file(&self.library, asked)
    }

    /// How many pages the PDF at `path` has.
    pub(super) fn page_count(&self, path: &Path) -> Result<usize, RenderError> {
        // The same lock, and the same way of opening, as `render_page`
        // below: its comments explain both. The two do not share a helper
        // that returns the open document, because the document borrows the
        // guard of the lock, and a function cannot return a value together
        // with what it borrows from (E0515).
        let pdfium = self.pdfium.lock().unwrap_or_else(PoisonError::into_inner);
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|error| open_failure(error, path))?;

        // PDFium answers in a C `int`. A negative count would be a bug in
        // PDFium; it is shown as a document without pages, as in the error
        // of a page past the end.
        Ok(usize::try_from(document.pages().len()).unwrap_or(0))
    }

    /// Draws page `number` (counted from 1) of the PDF at `path`, `width`
    /// pixels wide. The caller has checked that `width` is in range.
    pub(super) fn render_page(
        &self,
        path: &Path,
        number: NonZeroUsize,
        width: u32,
    ) -> Result<PageImage, RenderError> {
        // If a thread panicked while drawing, the lock is poisoned.
        // Nothing of ours is left half done inside (the guard holds no
        // state between operations), so drawing goes on.
        let pdfium = self.pdfium.lock().unwrap_or_else(PoisonError::into_inner);

        // `None`: no password. PDFium still tries the empty password by
        // itself, so a file that is encrypted and opens for anyone opens
        // here too.
        //
        // The document borrows `pdfium` (its type is `PdfDocument<'_>`):
        // the compiler will not let it outlive the guard above. Values
        // are dropped in the reverse order of their declaration, so the
        // bitmap, the page and the document are closed first, and the
        // lock is released last.
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|error| open_failure(error, path))?;
        let pages = document.pages();

        // PDFium counts pages from 0, in a C `int`. `try_from` fails for a
        // number too big for it, and `filter` drops one past the end: both
        // are the same answer for the caller.
        let count = pages.len();
        let index = PdfPageIndex::try_from(number.get() - 1)
            .ok()
            .filter(|index| *index < count);
        let Some(index) = index else {
            return Err(RenderError::PageOutOfRange {
                page: number.get(),
                // A negative count would be a bug in PDFium; it is shown
                // as a document without pages.
                pages: usize::try_from(count).unwrap_or(0),
            });
        };

        // PDFium gives no reason when a page fails to load: the page
        // number is in range, so what is wrong is inside the file.
        let page = pages.get(index).map_err(|_| RenderError::Broken {
            reason: "a page cannot be read",
        })?;

        // The size in points (1/72 of an inch), with the page's own
        // rotation already applied.
        let (width, height) = image_size(page.width().value, page.height().value, width).ok_or(
            RenderError::Broken {
                reason: "a page has no size",
            },
        )?;
        let config = PdfRenderConfig::new()
            // Both are at most `MAX_RENDER_HEIGHT`, far below what the C
            // `int` behind `Pixels` holds, so `as` cannot cut them.
            .set_fixed_size(width as Pixels, height as Pixels)
            // PDFium's own byte order is blue, green, red, alpha. Asked
            // this way it writes red first, so the bytes need no second
            // pass to swap them. It is the crate's default; it is spelled
            // out because `PageImage` promises this order.
            .set_reverse_byte_order(true);
        // The crate first fills the bitmap with opaque white (its default
        // "paper"), then PDFium draws the page over it.
        let bitmap = page
            .render_with_config(&config)
            .map_err(|error| RenderError::Engine {
                doing: "drawing the page",
                detail: format!("{error:?}"),
            })?;
        // A copy of PDFium's buffer, owned by Rust: the buffer itself is
        // freed when `bitmap` is dropped, at the end of this function.
        let rgba = bitmap.as_rgba_bytes();

        // Checked instead of trusted: everything that reads a `PageImage`
        // relies on this length.
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(RenderError::Engine {
                doing: "reading the drawn page",
                detail: format!(
                    "{} bytes for {width} by {height} pixels, not {expected}",
                    rgba.len()
                ),
            });
        }

        Ok(PageImage {
            width,
            height,
            rgba,
        })
    }
}

/// Why the library at a path could not be loaded, as an error to keep
/// behind `RenderError::LibraryFailed`.
fn load_failure(error: PdfiumError) -> Cause {
    match error {
        // What the system said ("invalid ELF header", "the specified
        // module could not be found"...), or which function is missing.
        PdfiumError::LoadLibraryError(source) => Box::new(source),
        // `into` turns a `String` into a boxed error whose message is
        // that text.
        PdfiumError::LoadLibraryFunctionNameError(name) => name.into(),
        other => format!("{other:?}").into(),
    }
}

/// What a failure to open the PDF at `path` means for the caller.
///
/// PDFium explains a failed open with one of six codes, which
/// `pdfium-render` hands over as `PdfiumInternalError`:
///
/// | PDFium says | Meaning for the caller |
/// | --- | --- |
/// | `FileError` | [`RenderError::Read`]: the file could not be read |
/// | `FormatError` | [`RenderError::NotPdf`]: not a PDF, or too damaged to open |
/// | `PasswordError` | [`RenderError::Encrypted`]: it needs a password |
/// | `SecurityError` | [`RenderError::UnsupportedProtection`] |
/// | `PageError` | [`RenderError::Broken`]: a page cannot be read |
/// | `Unknown` | [`RenderError::Engine`]: PDFium gave no reason |
///
/// Before PDFium is asked at all, `pdfium-render` opens the file itself; a
/// file that is missing or may not be read fails there, with the system's
/// own error, and is a [`RenderError::Read`] too.
fn open_failure(error: PdfiumError, path: &Path) -> RenderError {
    let engine_failure = |detail: String| RenderError::Engine {
        doing: "opening the file",
        detail,
    };
    match error {
        PdfiumError::IoError(source) => RenderError::Read {
            path: path.to_path_buf(),
            source,
        },
        // No `_` arm in this inner `match`: if a later PDFium adds a
        // seventh code, this stops compiling until it is given a meaning.
        PdfiumError::PdfiumLibraryInternalError(reason) => match reason {
            PdfiumInternalError::FileError => RenderError::Read {
                path: path.to_path_buf(),
                source: io::Error::other("PDFium could not read the file"),
            },
            PdfiumInternalError::FormatError => RenderError::NotPdf {
                path: path.to_path_buf(),
            },
            PdfiumInternalError::PasswordError => RenderError::Encrypted,
            PdfiumInternalError::SecurityError => RenderError::UnsupportedProtection,
            PdfiumInternalError::PageError => RenderError::Broken {
                reason: "a page cannot be read",
            },
            PdfiumInternalError::Unknown => engine_failure("no reason given".to_owned()),
        },
        // The crate has dozens of other errors, none of which opening a
        // file is meant to produce. If one does, it is shown as it is.
        other => engine_failure(format!("{other:?}")),
    }
}
