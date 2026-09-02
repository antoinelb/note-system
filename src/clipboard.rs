//! The desktop-native clipboard read boundary.
//!
//! WebKitGTK disables JavaScript clipboard reads unless its embedding
//! webview opts in, and Dioxus 0.7 does not expose that Wry builder switch.
//! One bounded worker therefore owns the native clipboard and serializes
//! every read away from the UI thread.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use tokio::sync::oneshot;

const REQUEST_CAPACITY: usize = 8;

type Answer = Result<String, String>;
/// PNG bytes when the clipboard holds an image, `None` when it holds
/// none (adr/2026-09-an-image-pastes-into-assets.md).
type ImageAnswer = Result<Option<Vec<u8>>, String>;
type Open = Box<
    dyn FnMut() -> Result<Box<dyn TextClipboard>, String> + Send + 'static,
>;
type Task = Box<dyn FnOnce() + Send + 'static>;
type Spawner = fn(Task) -> std::io::Result<()>;

/// A cloneable handle to the one clipboard worker.
#[derive(Clone)]
pub struct Reader {
    state: State,
}

#[derive(Clone)]
enum State {
    Running(SyncSender<Request>),
    Failed(Arc<str>),
}

enum Request {
    Text(oneshot::Sender<Answer>),
    Image(oneshot::Sender<ImageAnswer>),
}

/// Starts the production clipboard reader.
///
/// A thread that cannot start becomes a failed reader rather than a panic;
/// the first paste then reaches the ordinary status surface with the cause.
pub fn native() -> Reader {
    start(spawn_thread, Box::new(open_native))
}

impl Reader {
    /// Reads UTF-8 text without blocking the UI thread.
    pub async fn read_text(&self) -> Answer {
        let (reply, answer) = oneshot::channel();
        self.request(Request::Text(reply))?;
        answer.await.map_err(|_| {
            "clipboard reader stopped before answering".to_string()
        })?
    }

    /// Reads the clipboard's image as PNG bytes, encoded on the worker so
    /// the UI thread never sees the pixels; `Ok(None)` when it holds none.
    pub async fn read_image(&self) -> ImageAnswer {
        let (reply, answer) = oneshot::channel();
        self.request(Request::Image(reply))?;
        answer.await.map_err(|_| {
            "clipboard reader stopped before answering".to_string()
        })?
    }

    fn request(&self, request: Request) -> Result<(), String> {
        let requests = match &self.state {
            State::Running(requests) => requests,
            State::Failed(reason) => return Err(reason.to_string()),
        };
        match requests.try_send(request) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err("clipboard reader is busy".to_string())
            }
            Err(TrySendError::Disconnected(_)) => {
                Err("clipboard reader stopped".to_string())
            }
        }
    }
}

fn start(spawn: Spawner, open: Open) -> Reader {
    let (requests, pending) = sync_channel(REQUEST_CAPACITY);
    let task = Box::new(move || serve(pending, open));
    match spawn(task) {
        Ok(()) => Reader {
            state: State::Running(requests),
        },
        Err(error) => Reader {
            state: State::Failed(
                format!("starting clipboard reader: {error}").into(),
            ),
        },
    }
}

fn spawn_thread(task: Task) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("clipboard-reader".to_string())
        .spawn(task)
        .map(|_| ())
}

fn serve(requests: Receiver<Request>, mut open: Open) {
    let mut clipboard = None;
    for request in requests {
        match request {
            Request::Text(reply) => {
                let answer = read(&mut clipboard, &mut open, |active| {
                    active.get_text()
                });
                let _ = reply.send(answer);
            }
            Request::Image(reply) => {
                let answer = read(&mut clipboard, &mut open, |active| {
                    active.get_image()
                });
                let _ = reply.send(answer);
            }
        }
    }
}

/// One read of either kind: the clipboard is opened on first use and
/// dropped after a refusal, so the next request opens it afresh.
fn read<T>(
    clipboard: &mut Option<Box<dyn TextClipboard>>,
    open: &mut Open,
    get: impl FnOnce(&mut dyn TextClipboard) -> Result<T, String>,
) -> Result<T, String> {
    let active = match clipboard {
        Some(active) => active,
        None => clipboard.insert(open()?),
    };
    let answer = get(active.as_mut());
    if answer.is_err() {
        *clipboard = None;
    }
    answer
}

