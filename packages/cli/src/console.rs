use std::{
    io,
    ops::Deref,
    sync::{Arc, RwLock},
};

use console::{pad_str, Alignment, Style, Term};
use dialoguer::{Input, Password, Select};
use flexi_logger::{writers::LogWriter, DeferredNow, Level, Record};
use indicatif::{MultiProgress, ProgressBar};

#[derive(Default)]
struct Progress {
    progress: MultiProgress,
    bars: usize,
}

#[derive(Clone)]
pub struct Bar {
    bar: ProgressBar,
    progress: Arc<RwLock<Option<Progress>>>,
}

impl Bar {
    pub fn set_position(&self, position: u64) {
        self.bar.set_position(position);
    }

    pub fn set_length(&self, length: u64) {
        self.bar.set_length(length);
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

#[derive(Clone)]
pub struct Console {
    term: Term,
    progress: Arc<RwLock<Option<Progress>>>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            term: Term::stdout(),
            progress: Arc::new(RwLock::new(None)),
        }
    }
}

impl Console {
    pub fn add_progress(&self) -> Bar {
        let inner_bar = ProgressBar::new(100);

        let mut p_state = self.progress.write().unwrap();
        let progress = p_state.get_or_insert(Default::default());
        progress.bars += 1;

        Bar {
            bar: progress.progress.add(inner_bar),
            progress: self.progress.clone(),
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

    pub fn input<P: Into<String>>(&self, prompt: P) -> String {
        self.with_term(|term| {
            Input::new()
                .with_prompt(prompt)
                .interact_text_on(term)
                .unwrap()
        })
    }

    pub fn password<P: Into<String>>(&self, prompt: P) -> String {
        self.with_term(|term| {
            Password::new()
                .with_prompt(prompt)
                .interact_on(term)
                .unwrap()
        })
    }

    pub fn select<P: Into<String>, S: ToString>(&self, prompt: P, items: &[S]) -> usize {
        self.with_term(|term| {
            Select::new()
                .with_prompt(prompt)
                .items(items)
                .default(0)
                .interact_on(term)
                .unwrap()
        })
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
