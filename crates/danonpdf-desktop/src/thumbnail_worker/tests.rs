//! Tests for the worker thread. Every one of them starts a real thread and
//! talks to it through the real channels; what is fake, in all but the
//! last two, is the engine, so the tests need no PDFium and decide by
//! themselves when a page is "drawn".
//!
//! # No test here sleeps
//!
//! A test that waits "long enough" for another thread is wrong twice: it
//! is slow when the thread is fast, and it fails when the thread is slow.
//! These tests wait for the thing itself. The worker says that a reply is
//! ready by calling its wake function, and here that function sends a
//! message, so a test blocks in `recv` until the reply is really there.
//! The fake engine can be told to stop inside a call until the test lets
//! it go on, which is how a test gets to act "while a page is being
//! drawn". Every wait has a time limit ([`GUARD`]), but only so that a
//! broken worker fails the test instead of hanging the run: no test passes
//! because time went by.

use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use super::*;
use crate::test_support::ScratchDir;

/// The longest a test waits for the worker before it gives up and fails.
/// Never reached when the worker works.
const GUARD: Duration = Duration::from_secs(30);

/// A thumbnail box that an A4 page drawn 160 pixels wide fits in.
const SIZE: ThumbnailSize = ThumbnailSize {
    width: 160,
    max_height: 226,
};

/// One call that the fake engine received.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Count,
    Draw { page: usize, width: u32 },
}

/// An engine that draws nothing: it reports every call it gets, and makes
/// up its answers.
///
/// It is `Clone` because the worker asks a function for its engine, and a
/// function that can be called more than once cannot give its only copy
/// away. Each field is therefore something that can have several owners.
#[derive(Clone)]
struct FakeEngine {
    /// Every call is reported here, from the worker thread, so a test can
    /// see what reached the engine and in which order. A `Sender` can be
    /// cloned: a channel may have many senders.
    calls: Sender<Call>,
    /// When there is one, every `render_page` waits here for a message
    /// from the test before it answers.
    ///
    /// A `Receiver` cannot be cloned (a channel has one receiving end),
    /// and it cannot be used from two threads at once. `Arc` gives it
    /// several owners, and `Mutex` makes sure that only one of them
    /// receives at a time.
    gate: Option<Arc<Mutex<Receiver<()>>>>,
    /// How many pages every file "has".
    pages: usize,
    /// How many pixels high the page is drawn for every pixel of width.
    height_per_width: u32,
}

impl PageEngine for FakeEngine {
    fn page_count(&self, _path: &Path) -> Result<usize, RenderError> {
        // The test may have stopped listening; that is not the engine's
        // business.
        let _ = self.calls.send(Call::Count);
        Ok(self.pages)
    }

    fn render_page(
        &self,
        _path: &Path,
        page: NonZeroUsize,
        width: u32,
    ) -> Result<PageImage, RenderError> {
        let _ = self.calls.send(Call::Draw {
            page: page.get(),
            width,
        });
        if let Some(gate) = &self.gate {
            gate.lock()
                .unwrap()
                .recv_timeout(GUARD)
                .expect("the test should let the page through");
        }
        let height = width * self.height_per_width;
        Ok(PageImage {
            width,
            height,
            // Every byte is the page number, so a test can tell which
            // page an image is of.
            rgba: vec![page.get() as u8; (width * height * 4) as usize],
        })
    }
}

/// A worker with a fake engine, and the test's ends of everything that
/// comes out of it.
struct Harness {
    worker: Worker,
    generation: Generation,
    /// One message for every time the worker woke the window.
    woken: Receiver<()>,
    /// The calls that reached the engine.
    calls: Receiver<Call>,
    /// Lets one gated `render_page` through per message. `None` when the
    /// engine does not wait.
    gate: Option<Sender<()>>,
    /// How many times the worker asked for its engine.
    loads: Arc<AtomicUsize>,
}

impl Harness {
    /// A worker whose engine answers at once.
    fn new() -> Self {
        Self::with(false, 1)
    }

