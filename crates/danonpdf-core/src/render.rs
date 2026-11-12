//! Drawing a page of a PDF as pixels, and writing those pixels as a PNG.
//!
//! The rest of the core works on the structure of a PDF: which objects make
//! up a page, and where to copy them. Drawing is another job. It means
//! running the page's drawing instructions, with its fonts and images, and
//! that takes an engine far bigger than this whole program. DanonPDF uses
//! PDFium, the engine of Chromium: a library written in C++ that is loaded
//! from a file next to the program when it runs (`pdfium.dll` on Windows).
//!
//! What a caller sees is small and says nothing about PDFium:
//!
//! - [`Renderer`]: the engine, loaded. [`Renderer::open`] loads it from a
//!   folder, and [`default_library_folder`] says which folder that is
//!   unless told otherwise.
//! - [`Renderer::page_count`]: how many pages a file has, for the engine
//!   that will draw them.
//! - [`Renderer::render_page`]: one page of a file, as a [`PageImage`].
//! - [`write_png`]: a [`PageImage`], as a PNG file.
//!
//! Everything that touches PDFium is in the `pdfium` module below. Drawing
//! pages with another engine means replacing that one file.

mod errors;
mod pdfium;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

pub use errors::RenderError;

use crate::save::{Durability, write_atomically};

/// The widest image that [`Renderer::render_page`] draws, in pixels.
///
/// An image takes four bytes per pixel, and while it is drawn it exists
/// twice: in PDFium's own buffer and in the copy handed to the caller.
/// Without a limit, one wrong number from a caller would ask the system for
/// gigabytes. 4096 pixels is a page at about 500 dots per inch for A4:
/// far more than a thumbnail or a preview on a screen needs.
pub const MAX_RENDER_WIDTH: u32 = 4096;

/// The tallest image that [`Renderer::render_page`] draws, in pixels.
///
/// The height is not asked for: it follows from the width and the shape of
/// the page, so it needs a limit of its own, or a page shaped like a till
/// receipt would get around [`MAX_RENDER_WIDTH`]. Twice the widest width
/// leaves room for every common paper size. The largest image is then
/// 4096 by 8192 pixels, 128 MiB of pixels.
pub const MAX_RENDER_HEIGHT: u32 = 8192;

/// The environment variable that names the folder of the PDFium library,
/// for when it is not next to the program: see [`default_library_folder`].
pub const PDFIUM_DIR_VARIABLE: &str = "DANONPDF_PDFIUM_DIR";

/// A drawn page: plain pixels, with nothing of the engine that drew them.
///
/// `rgba` holds the pixels row by row, from the top row to the bottom one
/// and from left to right inside a row. Each pixel is four bytes: red,
/// green, blue and alpha (how opaque it is), from 0 to 255. So `rgba` is
/// `width * height * 4` bytes long, and the pixel in column `x` of row `y`
/// starts at `(y * width + x) * 4`.
///
/// The page is drawn on white paper that is fully opaque, so alpha is 255
/// in every pixel. That also settles the question of "premultiplied"
/// alpha (colours stored already multiplied by their alpha): at 255 both
/// ways of storing a pixel give the same bytes, so the image can be handed
/// to code that expects either.
///
/// The fields are public: this is data to pass along, to a window or to
/// [`write_png`], not something with rules to protect.
#[derive(Clone, PartialEq, Eq)]
pub struct PageImage {
    /// How many pixels wide.
    pub width: u32,
    /// How many pixels high.
    pub height: u32,
    /// The pixels: see above.
    pub rgba: Vec<u8>,
}

impl fmt::Debug for PageImage {
    /// Written by hand instead of derived: the derived one prints every
    /// byte, and a failing test would bury its message under megabytes of
    /// numbers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba", &format_args!("{} bytes", self.rgba.len()))
            .finish()
    }
}

/// The engine that draws pages, loaded and ready.
///
/// It is a small handle to the one engine of the process: copying it is
/// free, and every copy draws with the same PDFium. It can be given to
/// another thread (it is `Send` and `Sync`), and several threads may draw
/// at the same moment. They will not be faster than one: PDFium works on
/// one page at a time, so the others wait their turn.
///
/// PDFium is never unloaded: it stays in memory until the program ends.
#[derive(Clone, Copy)]
pub struct Renderer {
    // `&'static`: a borrow that is good for the whole run of the program.
    // The engine lives in a `static` of the `pdfium` module.
    engine: &'static pdfium::Engine,
}

