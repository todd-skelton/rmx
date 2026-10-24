//! Remix's status channel (`ReporterStatusChannel`): the progress lines of `new` and
//! `doctor`. On an ANSI terminal a running step redraws `• Label.`, `..` and `...` in
//! place and its result replaces it; elsewhere each line is written once.

use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rsmix_terminal::Styles;

/// Remix `clearCurrentLine`.
pub(crate) const CLEAR_LINE: &str = "\r\x1b[2K";

/// Remix's `DEFAULT_STATUS_FRAME_INTERVAL_MS`.
const FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// A shared output stream.
pub(crate) type Output = Arc<Mutex<dyn Write + Send>>;

pub(crate) struct Status {
    out: Output,
    styles: Styles,
    live: bool,
    interval: Duration,
    animation: Option<(mpsc::Sender<()>, JoinHandle<()>)>,
}

impl Status {
    /// Progress on `out`; `live` when it is a terminal that takes ANSI.
    pub(crate) fn new(out: Output, styles: Styles, live: bool) -> Status {
        Status { out, styles, live, interval: FRAME_INTERVAL, animation: None }
    }

    #[cfg(test)]
    pub(crate) fn with_interval(mut self, interval: Duration) -> Status {
        self.interval = interval;
        self
    }

    /// `text` as it is (details, summaries).
    pub(crate) fn text(&mut self, text: &str) {
        self.stop();
        write(&self.out, text);
    }

    /// A line of the progress protocol: `• Step...` starts a step, any other line ends it
    /// (`✓ Step`, `✗ Step`, `• step (skipped: reason)`).
    pub(crate) fn line(&mut self, line: &str) {
        self.stop();
        let running = line.strip_prefix("\u{2022} ").and_then(|l| l.strip_suffix("..."));
        match running {
            Some(label) if self.live => self.animate(label),
            _ => {
                let clear = if self.live { CLEAR_LINE } else { "" };
                write(&self.out, &format!("{clear}{}\n", super::style_progress(line, &self.styles)));
            }
        }
    }

    fn animate(&mut self, label: &str) {
        let (styles, label) = (self.styles, label.to_string());
        let frame = move |dots: &str| format!("{CLEAR_LINE}{}", styles.gray(&format!("\u{2022} {label}{dots}")));
        write(&self.out, &frame("."));
        let (stop, stopped) = mpsc::channel::<()>();
        let (out, interval) = (self.out.clone(), self.interval);
        let timer = std::thread::spawn(move || {
            let mut dots = 1;
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                dots = dots % 3 + 1;
                write(&out, &frame(&".".repeat(dots)));
            }
        });
        self.animation = Some((stop, timer));
    }

    fn stop(&mut self) {
        if let Some((stop, timer)) = self.animation.take() {
            drop(stop);
            let _ = timer.join();
        }
    }
}

impl Drop for Status {
    fn drop(&mut self) {
        self.stop();
    }
}

fn write(out: &Output, text: &str) {
    let mut out = out.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
