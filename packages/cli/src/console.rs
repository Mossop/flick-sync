use std::{
    io,
    ops::{Deref, DerefMut},
    sync::{Arc, RwLock},
};

use console::{pad_str, Alignment, Style, Term};
use dialoguer::{Input, Password, Select};
use flexi_logger::{writers::LogWriter, DeferredNow, Level, Record};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

#[derive(Default)]
struct ProgressBars {
    bars: MultiProgress,
    count: usize,
}

#[derive(Clone)]
pub struct Bar {
    bar: ProgressBar,
    progress_bars: Arc<RwLock<Option<ProgressBars>>>,
}

impl Bar {
    pub fn set_position(&self, position: u64) {
        self.bar.set_position(position);
    }

    pub fn set_length(&self, length: u64) {
        self.bar.set_length(length);
    }

    pub fn finish(self) {
        self.bar.finish_and_clear();

        let mut progress = self.progress_bars.write().unwrap();
        let is_empty = if let Some(ref mut progress) = progress.deref_mut() {
            progress.bars.remove(&self.bar);
            progress.count -= 1;
            progress.count == 0
        } else {
            false
        };

        if is_empty {
            *progress = None;
        }
    }
}

#[derive(Clone)]
pub struct Console {
    term: Term,
    progress_bars: Arc<RwLock<Option<ProgressBars>>>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            term: Term::stdout(),
            progress_bars: Arc::new(RwLock::new(None)),
        }
    }
}

impl Console {
    pub fn add_progress_bar(&self, msg: &str) -> Bar {
        let inner_bar = ProgressBar::new(100)
            .with_message(msg.to_owned())
            .with_style(
                ProgressStyle::with_template(
                    "{msg:30!} {wide_bar}    {bytes:>10}/{total_bytes:10}",
                )
                .unwrap(),
            );

        let mut p_state = self.progress_bars.write().unwrap();
        let progress = p_state.get_or_insert(Default::default());
        progress.count += 1;

        Bar {
            bar: progress.bars.add(inner_bar),
            progress_bars: self.progress_bars.clone(),
        }
    }

    fn with_term<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Term) -> R,
    {
        let progress = self.progress_bars.read().unwrap();

        if let Some(bars) = progress.deref() {
            bars.bars.suspend(|| f(&self.term))
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
