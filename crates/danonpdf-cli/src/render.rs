//! The render operation: draw one page of a PDF file and write it as a PNG
//! image.
//!
//! Drawing is the core's business, and the core leaves it to PDFium, a
//! library that is loaded from a file when the program runs (`pdfium.dll`,
//! `libpdfium.so`). This is the only operation that needs that file: the
//! other four never ask for it, so they work without it.
//!
//! What this module adds is the same two rules as every operation that
//! writes, and the choice of where to look for the library.

use std::num::NonZeroUsize;
use std::path::Path;

use danonpdf_core::{PageImage, RenderError, Renderer, default_library_folder, write_png};

use crate::outcome::{Failure, Report, check_outputs, render_failure_from};

/// Draws page `page` of `input` (counted from 1), `width` pixels wide, and
/// writes the image as a PNG file at `output`.
///
/// `output` must not be `input`, whatever `force` says, and must not exist
/// yet unless `force` is true. Both are checked before the library is even
/// looked for, so a command line that breaks a rule gets the same answer on
/// a computer without PDFium.
///
/// The report gives the size of the image that was written, which is not
/// always the width that was asked for: see `Renderer::render_page` in the
/// core.
pub fn run(
    input: &Path,
    page: NonZeroUsize,
    width: u32,
    output: &Path,
    force: bool,
) -> Result<Report, Failure> {
    check_outputs(&[input], &[output], force)?;

    // Two steps, each with its own way to fail, and one place that gives
    // either failure its code. The image is whole in memory before the
    // first byte is written, so a page that cannot be drawn leaves no file.
    let image = draw(input, page, width).map_err(|error| render_failure_from(&error, input))?;
    write_png(&image, output).map_err(|error| render_failure_from(&error, input))?;

    Ok(Report::Render {
        output: output.to_path_buf(),
        page: page.get(),
        width: image.width,
        height: image.height,
    })
}

/// Loads PDFium and draws the page.
///
/// A function of its own so that its three steps can each end with `?`:
/// all three fail with a `RenderError`, and `run` turns whichever it was
/// into a failure in one place.
fn draw(input: &Path, page: NonZeroUsize, width: u32) -> Result<PageImage, RenderError> {
    // The folder of this program, or the one that the environment variable
    // `DANONPDF_PDFIUM_DIR` names. The core owns that rule, so the window
    // will look in the same place.
    let library_folder = default_library_folder()?;
    let renderer = Renderer::open(&library_folder)?;
    renderer.render_page(input, page, width)
}
