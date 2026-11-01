//! The terminal progress indicator (decisions D27): a spinner and a short
//! message on stderr, shown only once a call takes longer than [`DELAY`], and
//! cleared before the results are printed. Results (stdout) never contain it.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use h5tags_core::session::Progress;

/// Calls faster than this never show the spinner, so it doesn't flicker.
pub const DELAY: Duration = Duration::from_millis(300);
const TICK: Duration = Duration::from_millis(80);
const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The spinner line: its message, and how many characters are on screen.
#[derive(Default)]
struct Line {
    message: String,
    drawn: usize,
}

impl Line {
    /// Removes the line from the screen, if it is drawn.
    fn clear(&mut self, out: &mut impl Write) {
        if self.drawn > 0 {
            let _ = write!(out, "\r{}\r", " ".repeat(self.drawn));
            let _ = out.flush();
            self.drawn = 0;
        }
    }
}

/// One frame, `\r⠹ message`, padded to cover a longer previous frame, and its width.
fn frame_text(frame: char, message: &str, previous_width: usize) -> (String, usize) {
    let text = format!("{frame} {message}");
    let width = text.chars().count();
    (format!("\r{text}{}", " ".repeat(previous_width.saturating_sub(width))), width)
}

/// The message for a progress report: `forge: reading modules 120/340`.
pub fn progress_message(build: &str, p: Progress) -> String {
    format!("{build}: {} {}/{}", p.stage, p.done, p.total)
}

struct Shared {
    line: Mutex<Line>,
    stop: AtomicBool,
}

/// Updates the message, or writes other stderr output without garbling the line.
#[derive(Clone)]
pub struct SpinnerHandle(Arc<Shared>);

impl SpinnerHandle {
    pub fn set_message(&self, message: String) {
        self.0.line.lock().unwrap().message = message;
    }

    /// Clears the spinner line, runs `write` with stderr, and leaves the next
    /// tick to redraw the spinner below.
    fn write_above(
        &self,
        write: impl FnOnce(&mut std::io::StderrLock<'_>) -> std::io::Result<usize>,
    ) -> std::io::Result<usize> {
        let mut line = self.0.line.lock().unwrap();
        let mut err = std::io::stderr().lock();
        line.clear(&mut err);
        write(&mut err)
    }
}

pub struct Spinner {
    handle: SpinnerHandle,
    thread: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Spinner {
        let shared = Arc::new(Shared {
            line: Mutex::new(Line { message: message.to_owned(), drawn: 0 }),
            stop: AtomicBool::new(false),
        });
        let s = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let started = Instant::now();
            let mut n = 0;
            while !s.stop.load(Ordering::Relaxed) {
                if started.elapsed() >= DELAY {
                    let mut line = s.line.lock().unwrap();
                    let (text, width) = frame_text(FRAMES[n % FRAMES.len()], &line.message, line.drawn);
                    let mut err = std::io::stderr().lock();
                    let _ = err.write_all(text.as_bytes());
                    let _ = err.flush();
                    line.drawn = width;
                    n += 1;
                }
                std::thread::park_timeout(TICK);
            }
        });
        Spinner { handle: SpinnerHandle(shared), thread: Some(thread) }
    }

    pub fn handle(&self) -> SpinnerHandle {
        self.handle.clone()
    }

    /// Stops the spinner and clears its line: nothing is left on screen.
    pub fn finish(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.handle.0.stop.store(true, Ordering::Relaxed);
            thread.thread().unpark();
            let _ = thread.join();
            self.handle.0.line.lock().unwrap().clear(&mut std::io::stderr().lock());
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Log output to stderr that clears the spinner line before each write.
pub struct LogWriter(pub Option<SpinnerHandle>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &self.0 {
            Some(handle) => handle.write_above(|err| err.write(buf)),
            None => std::io::stderr().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_cover_the_previous_one() {
        assert_eq!(frame_text('⠋', "loading", 0), ("\r⠋ loading".to_owned(), 9));
        // A shorter message after a longer one is padded, so no old characters remain.
        assert_eq!(frame_text('⠙', "ok", 9), ("\r⠙ ok     ".to_owned(), 4));
    }

    #[test]
    fn clearing_overwrites_the_drawn_width() {
        let mut out = Vec::new();
        let mut line = Line { message: "x".into(), drawn: 4 };
        line.clear(&mut out);
        assert_eq!(out, b"\r    \r");
        assert_eq!(line.drawn, 0);
        out.clear();
        line.clear(&mut out); // nothing drawn: nothing written
        assert!(out.is_empty());
    }

    #[test]
    fn progress_messages() {
        let p = Progress { stage: Progress::READING_MODULES, done: 120, total: 340 };
        assert_eq!(progress_message("forge", p), "forge: reading modules 120/340");
    }

    #[test]
    fn a_fast_call_leaves_nothing_on_screen() {
        // Finished before DELAY: the thread never draws, and finish() returns promptly.
        let started = Instant::now();
        let spinner = Spinner::start("loading");
        let handle = spinner.handle();
        spinner.finish();
        assert!(started.elapsed() < DELAY);
        assert_eq!(handle.0.line.lock().unwrap().drawn, 0);
    }
}
