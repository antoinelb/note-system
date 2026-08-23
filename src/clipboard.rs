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

struct Request {
    reply: oneshot::Sender<Answer>,
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
        let reply = self.request()?;
        reply.await.map_err(|_| {
            "clipboard reader stopped before answering".to_string()
        })?
    }

    fn request(&self) -> Result<oneshot::Receiver<Answer>, String> {
        let requests = match &self.state {
            State::Running(requests) => requests,
            State::Failed(reason) => return Err(reason.to_string()),
        };
        let (reply, answer) = oneshot::channel();
        match requests.try_send(Request { reply }) {
            Ok(()) => Ok(answer),
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
    for Request { reply } in requests {
        let answer = read(&mut clipboard, &mut open);
        let _ = reply.send(answer);
    }
}

fn read(
    clipboard: &mut Option<Box<dyn TextClipboard>>,
    open: &mut Open,
) -> Answer {
    let active = match clipboard {
        Some(active) => active,
        None => clipboard.insert(open()?),
    };
    let answer = active.get_text();
    if answer.is_err() {
        *clipboard = None;
    }
    answer
}

trait TextClipboard {
    fn get_text(&mut self) -> Answer;
}

impl TextClipboard for arboard::Clipboard {
    // The platform call itself belongs to arboard; the worker contract is
    // covered through `TextClipboard` fakes without requiring an X/Wayland
    // server in the test process.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn get_text(&mut self) -> Answer {
        arboard::Clipboard::get_text(self).map_err(|error| error.to_string())
    }
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
    }

    #[test]
    fn the_request_queue_is_bounded() {
        let (requests, _pending) = sync_channel(1);
        let reader = Reader {
            state: State::Running(requests),
        };
        let _held = reader.request().expect("the first slot");
        assert_eq!(
            reader.request().expect_err("the queue is full"),
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