trait TextClipboard {
    fn get_text(&mut self) -> Answer;
    fn get_image(&mut self) -> ImageAnswer;
}

impl TextClipboard for arboard::Clipboard {
    // The platform call itself belongs to arboard; the worker contract is
    // covered through `TextClipboard` fakes without requiring an X/Wayland
    // server in the test process.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn get_text(&mut self) -> Answer {
        arboard::Clipboard::get_text(self).map_err(|error| error.to_string())
    }

    // A clipboard with no image is not a failure — arboard says
    // `ContentNotAvailable` — and the RGBA it hands over is encoded here,
    // on the worker, never on the UI thread.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn get_image(&mut self) -> ImageAnswer {
        match arboard::Clipboard::get_image(self) {
            Ok(image) => {
                encode_png(image.width, image.height, &image.bytes).map(Some)
            }
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// RGBA pixels as a PNG, the one image format every Typst `#image` reads
/// and every browser shows. A width or height past `u32`, or a byte count
/// that is not width × height × 4, is refused rather than guessed at.
fn encode_png(
    width: usize,
    height: usize,
    rgba: &[u8],
) -> Result<Vec<u8>, String> {
    let (w, h) = (
        u32::try_from(width).map_err(|_| "image too wide".to_string())?,
        u32::try_from(height).map_err(|_| "image too tall".to_string())?,
    );
    if rgba.len() != width.saturating_mul(height).saturating_mul(4) {
        return Err(format!(
            "image: {} bytes for {width}×{height} rgba",
            rgba.len()
        ));
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(rgba))
        .map_err(|error| format!("image: {error}"))?;
    Ok(out)
}

// Native construction is the same foreign boundary: the worker tests inject
// success and failure without requiring a display server.
#[cfg_attr(coverage_nightly, coverage(off))]
fn open_native() -> Result<Box<dyn TextClipboard>, String> {
    arboard::Clipboard::new()
        .map(|clipboard| Box::new(clipboard) as Box<dyn TextClipboard>)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    struct FakeClipboard {
        answers: VecDeque<Answer>,
    }

    impl TextClipboard for FakeClipboard {
        fn get_text(&mut self) -> Answer {
            self.answers
                .pop_front()
                .unwrap_or_else(|| Err("no scripted answer".to_string()))
        }

        /// The fake's image is its next text answer's bytes, or none when
        /// the answer is empty — enough to drive both arms of the worker.
        fn get_image(&mut self) -> ImageAnswer {
            self.get_text()
                .map(|text| (!text.is_empty()).then(|| text.into_bytes()))
        }
    }

    #[test]
    fn the_worker_answers_images_through_the_same_clipboard() {
        let reader = start(
            spawn_thread,
            Box::new(move || {
                Ok(Box::new(FakeClipboard {
                    answers: VecDeque::from([
                        Ok("png".to_string()),
                        Ok(String::new()),
                        Err("gone".to_string()),
                    ]),
                }))
            }),
        );
        assert_eq!(block_on(reader.read_image()), Ok(Some(b"png".to_vec())));
        assert_eq!(block_on(reader.read_image()), Ok(None));
        assert_eq!(block_on(reader.read_image()), Err("gone".to_string()));
    }

    #[test]
    fn rgba_encodes_as_png_and_a_wrong_byte_count_is_refused() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255])
            .expect("two pixels encode");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(encode_png(2, 1, &[0; 5]).is_err());
        // the encoder itself refuses an empty image
        assert!(encode_png(0, 0, &[]).is_err());
        assert!(encode_png(usize::MAX, 1, &[]).is_err());
        assert!(encode_png(1, usize::MAX, &[]).is_err());
    }

    #[test]
    fn the_worker_reuses_a_healthy_clipboard() {
        let opens = Arc::new(std::sync::Mutex::new(0usize));
        let counted = opens.clone();
        let reader = start(
            spawn_thread,
            Box::new(move || {
                *counted.lock().expect("the counter") += 1;
                Ok(Box::new(FakeClipboard {
                    answers: VecDeque::from([
                        Ok("first".to_string()),
                        Ok("second".to_string()),
                    ]),
                }))
            }),
        );

        assert_eq!(block_on(reader.read_text()), Ok("first".to_string()));
        assert_eq!(block_on(reader.read_text()), Ok("second".to_string()));
        assert_eq!(*opens.lock().expect("the counter"), 1);
    }

    #[test]
    fn a_failed_read_reopens_the_clipboard_on_the_next_request() {
        let opens = Arc::new(std::sync::Mutex::new(0usize));
        let counted = opens.clone();
        let reader = start(
            spawn_thread,
            Box::new(move || {
                let mut opens = counted.lock().expect("the counter");
                *opens += 1;
                let answer = if *opens == 1 {
                    Err("occupied".to_string())
                } else {
                    Ok("recovered".to_string())
                };
                Ok(Box::new(FakeClipboard {
                    answers: VecDeque::from([answer]),
                }))
            }),
        );

        assert_eq!(block_on(reader.read_text()), Err("occupied".to_string()));
        assert_eq!(block_on(reader.read_text()), Ok("recovered".to_string()));
        assert_eq!(*opens.lock().expect("the counter"), 2);
    }

    #[test]
    fn failures_to_start_open_or_answer_are_explicit() {
        fn refuse(_: Task) -> std::io::Result<()> {
            Err(std::io::Error::other("no thread"))
        }

        let failed = start(refuse, Box::new(open_native));
        assert_eq!(
            block_on(failed.read_text()),
            Err("starting clipboard reader: no thread".to_string())
        );

        let unopened =
            start(spawn_thread, Box::new(|| Err("no display".to_string())));
        assert_eq!(
            block_on(unopened.read_text()),
            Err("no display".to_string())
        );

        let (requests, pending) = sync_channel(1);
        let stopped = Reader {
            state: State::Running(requests),
        };
        drop(pending);
        assert_eq!(
            block_on(stopped.read_text()),
            Err("clipboard reader stopped".to_string())
        );
        assert_eq!(
            block_on(stopped.read_image()),
            Err("clipboard reader stopped".to_string())
        );

        let (requests, pending) = sync_channel(1);
        let unanswered = Reader {
            state: State::Running(requests),
        };
        let handle = std::thread::spawn(move || {
            if let Ok(request) = pending.recv() {
                drop(request);
            }
        });
        assert_eq!(
            block_on(unanswered.read_text()),
            Err("clipboard reader stopped before answering".to_string())
        );
        handle.join().expect("the test worker stops");

        // the image read stops the same way
        let (requests, pending) = sync_channel(1);
        let unanswered = Reader {
            state: State::Running(requests),
        };
        let handle = std::thread::spawn(move || {
            if let Ok(request) = pending.recv() {
                drop(request);
            }
        });
        assert_eq!(
            block_on(unanswered.read_image()),
            Err("clipboard reader stopped before answering".to_string())
        );
        handle.join().expect("the test worker stops");
    }

    #[test]
    fn the_request_queue_is_bounded() {
        let (requests, _pending) = sync_channel(1);
        let reader = Reader {
            state: State::Running(requests),
        };
        let (reply, _held) = oneshot::channel();
        reader
            .request(Request::Text(reply))
            .expect("the first slot");
        let (reply, _second) = oneshot::channel();
        assert_eq!(
            reader
                .request(Request::Image(reply))
                .expect_err("the queue is full"),
            "clipboard reader is busy"
        );
    }

    #[test]
    fn the_native_boundary_is_constructible() {
        let reader = native();
        drop(reader);
        let _ = open_native();
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let mut future = Box::pin(future);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        for _ in 0..10_000 {
            if let Poll::Ready(output) =
                Pin::new(&mut future).poll(&mut context)
            {
                return output;
            }
            std::thread::yield_now();
        }
        panic!("the clipboard future did not settle");
    }
}
