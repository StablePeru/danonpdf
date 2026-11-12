//! The Pages view: drawing, and the two things that drawing owns.
//!
//! Everything here reads a [`Thumbnails`] to draw it, and turns what the
//! user does into calls on it. Which file is taken, which pages are asked
//! for, which are forgotten and what the status says are decided in
//! `thumbnails.rs`, where they are tested without a window. `app.rs`
//! explains how egui redraws from state on every frame.
//!
//! Two things live here and not in the state, because both only exist
//! once there is a window:
//!
//! - The worker thread (`thumbnail_worker.rs`). It is started the first
//!   time there is something to ask it, with a way to wake this window
//!   that is made from egui's `Context`.
//! - The textures. A texture is a picture in the memory of the graphics
//!   card, which is where egui draws from. The worker sends plain pixels;
//!   each image is handed to egui once, when it arrives, and from then on
//!   a frame only says "draw texture N here".
//!
//! None of this has a test: it needs a real window. It is kept thin for
//! that reason, and every number it uses comes from a function of the
//! state that does have one.

use std::collections::BTreeMap;

use eframe::egui;

use crate::dialogs;
use crate::drops;
use crate::paths::display_name;
use crate::texts;
use crate::thumbnail_worker::{self, Worker};
use crate::thumbnails::{
    Document, Slot, Status, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH, Thumbnails, columns_for, fit_within,
    row_count, thumbnail_size,
};

/// The space between a thumbnail and the page number under it, in egui
/// points.
const LABEL_GAP: f32 = 4.0;

/// The space left free around the text shown in the place of a thumbnail.
const PLACEHOLDER_MARGIN: f32 = 8.0;

/// What the Pages view keeps besides its state: the worker thread and the
/// textures. `Default` has neither, so the app can be built without a
/// window, as the tests do.
#[derive(Default)]
pub struct ThumbnailsView {
    /// `None` until the first request, and again after a worker died.
    worker: Option<Worker>,
    /// The texture of every page that the state holds as `Slot::Ready`,
    /// by page number.
    ///
    /// A `TextureHandle` frees its texture when it is dropped, so removing
    /// an entry from this map is all it takes to give the memory back.
    textures: BTreeMap<usize, egui::TextureHandle>,
}

