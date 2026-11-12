//! The thread that draws thumbnails, so that the window never waits for
//! PDFium.
//!
//! Drawing a page takes PDFium anything from a millisecond to a second, and
//! the thread that draws the window must hand over a frame every few
//! milliseconds: while it is busy with anything else, the window does not
//! answer. So every call into PDFium that the Pages view needs (loading the
//! library, counting the pages of a file, drawing one) is made on a second
//! thread, the worker, and the two threads only ever talk through two
//! channels:
//!
//! ```text
//!   window thread                                worker thread
//!   -------------                                -------------
//!   Worker::send(Request)  --- requests --->     waits in recv(), then PDFium
//!   Worker::try_recv()     <--- replies ----     Reply, then wake()
//! ```
//!
//! A channel is a queue with two ends. What one thread sends, it gives
//! away: the value is moved into the channel, and from then on only the
//! thread that receives it can touch it. Nothing is shared, so there is
//! nothing to lock. The one thing the two threads do share is a number,
//! the [`Generation`], explained on that type.
//!
//! Nothing here knows egui. The worker tells the window that a reply is
//! waiting by calling a function it was given ([`Wake`]); the window gives
//! it one that asks egui for a new frame, and a test gives it one that
//! sends a message. And nothing here knows PDFium either: the worker draws
//! with any [`PageEngine`], which is the core's `Renderer` in the program
//! and a fake in most tests.

use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SendError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use danonpdf_core::{
    MAX_RENDER_HEIGHT, MAX_RENDER_WIDTH, PageImage, RenderError, Renderer, default_library_folder,
};

/// What the worker needs from whatever draws pages: the two questions the
/// Pages view asks about a file.
///
/// A trait says what a type can do without saying which type it is. The
/// worker is written against this one, so a test can hand it an engine
/// that needs no PDFium, answers at once, or waits until the test lets it
/// go on.
pub trait PageEngine {
    /// How many pages the file at `path` has.
    fn page_count(&self, path: &Path) -> Result<usize, RenderError>;

    /// Page `page` of the file at `path`, drawn `width` pixels wide.
    fn render_page(
        &self,
        path: &Path,
        page: NonZeroUsize,
        width: u32,
    ) -> Result<PageImage, RenderError>;
}

/// The real engine: PDFium, through the core.
impl PageEngine for Renderer {
    fn page_count(&self, path: &Path) -> Result<usize, RenderError> {
        // `Renderer::page_count` and not `self.page_count`: both the type
        // and this trait have a method of that name. Written this way it
        // says which one is meant, and cannot turn into a call to itself.
        Renderer::page_count(self, path)
    }

    fn render_page(
        &self,
        path: &Path,
        page: NonZeroUsize,
        width: u32,
    ) -> Result<PageImage, RenderError> {
        Renderer::render_page(self, path, page, width)
    }
}

/// Loads PDFium from the folder the program is in (or the one the
/// environment variable names: see the core's `default_library_folder`).
///
/// This is what the program gives [`Worker::spawn`] as the way to get an
/// engine, so the library is loaded on the worker thread, the first time a
/// file is opened in the Pages view. A window that never shows that view
/// never loads it, and Merge and Split work without the file being there.
pub fn load_pdfium() -> Result<Renderer, RenderError> {
    Renderer::open(&default_library_folder()?)
}

/// A number that says which requests are still wanted. The window thread
/// raises it; the worker thread reads it.
///
/// Every request carries the generation it was made in. Whenever the
/// window stops wanting what it has asked for (another file was chosen,
/// the view was left, the pages asked for scrolled out of sight), it moves
/// on to the next generation. Two things follow:
///
/// - The worker compares each request it takes from the queue with the
///   current generation, and drops the ones from an older one without
///   calling PDFium. So a long queue of pages nobody will look at costs
///   nothing, and the pages that are wanted now are not kept waiting.
/// - The window ignores every reply from an older generation. The worker
///   may have been in the middle of a page when the generation moved on,
///   and that page still arrives.
///
/// Requests cannot be taken back out of a channel, which is why they are
/// cancelled this way, by a number on the side.
///
/// # How two threads share it
///
/// `Arc` is a pointer that counts its owners: `clone` gives one more
/// owner of the same number, not a copy of it, and the number is freed
/// when the last owner is dropped. Each thread holds one.
///
/// What the `Arc` points at is an `AtomicU64`. A plain `u64` could not be
/// changed through an `Arc`, which only lends its content for reading, and
/// if it could, one thread might read it while the other is halfway
/// through writing it. An atomic is a number that is read and written in
/// one step that cannot be interrupted, so it can be changed through a
/// shared reference, with no lock.
#[derive(Debug, Clone, Default)]
pub struct Generation(Arc<AtomicU64>);