    /// A worker whose engine waits inside every `render_page` until
    /// [`Self::let_one_page_through`] is called.
    fn gated() -> Self {
        Self::with(true, 1)
    }

    fn with(gated: bool, height_per_width: u32) -> Self {
        let (calls_sender, calls) = mpsc::channel();
        let (woken_sender, woken) = mpsc::channel();
        let (gate, gate_receiver) = if gated {
            let (sender, receiver) = mpsc::channel();
            (Some(sender), Some(Arc::new(Mutex::new(receiver))))
        } else {
            (None, None)
        };
        let engine = FakeEngine {
            calls: calls_sender,
            gate: gate_receiver,
            pages: 3,
            height_per_width,
        };
        let loads = Arc::new(AtomicUsize::new(0));
        let loads_in_thread = Arc::clone(&loads);
        let generation = Generation::default();

        let worker = Worker::spawn(
            generation.clone(),
            move || {
                loads_in_thread.fetch_add(1, Ordering::SeqCst);
                Ok(engine.clone())
            },
            // The wake function of the tests: it tells the test thread,
            // through a channel, that a reply is waiting.
            Box::new(move || {
                let _ = woken_sender.send(());
            }),
        )
        .unwrap();

        Self {
            worker,
            generation,
            woken,
            calls,
            gate,
            loads,
        }
    }

    /// Asks for the page count of a file, in the current generation.
    fn open(&self) {
        self.worker
            .send(Request::Open {
                generation: self.generation.current(),
                path: PathBuf::from("a.pdf"),
            })
            .unwrap();
    }

    /// Asks for page `page`, in the current generation.
    fn draw(&self, page: usize) {
        self.worker
            .send(Request::Page {
                generation: self.generation.current(),
                path: PathBuf::from("a.pdf"),
                page: NonZeroUsize::new(page).unwrap(),
                size: SIZE,
            })
            .unwrap();
    }

    /// Waits until the worker wakes the window, and takes the reply that
    /// must be waiting by then.
    fn next_reply(&self) -> Reply {
        self.woken
            .recv_timeout(GUARD)
            .expect("the worker should wake the window after a reply");
        self.worker
            .try_recv()
            .expect("a reply should be in the channel before the window is woken")
    }

    /// Waits until the next call reaches the engine.
    fn next_call(&self) -> Call {
        self.calls
            .recv_timeout(GUARD)
            .expect("the engine should be called")
    }

    fn let_one_page_through(&self) {
        self.gate
            .as_ref()
            .expect("only a gated engine waits")
            .send(())
            .unwrap();
    }

    /// Whether nothing more has come out of the worker: no reply, no
    /// wake-up and no call to the engine.
    fn is_quiet(&self) -> bool {
        matches!(self.worker.try_recv(), Err(TryRecvError::Empty))
            && self.woken.try_recv().is_err()
            && self.calls.try_recv().is_err()
    }
}

/// The page number and the image of a reply to a page request that
/// worked. Fails the test for any other reply.
fn drawn(reply: Reply) -> (u64, usize, PageImage) {
    match reply {
        Reply::Page {
            generation,
            page,
            result: Ok(image),
        } => (generation, page.get(), image),
        other => panic!("expected a drawn page, got {other:?}"),
    }
}

// The protocol

#[test]
fn a_file_is_counted_and_a_page_is_drawn() {
    let harness = Harness::new();

    harness.open();
    let opened = harness.next_reply();
    harness.draw(2);
    let (generation, page, image) = drawn(harness.next_reply());

    assert!(
        matches!(
            opened,
            Reply::Opened {
                generation: 0,
                result: Ok(3)
            }
        ),
        "unexpected reply: {opened:?}"
    );
    assert_eq!((generation, page), (0, 2));
    assert_eq!((image.width, image.height), (160, 160));
    assert!(image.rgba.iter().all(|byte| *byte == 2));
    assert_eq!(harness.next_call(), Call::Count);
    assert_eq!(
        harness.next_call(),
        Call::Draw {
            page: 2,
            width: 160
        }
    );
}