impl ThumbnailsView {
    /// Passes requests and replies between `state` and the worker thread,
    /// starting the thread first if there is something to send and no
    /// thread yet. It never waits.
    ///
    /// The app calls it twice in a frame that shows this view: before
    /// drawing, so the frame draws what has arrived, and after, so what
    /// the frame asked for goes out at once.
    pub fn exchange(&mut self, ctx: &egui::Context, state: &mut Thumbnails) {
        if self.worker.is_none() && state.has_requests() {
            // The worker's way to wake this window. `Context` is a handle
            // that can be cloned and given to another thread; asking for
            // a repaint from there makes egui draw a frame, in which the
            // call above this one finds the reply. Without it the reply
            // would wait in the channel until the mouse moved.
            let ctx = ctx.clone();
            let wake = Box::new(move || ctx.request_repaint());
            match Worker::spawn(
                state.generation().clone(),
                thumbnail_worker::load_pdfium,
                wake,
            ) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    state.worker_did_not_start(&error);
                    return;
                }
            }
        }

        // A "let chain": there is a worker, and the exchange found it
        // gone. Dropping the handle of a thread that has ended waits for
        // nothing; the next request starts a new one.
        if let Some(worker) = &self.worker
            && !state.exchange(worker)
        {
            self.worker = None;
        }
    }

    /// Draws the view into the central panel: the button and the name of
    /// the file on top, and under them the grid of thumbnails, which is
    /// also where a file can be dropped.
    pub fn show(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, state: &mut Thumbnails) {
        self.update_textures(ui.ctx(), state);

        ui.horizontal(|ui| {
            // A "let chain": the dialog only opens when the button was
            // clicked, and the block only runs when the dialog returned a
            // file (`None` means the user cancelled).
            if ui
                .button(texts::CHOOSE_BUTTON)
                .on_hover_text(texts::PAGES_CHOOSE_HINT)
                .clicked()
                && let Some(path) = dialogs::pick_pdf(frame)
            {
                state.choose([path]);
            }
            if let Document::Open { path, pages } = state.document() {
                // The page count first, so a long name is the part that
                // is cut short with "…". Hovering it shows the full path.
                ui.weak(texts::page_count(*pages));
                ui.add(egui::Label::new(display_name(path)).truncate())
                    .on_hover_text(path.display().to_string());
            }
        });
        ui.add_space(4.0);

        let zone = drops::drop_zone(ui);
        egui::Frame::group(ui.style())
            .stroke(zone.border)
            .show(ui, |ui| {
                // Fill the whole panel, so the drop zone is the whole area
                // and not just the size of its contents.
                ui.set_min_size(ui.available_size());

                if zone.hovering {
                    ui.centered_and_justified(|ui| ui.label(texts::PAGES_DROP_NOW));
                    return;
                }
                // The page count is copied out of the state first: the
                // grid changes the state (it asks for pages), and a
                // `match` on `state.document()` would still be borrowing
                // it.
                match (state.pages(), state.document()) {
                    (Some(pages), _) => self.grid(ui, state, pages),
                    (None, Document::Opening { path }) => {
                        let text = texts::opening(&display_name(path));
                        ui.centered_and_justified(|ui| ui.weak(text));
                    }
                    (None, _) => {
                        ui.centered_and_justified(|ui| ui.weak(texts::PAGES_DROP_HINT));
                    }
                }
            });
    }

    /// Brings the textures in line with the state: one for every page it
    /// holds as drawn, and none for any other.
    fn update_textures(&mut self, ctx: &egui::Context, state: &mut Thumbnails) {
        // First the textures of pages that the state no longer holds: the
        // file was replaced, or the page was forgotten to save memory.
        //
        // The order matters. A page number means another page once
        // another file is open, and its old texture must not survive. It
        // cannot: pages are only asked for by `grid` below, which runs
        // after this, so between closing a file and the first drawn page
        // of the next one there is always a frame that finds the state
        // empty here.
        self.textures
            .retain(|page, _| state.slot(*page) == Some(&Slot::Ready));

        // Then the images that arrived. Each is handed to egui once and
        // dropped: from here on the texture is the only copy.
        for (page, image) in state.take_fresh() {
            let size = [image.width as usize, image.height as usize];
            // The core draws on opaque white paper, so "unmultiplied"
            // and "premultiplied" alpha are the same bytes here.
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
            // The name is only for debugging. `LINEAR` smooths the
            // picture when it is shown at a size that is not exactly its
            // own, which happens when a page does not have the shape of
            // the box.
            let texture =
                ctx.load_texture(format!("page-{page}"), pixels, egui::TextureOptions::LINEAR);
            self.textures.insert(page, texture);
        }
    }

    /// The grid of thumbnails of a document of `pages` pages.
    fn grid(&self, ui: &mut egui::Ui, state: &mut Thumbnails, pages: usize) {
        let spacing = ui.spacing().item_spacing;
        let label_height = ui.text_style_height(&egui::TextStyle::Body);
        // A cell is the box of the thumbnail with the page number under
        // it. Every cell has the same size, whatever the shape of its
        // page, so the grid can be laid out with arithmetic alone.
        let cell = egui::vec2(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT + LABEL_GAP + label_height);
        let columns = columns_for(ui.available_width(), cell.x, spacing.x);
        let rows = row_count(pages, columns);
        let size = thumbnail_size(ui.pixels_per_point());
        // The columns are centred in the room there is.
        let used = columns as f32 * (cell.x + spacing.x) - spacing.x;
        let left = ((ui.available_width() - used) / 2.0).max(0.0);

        // `show_rows` is a scroll area for a long list of rows of one
        // height: it works out which rows are on screen and only asks
        // for those to be drawn, so a document of a thousand pages draws
        // a dozen thumbnails per frame.
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            cell.y,
            rows,
            |ui, visible| {
                // This is where the state learns what is on screen, and
                // asks the worker for the pages that are missing.
                state.show_rows(visible.clone(), columns, size);

                // The top left corner of the first row on screen.
                let origin = ui.max_rect().left_top() + egui::vec2(left, 0.0);
                for row in visible.clone() {
                    for column in 0..columns {
                        let page = row * columns + column + 1;
                        if page > pages {
                            break;
                        }
                        let offset = egui::vec2(
                            column as f32 * (cell.x + spacing.x),
                            (row - visible.start) as f32 * (cell.y + spacing.y),
                        );
                        let rect = egui::Rect::from_min_size(origin + offset, cell);
                        self.cell(ui, rect, page, state.slot(page));
                    }
                }
            },
        );
    }

    /// One cell of the grid, drawn in `rect`: the thumbnail of `page`, or
    /// what stands in for it, and the page number under it.
    fn cell(&self, ui: &mut egui::Ui, rect: egui::Rect, page: usize, slot: Option<&Slot>) {
        let frame =
            egui::Rect::from_min_size(rect.min, egui::vec2(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT));
        let label = egui::Rect::from_min_max(
            egui::pos2(rect.left(), frame.bottom() + LABEL_GAP),
            rect.max,
        );
        let border = ui.visuals().widgets.noninteractive.bg_stroke;
        let outline = |ui: &egui::Ui, rect: egui::Rect| {
            ui.painter()
                .rect_stroke(rect, 0.0, border, egui::StrokeKind::Inside);
        };

        // A tuple of the two things that decide what is drawn: what the
        // state knows about the page, and whether its texture is here.
        match (slot, self.textures.get(&page)) {
            (Some(Slot::Ready), Some(texture)) => {
                // The picture keeps its shape: it is fitted into the box
                // and centred there, with a line around it, since paper
                // is white and so is a light window.
                let (width, height) = fit_within(texture.size(), THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
                let picture =
                    egui::Rect::from_center_size(frame.center(), egui::vec2(width, height));
                egui::Image::new(texture).paint_at(ui, picture);
                outline(ui, picture);
            }
            (Some(Slot::Failed { reason }), _) => {
                // The page keeps its place, so the pages after it do not
                // move, and says why it is empty.
                outline(ui, frame);
                let inside = frame.shrink(PLACEHOLDER_MARGIN);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(inside)
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                    |ui| {
                        // A long reason is cut off at the edge of the box
                        // instead of running over the next row; hovering
                        // shows all of it.
                        ui.set_clip_rect(inside.intersect(ui.clip_rect()));
                        ui.label(texts::PAGE_NOT_DRAWN);
                        ui.add(egui::Label::new(egui::RichText::new(reason).weak().small()).wrap())
                            .on_hover_text(reason);
                    },
                );
            }
            // Asked for and not here yet, or not asked for yet: both are
            // an empty box for now.
            _ => {
                outline(ui, frame);
                ui.put(
                    frame,
                    egui::Label::new(egui::RichText::new(texts::PAGE_WAITING).weak()),
                );
            }
        }
        // `put` draws a widget inside a rectangle that the caller chose,
        // centred, instead of at the next free place.
        ui.put(label, egui::Label::new(texts::page_label(page)));
    }
}