impl Generation {
    /// The generation that requests are wanted for now.
    pub fn current(&self) -> u64 {
        // `SeqCst` is the strictest of the orderings an atomic offers:
        // every thread sees the changes in one same order. The weaker ones
        // are faster on some processors and harder to reason about;
        // nothing here runs often enough for the difference to matter.
        self.0.load(Ordering::SeqCst)
    }

    /// Moves on to the next generation and returns it. Everything asked
    /// for before this call is no longer wanted.
    pub fn advance(&self) -> u64 {
        // `fetch_add` adds and returns the value from before, as one
        // step: two threads advancing at once would still get two
        // different numbers.
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// The largest image a thumbnail may be, in pixels of the screen.
///
/// A page is drawn [`ThumbnailSize::width`] pixels wide, and its height
/// follows from its shape. A page so tall that it would pass
/// [`ThumbnailSize::max_height`] is drawn narrower instead, so that every
/// thumbnail fits in a box of this size whatever the shape of its page.
/// That is what puts a limit on the memory one thumbnail takes: without
/// it, a page shaped like a till receipt would be a strip thousands of
/// pixels long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailSize {
    pub width: u32,
    pub max_height: u32,
}

impl ThumbnailSize {
    /// A size of `width` by `max_height` pixels, brought inside what the
    /// core draws: each side at least 1 pixel, and no more than the
    /// largest image of the core.
    pub fn new(width: u32, max_height: u32) -> Self {
        Self {
            width: width.clamp(1, MAX_RENDER_WIDTH),
            max_height: max_height.clamp(1, MAX_RENDER_HEIGHT),
        }
    }
}

/// What the window asks the worker to do.
///
/// Both kinds carry the path: the worker keeps nothing from one request to
/// the next (the core opens and closes the file in every call), so the
/// order of the requests does not matter and any of them can be skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open the file and say how many pages it has.
    Open { generation: u64, path: PathBuf },
    /// Draw one page of the file, to fit in `size`.
    Page {
        generation: u64,
        path: PathBuf,
        page: NonZeroUsize,
        size: ThumbnailSize,
    },
}

impl Request {
    /// The generation this request was made in.
    pub fn generation(&self) -> u64 {
        // `|` joins two patterns into one arm: both bind `generation` to
        // the same type.
        match self {
            Self::Open { generation, .. } | Self::Page { generation, .. } => *generation,
        }
    }
}

/// What the worker answers: one reply for every request it did not skip.
///
/// A reply carries the generation of its request, so the window can tell
/// an answer it still waits for from one that comes too late.
///
/// The error is the core's own type and not a text: the window decides
/// what to say about it, and it says different things about a library
/// that is missing and about a file that cannot be read.
#[derive(Debug)]
pub enum Reply {
    /// The answer to [`Request::Open`]: the page count of the file.
    Opened {
        generation: u64,
        result: Result<usize, RenderError>,
    },
    /// The answer to [`Request::Page`]: the image of the page.
    Page {
        generation: u64,
        page: NonZeroUsize,
        result: Result<PageImage, RenderError>,
    },
}

/// How the worker tells the window that a reply is waiting.
///
/// The window thread sleeps until something asks for a frame, so a reply
/// could sit in the channel until the mouse happens to move. The worker
/// calls this after every reply it sends.
///
/// `dyn Fn()` is "some function that takes nothing and returns nothing,
/// whichever it is": a closure's own type has no name, so it is kept
/// behind a pointer (`Box`) and called through it. `+ Send` is part of the
/// type on purpose. The function is handed to another thread, and Rust
/// only lets a value cross to another thread if its type is `Send`.
/// Without it, the compiler has to assume that the closure may hold
/// something tied to this thread, and refuses to move it (E0277).
pub type Wake = Box<dyn Fn() + Send>;