impl Renderer {
    /// Loads PDFium from `library_folder` and gets it ready.
    ///
    /// The folder must hold the library under the name the system gives
    /// such files: `pdfium.dll` on Windows, `libpdfium.so` on Linux. See
    /// [`default_library_folder`] for the folder a program should use.
    ///
    /// A relative folder is taken from the current folder of the process at
    /// the moment of the call, and turned into an absolute path at once.
    /// That absolute path is what the engine remembers and what every
    /// error names, so neither changes meaning if the process moves to
    /// another folder later.
    ///
    /// A process can load PDFium once. The first call that succeeds decides
    /// which file it is, for good:
    ///
    /// - A later call that names the same file gets the same engine. It
    ///   costs almost nothing, so callers need not keep the first one
    ///   around. This holds even if the file has been deleted or replaced
    ///   since: the library is in memory and is not read again. It is the
    ///   path that is compared, not what the file holds.
    /// - A later call that names a folder without the library fails with
    ///   [`RenderError::LibraryMissing`], as it would in a process that
    ///   has loaded nothing.
    /// - A later call that names another library file fails with
    ///   [`RenderError::LibraryAlreadyLoaded`], instead of handing over, in
    ///   silence, an engine that is not the one asked for.
    ///
    /// A call that fails loads nothing and uses nothing up: a later call
    /// can still load a good library.
    ///
    /// Loading a library runs code from that file inside this program, with
    /// everything the program may do. Only load it from a folder that is
    /// trusted as much as the program itself.
    pub fn open(library_folder: &Path) -> Result<Self, RenderError> {
        pdfium::load(library_folder).map(|engine| Self { engine })
    }

    /// The library file that was loaded, as an absolute path.
    pub fn library(&self) -> &Path {
        self.engine.library()
    }

    /// How many pages the PDF file at `path` has, as PDFium counts them.
    ///
    /// It is the count to use before [`Self::render_page`]: both read the
    /// file with PDFium, so every page from 1 to this number is one that
    /// drawing will accept, and no other. The core's `page_count` reads
    /// the file with lopdf, and the two can disagree about a damaged file
    /// (see "What PDFium accepts" on [`Self::render_page`]). A front end
    /// that draws the pages of a file should ask one engine both
    /// questions.
    ///
    /// A file is refused for the reasons [`Self::render_page`] refuses it,
    /// with the same errors. A document without pages is not an error
    /// here: its count is 0.
    ///
    /// The file is opened, read and closed again in this one call, and is
    /// never changed.
    pub fn page_count(&self, path: &Path) -> Result<usize, RenderError> {
        self.engine.page_count(path)
    }

    /// Draws one page of the PDF file at `path`.
    ///
    /// `page` counts from 1. Its type has no zero, so "page 0" cannot be
    /// asked for by mistake: `NonZeroUsize::new` makes the caller decide
    /// what a 0 means before it gets here.
    ///
    /// `width` is the width of the image in pixels, from 1 to
    /// [`MAX_RENDER_WIDTH`]. The height follows from the shape of the page:
    /// an A4 page drawn 100 pixels wide is 142 pixels high. Two cases bend
    /// that rule:
    ///
    /// - A page so wide that its height would round down to nothing gets
    ///   one row of pixels.
    /// - A page so tall that its height would pass [`MAX_RENDER_HEIGHT`] is
    ///   drawn smaller, keeping its shape, so it fits that height. The
    ///   image is then narrower than `width`. Read the size from the
    ///   [`PageImage`], not from what was asked.
    ///
    /// The file is opened, read and closed again in this one call. The
    /// file is only read, never changed.
    ///
    /// # What PDFium accepts
    ///
    /// The file is read by PDFium, not by the library that the rest of the
    /// core reads PDF with (lopdf). On the files tried so far they agree
    /// on the cases that matter most:
    ///
    /// - A file that needs a password is refused
    ///   ([`RenderError::Encrypted`]); there is no way to give one.
    /// - A file that is encrypted and opens with the empty password is
    ///   drawn like any other, as it is merged like any other.
    /// - A file that is not a PDF is refused ([`RenderError::NotPdf`]).
    ///
    /// And they are known to differ on damaged files, so "it can be drawn"
    /// and "it can be merged or split" are two separate questions:
    ///
    /// - A file whose end is cut off is still read by lopdf, which can
    ///   count its pages. PDFium refuses it ([`RenderError::NotPdf`]).
    /// - A file whose catalog has no page tree is damaged for the other
    ///   operations. For PDFium it is a document with no pages, so page 1
    ///   is [`RenderError::PageOutOfRange`].
    ///
    /// Two more things PDFium decides by itself:
    ///
    /// - A page that states no size is drawn as US Letter, 612 by 792
    ///   points, instead of being refused.
    /// - A page that is turned (`/Rotate`) is drawn turned, so its image
    ///   is wide where the page as stored is tall.
    pub fn render_page(
        &self,
        path: &Path,
        page: NonZeroUsize,
        width: u32,
    ) -> Result<PageImage, RenderError> {
        // Before the file is touched: a width that cannot be drawn is the
        // caller's mistake whatever the file is.
        if width == 0 || width > MAX_RENDER_WIDTH {
            return Err(RenderError::WidthOutOfRange {
                width,
                maximum: MAX_RENDER_WIDTH,
            });
        }
        self.engine.render_page(path, page, width)
    }
}

impl fmt::Debug for Renderer {
    /// PDFium itself has nothing worth printing; the file it came from is
    /// what tells two setups apart.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Renderer")
            .field("library", &self.library())
            .finish()
    }
}

