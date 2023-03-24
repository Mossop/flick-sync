use std::{
    io,
    ops::Deref,
    sync::{Arc, RwLock},
};

use console::{pad_str, Alignment, Style, Term};
use flexi_logger::{writers::LogWriter, DeferredNow, Level, Record};
use indicatif::MultiProgress;

struct Progress {
    progress: MultiProgress,
}

#[derive(Clone)]
pub struct Console {
    term: Term,
    progress: Arc<RwLock<Option<Progress>>>,
}

impl Console {
    pub fn new() -> Self {
        Self {
            term: Term::stdout(),
            progress: Arc::new(RwLock::new(None)),
        }
    }

    fn with_term<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Term) -> R,
    {
        let progress = self.progress.read().unwrap();

        if let Some(bars) = progress.deref() {
            bars.progress.suspend(|| f(&self.term))
        } else {
            f(&self.term)
        }
    }

    fn inner_println<S: AsRef<str>>(&self, msg: S) -> io::Result<()> {
        self.with_term(|term| term.write_line(msg.as_ref()))
    }

    pub fn println<S: AsRef<str>>(&self, msg: S) {
        self.inner_println(msg).unwrap();
    }
}

impl LogWriter for Console {
    fn write(&self, _now: &mut DeferredNow, record: &Record) -> std::io::Result<()> {
        let style = match record.level() {
            Level::Error => Style::new().red(),
            Level::Warn => Style::new().yellow(),
            Level::Info => Style::new(),
            Level::Debug => Style::new().blue().bright(),
            Level::Trace => Style::new().black().bright(),
        };

        self.inner_println(format!(
            "{} {} {}",
            style.apply_to(pad_str(record.level().as_str(), 5, Alignment::Right, None)),
            pad_str(&format!("[{}]", record.target()), 20, Alignment::Left, None),
            style.apply_to(format!("{}", record.args())),
        ))
    }

    fn flush(&self) -> std::io::Result<()> {
        self.term.flush()
    }
}