/// How long closing the window waits for the worker thread to end.
///
/// An idle worker ends at once, and one that is drawing a page ends when
/// the page is done. A slow page takes PDFium about a second (see the top
/// of this file), so two seconds cover an ordinary close with room to
/// spare, and the thread is gone before the program is. Only a call that
/// has stalled lasts longer (a file on a network share that stopped
/// answering, a page PDFium cannot get through), and for that one the
/// wait must end: a program that stays behind, out of sight, after its
/// window was closed is worse than a thread cut short. Two seconds is
/// also about as long as a window that was told to close can stay on
/// screen before it looks stuck.
pub const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// How [`Worker::end`] left the worker thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// The thread has ended: nothing of it is left.
    Ended,
    /// The thread was still inside a call when the wait ran out, and was
    /// left to finish by itself.
    LeftRunning,
}

/// The window's handle on the worker thread: the two ends of the channels
/// that the window holds, and the thread itself.
///
/// Dropping it ends the thread: see the `Drop` at the bottom of this file.
pub struct Worker {
    /// `Option` only so that `end` can let go of the sender before it
    /// waits for the thread. It is `Some` until then.
    requests: Option<Sender<Request>>,
    replies: Receiver<Reply>,
    generation: Generation,
    /// `Option` for the same reason: `JoinHandle::join` takes the handle
    /// by value, and `end` only has `&mut self`.
    thread: Option<JoinHandle<()>>,
    /// How the window learns that the thread has ended, without waiting
    /// for it longer than it wants to. Nothing is ever sent through this
    /// channel. The thread holds the sending end for as long as it runs,
    /// and a channel reports that its last sender is gone: that report is
    /// the signal. It comes however the thread ends, by a panic too, since
    /// a panic drops everything the thread owned on its way out.
    ///
    /// `JoinHandle::join` cannot do this: it waits for as long as it
    /// takes, with no way to give up.
    ended: Receiver<()>,
}

impl Worker {
    /// Starts the worker thread.
    ///
    /// - `generation`: the number the window raises to cancel what it
    ///   asked for. The worker gets one more owner of the same number.
    /// - `load_engine`: how to get the engine. It is called on the worker
    ///   thread, when the first request arrives, and again for a later
    ///   request if it failed: the library may have been put in place in
    ///   the meantime.
    /// - `wake`: called on the worker thread after every reply.
    ///
    /// # What the bounds say
    ///
    /// `L: Send + 'static`: the function that loads the engine is moved to
    /// the new thread, so it must be allowed to cross (`Send`), and it must
    /// not borrow anything that could go away while the thread still runs
    /// (`'static`): a thread started this way can outlive the function
    /// that started it.
    ///
    /// The engine itself (`E`) needs neither. It is made on the worker
    /// thread and never leaves it: what crosses is the function that makes
    /// it, not the engine. So an engine that could not be sent to another
    /// thread would still do, as long as it can be made there.
    ///
    /// Fails only if the system cannot start a thread.
    pub fn spawn<E, L>(generation: Generation, load_engine: L, wake: Wake) -> io::Result<Self>
    where
        E: PageEngine,
        L: Fn() -> Result<E, RenderError> + Send + 'static,
    {
        // Two channels, one for each direction. `channel()` returns the
        // two ends of a new queue: the window keeps the sending end of
        // the requests and the receiving end of the replies, and the
        // thread gets the other two.
        let (request_sender, request_receiver) = mpsc::channel();
        let (reply_sender, reply_receiver) = mpsc::channel();
        // A third one, only to learn that the thread has ended: see the
        // field `ended`.
        let (ended_sender, ended) = mpsc::channel();
        let for_the_thread = generation.clone();

        // `move` gives the closure everything it names: both channel ends,
        // its own owner of the generation, and the two functions. They are
        // locals of this function, gone when it returns, while the thread
        // goes on, so the thread has to own them and not borrow them.
        //
        // This closure would take them even without the word, because
        // `run` takes each of them by value, and a closure takes ownership
        // of what its body uses up. A closure that only reads a local (say
        // `for_the_thread.current()`) borrows it instead, and the compiler
        // then refuses to start a thread with it (E0373). `move` says what
        // is meant, whatever the body does.
        //
        // `Builder` and not `thread::spawn`: it names the thread (the name
        // shows up in a debugger and in the message of a panic), and it
        // returns an error where `thread::spawn` would panic.
        let thread = thread::Builder::new()
            .name("thumbnails".to_owned())
            .spawn(move || {
                // Never used, only owned. A local is dropped when its
                // block ends, and `run` has returned by then, having let
                // go of the engine and of everything else it was given. So
                // this sender is the last thing the thread drops, and the
                // window reads "this sender is gone" as "the thread has
                // ended".
                let _until_the_thread_ends = ended_sender;
                run(
                    request_receiver,
                    reply_sender,
                    for_the_thread,
                    load_engine,
                    wake,
                );
            })?;

        Ok(Self {
            requests: Some(request_sender),
            replies: reply_receiver,
            generation,
            thread: Some(thread),
            ended,
        })
    }