/// The folder a program should load PDFium from.
///
/// It is the folder the program itself is in: the library ships next to
/// the `.exe`, and a folder that holds the program is trusted as much as
/// the program. The current folder is never used: it is wherever the user
/// happens to be, which may be a folder of downloaded files.
///
/// The environment variable `DANONPDF_PDFIUM_DIR` ([`PDFIUM_DIR_VARIABLE`])
/// names another folder when it is set and not empty. It is for the cases
/// where the library is not next to the program: running the tests, or
/// `cargo run`, where the program sits deep in the build folder.
///
/// The folder is not checked here: [`Renderer::open`] says whether the
/// library is in it.
pub fn default_library_folder() -> Result<PathBuf, RenderError> {
    library_folder_from(
        std::env::var_os(PDFIUM_DIR_VARIABLE),
        std::env::current_exe(),
    )
}

/// The rule of [`default_library_folder`], with its two inputs passed in:
/// the value of the variable, and the path of the running program.
///
/// A function that reads the environment by itself can only be tested by
/// changing the environment, which every thread of the process shares (in
/// this edition of Rust, `std::env::set_var` is `unsafe` for that reason).
/// Passing the values in makes the rule testable with no global state.
fn library_folder_from(
    variable: Option<OsString>,
    executable: io::Result<PathBuf>,
) -> Result<PathBuf, RenderError> {
    // `filter` turns "set, but to nothing" into "not set".
    if let Some(folder) = variable.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(folder));
    }

    let unknown = |source| RenderError::LibraryFolderUnknown { source };
    let executable = executable.map_err(unknown)?;
    match executable.parent() {
        Some(folder) => Ok(folder.to_path_buf()),
        // Only a path that is a bare root has no parent, and a program is
        // never that. Still an error and not a panic.
        None => Err(unknown(io::Error::other(
            "the path of the program has no folder",
        ))),
    }
}

/// The size in pixels of the image of a page that is `page_width` by
/// `page_height` points, drawn `width` pixels wide: the rules listed on
/// [`Renderer::render_page`]. `None` if the page has no usable size.
///
/// It is a function of numbers only, apart from the engine, so the rules
/// about sizes are tested without drawing anything.
fn image_size(page_width: f32, page_height: f32, width: u32) -> Option<(u32, u32)> {
    // Not a number, infinite, zero or negative: nothing can be drawn.
    let usable = |side: f32| side.is_finite() && side > 0.0;
    if !usable(page_width) || !usable(page_height) {
        return None;
    }

    // `f64::from` and not `as`: it only exists for conversions that cannot
    // lose anything, so it documents itself. An `f64` holds every `f32`
    // and every `u32` exactly.
    let shape = f64::from(page_height) / f64::from(page_width);
    let height = (f64::from(width) * shape).round();
    if height <= f64::from(MAX_RENDER_HEIGHT) {
        // `as` from a float to an integer cuts the decimals and stops at
        // the ends of the integer's range, so it cannot overflow.
        return Some((width, (height as u32).max(1)));
    }

    let narrower = (f64::from(MAX_RENDER_HEIGHT) / shape).round();
    Some(((narrower as u32).max(1), MAX_RENDER_HEIGHT))
}

/// Writes `image` at `path` as a PNG file: 8 bits per channel, with alpha.
/// PNG compresses without losing anything, so the file holds exactly the
/// pixels of `image`.
///
/// It follows the rules of every save in this crate:
///
/// - The file is written under a temporary name and renamed into place, so
///   `path` never names half a PNG, not even if the disk fills up halfway.
/// - It waits for the disk before the rename, as a merge does and a split
///   does not. It is one file, so the wait is a few milliseconds, and it
///   buys two things: a failure to get the bytes onto the disk is reported
///   as a failed save, and a file that this one replaces is never lost to
///   an incomplete one in a power cut.
/// - A file that is already at `path` is replaced without a question.
///   Whether replacing is allowed is the front end's rule, checked before
///   it calls the core (`name_is_taken`), as for a merge.
///
/// An `image` whose `rgba` does not hold `width * height * 4` bytes is
/// refused before anything is written.
pub fn write_png(image: &PageImage, path: &Path) -> Result<(), RenderError> {
    let failed = |source| RenderError::Save {
        path: path.to_path_buf(),
        source,
    };

    // `checked_mul` returns `None` instead of wrapping around when the
    // product does not fit: the fields are public, so the sizes can be
    // anything.
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if expected != Some(image.rgba.len()) {
        return Err(failed(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the image says {} by {} pixels and holds {} bytes",
                image.width,
                image.height,
                image.rgba.len()
            ),
        )));
    }

    write_atomically(path, Durability::Synced, |writer| {
        let mut encoder = png::Encoder::new(writer, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // The encoder has its own error type. `?` converts it into the
        // `io::Error` this closure returns, through the `From` that the
        // `png` crate provides for exactly that.
        let mut png = encoder.write_header()?;
        png.write_image_data(&image.rgba)?;
        // Writes the last chunk of the file. Dropping `png` would do it
        // too, and would swallow an error in doing so.
        png.finish()?;
        Ok(())
    })
    .map_err(failed)
}