#[test]
fn the_window_is_woken_once_after_each_reply() {
    let harness = Harness::new();

    harness.open();
    harness.draw(1);
    harness.draw(2);

    // `next_reply` waits for one wake-up and then finds one reply, three
    // times over: every reply was in the channel before its wake-up.
    for _ in 0..3 {
        harness.next_reply();
    }
    // Three replies were woken for, and there is no fourth wake-up. The
    // worker handles one request at a time, and the third reply is the
    // answer to the last request, so nothing can still be on its way.
    assert!(harness.woken.try_recv().is_err());
    assert!(matches!(
        harness.worker.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn replies_come_in_the_order_of_the_requests() {
    let harness = Harness::new();

    for page in [3, 1, 2] {
        harness.draw(page);
    }
    let pages: Vec<usize> = (0..3).map(|_| drawn(harness.next_reply()).1).collect();

    assert_eq!(pages, [3, 1, 2]);
}

// Stale requests

#[test]
fn requests_of_an_older_generation_are_skipped() {
    let harness = Harness::gated();

    // Three pages are asked for. The worker starts on the first one and
    // stops inside the engine, with the other two in the queue.
    for page in [1, 2, 3] {
        harness.draw(page);
    }
    assert_eq!(
        harness.next_call(),
        Call::Draw {
            page: 1,
            width: 160
        }
    );

    // The window moves on (another file, say) and asks for one page of
    // the new generation. Only now is page 1 allowed to finish.
    let newer = harness.generation.advance();
    harness.draw(7);
    harness.let_one_page_through();
    harness.let_one_page_through();

    // Page 1 was already being drawn, so its reply still comes; the
    // window is the one that ignores it (see `thumbnails.rs`).
    let (generation, page, _) = drawn(harness.next_reply());
    assert_eq!((generation, page), (0, 1));
    // Pages 2 and 3 never reached the engine: the next call is page 7.
    assert_eq!(
        harness.next_call(),
        Call::Draw {
            page: 7,
            width: 160
        }
    );
    let (generation, page, _) = drawn(harness.next_reply());
    assert_eq!((generation, page), (newer, 7));
    assert!(harness.is_quiet());
}

#[test]
fn a_skipped_request_gets_no_reply_and_no_wake_up() {
    let harness = Harness::new();

    // Asked for in generation 0, which is over before it is sent.
    let old = harness.generation.current();
    harness.generation.advance();
    harness
        .worker
        .send(Request::Open {
            generation: old,
            path: PathBuf::from("old.pdf"),
        })
        .unwrap();
    // One of the current generation after it, to have something to wait
    // for: when its reply is here, the stale one has been dealt with.
    harness.draw(1);

    let (_, page, _) = drawn(harness.next_reply());
    assert_eq!(page, 1);
    assert_eq!(
        harness.next_call(),
        Call::Draw {
            page: 1,
            width: 160
        }
    );
    assert!(harness.is_quiet());
}

// The engine

#[test]
fn the_engine_is_loaded_at_the_first_request_and_only_once() {
    let harness = Harness::new();

    // Not when the thread starts: nothing can load the engine before a
    // request arrives, so this cannot depend on which thread runs first.
    assert_eq!(harness.loads.load(Ordering::SeqCst), 0);

    harness.open();
    harness.next_reply();
    harness.draw(1);
    harness.next_reply();

    assert_eq!(harness.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_missing_library_is_the_reply_and_the_next_request_tries_again() {
    let (woken_sender, woken) = mpsc::channel();
    let loads = Arc::new(AtomicUsize::new(0));
    let loads_in_thread = Arc::clone(&loads);
    let generation = Generation::default();
    // `FakeEngine` only names the type of engine that this loader would
    // give if it ever worked.
    let worker = Worker::spawn(
        generation,
        move || -> Result<FakeEngine, RenderError> {
            loads_in_thread.fetch_add(1, Ordering::SeqCst);
            Err(RenderError::LibraryMissing {
                path: PathBuf::from("app/pdfium.dll"),
            })
        },
        Box::new(move || {
            let _ = woken_sender.send(());
        }),
    )
    .unwrap();
    let open = || Request::Open {
        generation: 0,
        path: PathBuf::from("a.pdf"),
    };

    for attempt in 1..=2 {
        worker.send(open()).unwrap();
        woken.recv_timeout(GUARD).unwrap();
        let reply = worker.try_recv().unwrap();

        match reply {
            Reply::Opened {
                result: Err(RenderError::LibraryMissing { path }),
                ..
            } => assert_eq!(path, Path::new("app/pdfium.dll")),
            other => panic!("expected a missing library, got {other:?}"),
        }
        // A failure is not remembered: each request asks again.
        assert_eq!(loads.load(Ordering::SeqCst), attempt);
    }
}

// The size of a thumbnail

#[test]
fn a_page_too_tall_for_the_box_is_drawn_again_narrower() {
    // Every page comes out ten times as high as it is wide.
    let harness = Harness::with(false, 10);

    harness.draw(1);
    let (_, _, image) = drawn(harness.next_reply());

    // At 160 wide it would be 1600 high, and the box is 226 high. Drawn
    // 22 wide it is 220 high: 160 * 226 / 1600 = 22.6, rounded down.
    assert_eq!(
        harness.next_call(),
        Call::Draw {
            page: 1,
            width: 160
        }
    );
    assert_eq!(harness.next_call(), Call::Draw { page: 1, width: 22 });
    assert_eq!((image.width, image.height), (22, 220));
    assert!(harness.is_quiet());
}

#[test]
fn only_an_image_taller_than_the_box_needs_a_narrower_width() {
    let image = |width, height| PageImage {
        width,
        height,
        rgba: Vec::new(),
    };

    assert_eq!(narrower_width(&image(160, 226), 226), None);
    assert_eq!(narrower_width(&image(160, 113), 226), None);
    assert_eq!(narrower_width(&image(160, 227), 226), Some(159));
    assert_eq!(narrower_width(&image(160, 452), 226), Some(80));
    // Never no pixels at all, however thin the strip.
    assert_eq!(narrower_width(&image(1, 8192), 226), Some(1));
    // The largest image of the core does not overflow the arithmetic.
    assert_eq!(
        narrower_width(&image(MAX_RENDER_WIDTH, MAX_RENDER_HEIGHT), 1),
        Some(1)
    );
}

#[test]
fn a_thumbnail_size_stays_inside_what_the_core_draws() {
    assert_eq!(ThumbnailSize::new(160, 226), SIZE);
    assert_eq!(
        ThumbnailSize::new(0, 0),
        ThumbnailSize {
            width: 1,
            max_height: 1
        }
    );
    assert_eq!(
        ThumbnailSize::new(u32::MAX, u32::MAX),
        ThumbnailSize {
            width: MAX_RENDER_WIDTH,
            max_height: MAX_RENDER_HEIGHT
        }
    );
}

// The generation

#[test]
fn every_owner_of_a_generation_sees_it_advance() {
    let window = Generation::default();
    let worker = window.clone();

    assert_eq!(worker.current(), 0);
    assert_eq!(window.advance(), 1);
    assert_eq!(window.advance(), 2);

    // A clone is one more owner of the same number, not a copy of it.
    assert_eq!(worker.current(), 2);
}

// The end of the thread

#[test]
fn dropping_the_worker_ends_its_thread() {
    let Harness {
        worker,
        generation,
        calls,
        ..
    } = Harness::new();
    worker
        .send(Request::Open {
            generation: generation.current(),
            path: PathBuf::from("a.pdf"),
        })
        .unwrap();

    // `drop` waits for the thread: if the thread did not end, the test
    // would hang here, and the test runner would say so.
    drop(worker);

    // Dropping moved the generation on, so whatever was still in the
    // queue was skipped and not drawn.
    assert_eq!(generation.current(), 1);
    // The thread owned the engine, and the engine a sender of this
    // channel. With the thread gone, every sender is gone, and that is
    // what `Disconnected` says: `Timeout` would mean one is still alive.
    let ended = loop {
        match calls.recv_timeout(GUARD) {
            Ok(_) => continue,
            Err(error) => break error,
        }
    };
    assert_eq!(ended, RecvTimeoutError::Disconnected);
}

#[test]
fn what_crosses_the_channels_is_send() {
    // Nothing runs here: the test is that this compiles. A value can only
    // be sent through a channel to another thread if its type is `Send`,
    // and a `Worker` is owned by the window, which eframe may move.
    fn crosses_threads<T: Send>() {}
    crosses_threads::<Request>();
    crosses_threads::<Reply>();
    crosses_threads::<Generation>();
    crosses_threads::<Wake>();
    crosses_threads::<Worker>();
}

// The real engine
//
// These two use PDFium itself: the build image unpacks it and points
// `DANONPDF_PDFIUM_DIR` at it. A process can load PDFium once, and every
// test of this crate shares one process, so no test here may load another
// library file: asking for a folder without one is refused before
// anything is loaded, whatever is loaded already.

/// A worker that loads its engine with `load_engine`, and the receiving
/// end of its wake-ups.
fn real_worker(
    load_engine: impl Fn() -> Result<Renderer, RenderError> + Send + 'static,
) -> (Worker, Receiver<()>) {
    let (woken_sender, woken) = mpsc::channel();
    let worker = Worker::spawn(
        Generation::default(),
        load_engine,
        Box::new(move || {
            let _ = woken_sender.send(());
        }),
    )
    .unwrap();
    (worker, woken)
}

#[test]
fn pdfium_counts_and_draws_through_the_worker() {
    let scratch = ScratchDir::new("worker-pdfium");
    // Page `n` of this file is `500 + n` points wide and 842 high.
    let path = scratch.pdf("report.pdf", 3);
    let (worker, woken) = real_worker(load_pdfium);

    worker
        .send(Request::Open {
            generation: 0,
            path: path.clone(),
        })
        .unwrap();
    woken.recv_timeout(GUARD).unwrap();
    let opened = worker.try_recv().unwrap();
    worker
        .send(Request::Page {
            generation: 0,
            path,
            page: NonZeroUsize::new(2).unwrap(),
            size: SIZE,
        })
        .unwrap();
    woken.recv_timeout(GUARD).unwrap();
    let (_, page, image) = drawn(worker.try_recv().unwrap());

    assert!(
        matches!(opened, Reply::Opened { result: Ok(3), .. }),
        "unexpected reply: {opened:?}"
    );
    assert_eq!(page, 2);
    // Page 2 is 502 by 842 points: 160 wide it would be 268 high, more
    // than the box allows, so it was drawn again 134 wide (160 * 226 /
    // 268, rounded down), which makes it 225 high.
    assert_eq!((image.width, image.height), (134, 225));
    assert_eq!(image.rgba.len(), 134 * 225 * 4);
    // An empty page is white paper, fully opaque.
    assert!(image.rgba.iter().all(|byte| *byte == 255));
}

#[test]
fn a_folder_without_pdfium_is_reported_with_the_file_that_was_looked_for() {
    let scratch = ScratchDir::new("worker-no-pdfium");
    let report = scratch.pdf("report.pdf", 1);
    let empty = scratch.file("no-library-here");
    std::fs::create_dir(&empty).unwrap();
    let folder = empty.clone();
    let (worker, woken) = real_worker(move || Renderer::open(&folder));

    worker
        .send(Request::Open {
            generation: 0,
            path: report,
        })
        .unwrap();
    woken.recv_timeout(GUARD).unwrap();
    let reply = worker.try_recv().unwrap();

    match reply {
        Reply::Opened {
            result: Err(RenderError::LibraryMissing { path }),
            ..
        } => {
            assert_eq!(path.parent(), Some(empty.as_path()));
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.contains("pdfium"), "unexpected library name {name}");
        }
        other => panic!("expected a missing library, got {other:?}"),
    }
}