    /// Queues `request` for the worker. It never waits: the queue has no
    /// limit, and the request is handled when the worker gets to it.
    ///
    /// Fails only if the worker thread has ended (it panicked), and hands
    /// the request back inside the error.
    pub fn send(&self, request: Request) -> Result<(), SendError<Request>> {
        match &self.requests {
            Some(requests) => requests.send(request),
            // Not reached: the sender is only taken out in `end`, which
            // only `drop` calls.
            None => Err(SendError(request)),
        }
    }

    /// The next reply, if one is waiting. It never waits either:
    /// `TryRecvError::Empty` means that nothing has arrived yet, and
    /// `TryRecvError::Disconnected` that nothing ever will, because the
    /// worker thread has ended.
    pub fn try_recv(&self) -> Result<Reply, TryRecvError> {
        self.replies.try_recv()
    }

    /// Waits until the worker thread has ended, and says whether it ended
    /// by itself (`Ok`) or by a panic (`Err`). For tests, which need to
    /// know that a thread is gone before they look at what it left.
    #[cfg(test)]
    pub fn join(&mut self) -> thread::Result<()> {
        match self.thread.take() {
            Some(thread) => thread.join(),
            None => Ok(()),
        }
    }
}

impl Worker {
    /// Ends the worker thread, and waits for it to be gone for no longer
    /// than `wait`. Three steps, in this order:
    ///
    /// 1. The generation moves on, so every request still in the queue is
    ///    skipped instead of drawn. A channel hands over everything that
    ///    was sent before it reports that the sender is gone; without
    ///    this, closing the window would wait for a queue of pages.
    /// 2. The sender is dropped. That is the signal: with no sender left,
    ///    the `recv` the worker waits in returns an error, and its loop
    ///    ends. No "please stop" message is needed.
    /// 3. The window waits until the thread has ended (see the field
    ///    `ended`), or until `wait` is over.
    ///
    /// # Why it waits
    ///
    /// When `main` returns, the system starts taking the program apart,
    /// its libraries included, without asking the other threads first. A
    /// thread that is inside PDFium at that moment is a way for a program
    /// to crash on its way out, and waiting for the page it is drawing
    /// avoids the question.
    ///
    /// # Why it does not wait for ever
    ///
    /// Step 1 only cancels what is in the queue. A call into PDFium that
    /// has started cannot be interrupted, and it reads a file: on a
    /// network share that stopped answering, that read may not come back
    /// for minutes. Waiting for it would keep the program running with its
    /// window already closed.
    ///
    /// So after `wait` the thread is let go: its handle is dropped, which
    /// leaves the thread running with nobody waiting for it. That is safe:
    ///
    /// - The thread borrows nothing from the window. It owns its ends of
    ///   the channels, its own owner of the generation and the engine, so
    ///   nothing it uses goes away while it runs.
    /// - It only reads files. It holds none open for writing, so no file
    ///   of the user's can be left half written, whenever the thread is
    ///   cut off.
    /// - If the program is closing, the system ends the thread with it.
    ///   The crash on the way out that waiting avoids is possible again
    ///   for this one stalled call; there is no work left for it to lose.
    /// - If the program goes on (a test, or a worker that the window
    ///   replaces), the thread ends by itself when its call returns: the
    ///   window is not listening for the reply, the request channel is
    ///   closed, and its loop ends.
    fn end(&mut self, wait: Duration) -> Ending {
        // `take` leaves `None` behind and hands over the handle. No handle
        // means that the thread was already waited for.
        let Some(thread) = self.thread.take() else {
            return Ending::Ended;
        };
        self.generation.advance();
        // The sender is dropped at the end of this statement.
        drop(self.requests.take());

        // `recv_timeout` waits for a message, for the last sender to go
        // away or for the time to run out, whichever comes first. No
        // message is ever sent, so `Ok` is listed only because the type
        // has it.
        match self.ended.recv_timeout(wait) {
            // The thread dropped its sender, which is the last thing it
            // does: `join` returns at once, and is only called so that
            // nothing of the thread is left behind. It gives an error if
            // the thread panicked, and there is nothing left to do about
            // that here.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                Ending::Ended
            }
            // Still inside a call. `thread` is dropped here, unjoined:
            // that is all it takes to let a thread go.
            Err(RecvTimeoutError::Timeout) => Ending::LeftRunning,
        }
    }
}