/// The line at the bottom of the window while this view is shown.
pub fn status_line(ui: &mut egui::Ui, state: &Thumbnails) {
    let error = ui.visuals().error_fg_color;
    // A `match` must cover every variant of `Status`. If a new kind of
    // status is added later, this stops compiling until it is drawn.
    match state.status() {
        Status::Idle => {
            // With nothing to report, the line says where the document
            // is.
            match state.document() {
                Document::None => ui.weak(texts::NO_FILE_CHOSEN),
                Document::Opening { path } => ui.weak(texts::opening(&display_name(path))),
                Document::Open { path, pages } => {
                    ui.weak(texts::pages_shown(&display_name(path), *pages))
                }
            };
        }
        Status::NoPdf { not_pdf } => {
            // The same words as in the other views, for the same thing.
            ui.label(texts::skipped(*not_pdf, 0));
        }
        Status::LeftOut {
            other_pdfs,
            not_pdf,
        } => {
            // This status is only set together with a file. While the
            // file is being opened, or after it failed to open, there
            // may be none to name.
            let name = match state.document() {
                Document::Opening { path } | Document::Open { path, .. } => display_name(path),
                Document::None => String::new(),
            };
            ui.label(texts::left_out(&name, *other_pdfs, *not_pdf));
        }
        Status::NotAccepted { file, message } => {
            ui.colored_label(error, texts::not_accepted(&display_name(file), message))
                .on_hover_text(file.display().to_string());
        }
        Status::NoEngine { message } => {
            ui.colored_label(error, texts::no_engine(message));
        }
        Status::WorkerStopped => {
            ui.colored_label(error, texts::WORKER_STOPPED);
        }
        Status::WorkerNotStarted { message } => {
            ui.colored_label(error, texts::worker_not_started(message));
        }
    }
}
