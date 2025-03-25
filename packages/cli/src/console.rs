use std::{
    io,
    sync::{Arc, Mutex},
};

use console::Term;
use dialoguer::{Input, Password, Select};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use tracing_subscriber::fmt::MakeWriter;

pub struct Bar {
    bar: ProgressBar,
    console: Console,
}

impl Bar {
    pub fn set_position(&self, position: u64) {
        self.bar.set_position(position);
    }

    pub fn set_length(&self, length: u64) {
        self.bar.set_length(length);
    }

    pub fn length(&self) -> Option<u64> {
        self.bar.length()
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        self.bar.finish_and_clear();

        self.console.progress_bars.remove(&self.bar);
        let mut state = self.console.state.lock().unwrap();
        state.progress_bar_count -= 1;

        self.console.update_draw_target(&state);
    }
}

pub struct ConsoleWriter {
    console: Console,
    _guard: BarHideGuard,
}

impl ConsoleWriter {
    fn new(console: &Console) -> Self {
        Self {
            console: console.clone(),
            _guard: BarHideGuard::new(console),
        }
    }
}

impl io::Write for ConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.console.term.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.console.term.flush()
    }
}

#[derive(Default)]
struct ConsoleState {
    progress_bar_count: usize,
    hide_count: usize,
}

struct BarHideGuard {
    console: Console,
}

impl BarHideGuard {
    fn new(console: &Console) -> Self {
        let mut state = console.state.lock().unwrap();
        state.hide_count += 1;

        console.update_draw_target(&state);

        Self {
            console: console.clone(),
        }
    }
}

impl Drop for BarHideGuard {
    fn drop(&mut self) {
        let mut state = self.console.state.lock().unwrap();
        state.hide_count -= 1;

        self.console.update_draw_target(&state);
    }
}

pub enum ProgressType {
    Bytes,
    Percent,
}

#[derive(Clone)]
pub struct Console {
    term: Term,
    progress_bars: MultiProgress,
    state: Arc<Mutex<ConsoleState>>,
}

impl Default for Console {
    fn default() -> Self {
        let term = Term::stdout();
        Self {
            progress_bars: MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
            term,
            state: Default::default(),
        }
    }
}

impl Console {
    fn update_draw_target(&self, state: &ConsoleState) {
        let should_be_hidden = state.progress_bar_count == 0 || state.hide_count > 0;

        if should_be_hidden != self.progress_bars.is_hidden() {
            if should_be_hidden {
                #[allow(unused_must_use)]
                {
                    self.progress_bars.clear();
                }
                self.progress_bars
                    .set_draw_target(ProgressDrawTarget::hidden());
            } else {
                self.progress_bars
                    .set_draw_target(ProgressDrawTarget::term(self.term.clone(), 10))
            }
        }
    }

    pub fn add_progress_bar(&self, msg: &str, progress_type: ProgressType) -> Bar {
        let style = match progress_type {
            ProgressType::Bytes => ProgressStyle::with_template(
                "{msg:35!} {wide_bar}  {decimal_bytes:>9}/{decimal_total_bytes:9}",
            )
            .unwrap(),
            ProgressType::Percent => {
                ProgressStyle::with_template("{msg:35!} {wide_bar}  {percent:>9}%         ")
                    .unwrap()
            }
        };

        let inner_bar = ProgressBar::new(100)
            .with_message(msg.to_owned())
            .with_style(style);

        let mut state = self.state.lock().unwrap();
        state.progress_bar_count += 1;
        self.update_draw_target(&state);

        Bar {
            bar: self.progress_bars.add(inner_bar),
            console: self.clone(),
        }
    }

    fn with_term<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Term) -> R,
    {
        let _guard = BarHideGuard::new(self);

        f(&self.term)
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

impl<'a> MakeWriter<'a> for Console {
    type Writer = ConsoleWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ConsoleWriter::new(self)
    }
}