impl Drop for Worker {
    /// Ends the worker thread, waiting for it for [`CLOSE_WAIT`] at most.
    ///
    /// The window drops its `Worker` when the program closes (eframe drops
    /// the app, and the `Worker` with it), and when it finds that the
    /// thread has ended by itself. [`Worker::end`] says what happens and
    /// why.
    fn drop(&mut self) {
        self.end(CLOSE_WAIT);
    }
}

/// What the worker thread does, from its start to its end: take a request,
/// answer it, wake the window, and again.
///
/// It is a function of its own, and not the body of the closure in
/// [`Worker::spawn`], so that its parameters say exactly what the thread
/// owns.
fn run<E: PageEngine>(
    requests: Receiver<Request>,
    replies: Sender<Reply>,
    wanted: Generation,
    load_engine: impl Fn() -> Result<E, RenderError>,
    wake: Wake,
) {
    // The engine, once it is loaded. It lives in this variable, on this
    // thread, until the thread ends.
    let mut engine: Option<E> = None;

    // `recv` puts the thread to sleep until a request arrives, so an idle
    // worker uses no processor time. It returns an error once the queue is
    // empty and every sender is gone: the window has dropped its `Worker`,
    // and the loop ends.
    while let Ok(request) = requests.recv() {
        // Asked for in an older generation: nobody waits for this one.
        // Checked here, at the last moment before the work, and not when
        // the request was sent: what matters is whether it is still
        // wanted now.
        if request.generation() != wanted.current() {
            continue;
        }

        let reply = match request {
            Request::Open { generation, path } => Reply::Opened {
                generation,
                result: loaded(&mut engine, &load_engine)
                    .and_then(|engine| engine.page_count(&path)),
            },
            Request::Page {
                generation,
                path,
                page,
                size,
            } => Reply::Page {
                generation,
                page,
                result: loaded(&mut engine, &load_engine)
                    .and_then(|engine| thumbnail(engine, &path, page, size)),
            },
        };

        // Sending fails only when the receiver is gone, which means the
        // window is closing: stop here.
        if replies.send(reply).is_err() {
            break;
        }
        // After the reply is in the channel, never before: the frame that
        // this asks for must find it there.
        wake();
    }
}

/// The engine, loading it first if this thread has none yet.
///
/// A failure is not remembered: the next request tries again, so a library
/// that the user puts in place while the program runs is found the next
/// time a file is chosen.
fn loaded<'a, E>(
    engine: &'a mut Option<E>,
    load_engine: &impl Fn() -> Result<E, RenderError>,
) -> Result<&'a E, RenderError> {
    // `insert` puts the value in and returns a reference to it where it
    // now lives, which is what the caller needs.
    match engine {
        Some(engine) => Ok(engine),
        None => Ok(engine.insert(load_engine()?)),
    }
}

/// Page `page` of the file at `path`, as an image that fits in `size`.
///
/// The core is asked for a width, and the height follows from the page. A
/// page that comes out taller than the box is drawn a second time,
/// narrower by as much as it was too tall. The size of a page is not known
/// before it is drawn once, and such pages are rare, so drawing them twice
/// is cheaper than a second call in the core to ask for sizes.
fn thumbnail<E: PageEngine>(
    engine: &E,
    path: &Path,
    page: NonZeroUsize,
    size: ThumbnailSize,
) -> Result<PageImage, RenderError> {
    let image = engine.render_page(path, page, size.width)?;
    match narrower_width(&image, size.max_height) {
        Some(width) => engine.render_page(path, page, width),
        None => Ok(image),
    }
}

/// The width at which the page of `image` would be `max_height` pixels
/// high, if `image` is taller than that. `None` if it already fits.
fn narrower_width(image: &PageImage, max_height: u32) -> Option<u32> {
    if image.height <= max_height {
        return None;
    }
    // In `u64`, where the product of two `u32` always fits. The division
    // rounds down, so the new image is never taller than asked, and the
    // result is below `image.width`, so it fits back into a `u32`.
    let width = u64::from(image.width) * u64::from(max_height) / u64::from(image.height);
    Some((width as u32).max(1))
}

#[cfg(test)]
mod tests;
