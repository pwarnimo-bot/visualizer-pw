mod audio;
mod dsp;
mod theme;
mod ui;

use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use dsp::{FFT_N, Meters, Spectrum};
use theme::{THEMES, Theme};

const FRAME: Duration = Duration::from_micros(16_667);
/// 0 means "fit to width".
const BAR_COUNTS: [usize; 7] = [0, 16, 32, 48, 64, 96, 128];

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Dashboard,
    Spectrum,
    Scope,
    Meters,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ScopeMode {
    Overlay,
    Split,
    Lissajous,
}

pub struct App {
    pub view: View,
    pub scope_mode: ScopeMode,
    pub theme_idx: usize,
    pub sens_db: f32,
    pub bars_idx: usize,
    pub frozen: bool,
    pub help: bool,
    pub quit: bool,
    pub source: String,
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub spectrum: Spectrum,
    pub meters: Meters,
    /// Slowly decaying peak used for the oscilloscope's auto-gain.
    pub scope_peak: f32,
}

impl App {
    fn new() -> Self {
        Self {
            view: View::Dashboard,
            scope_mode: ScopeMode::Overlay,
            theme_idx: 0,
            sens_db: 0.0,
            bars_idx: 0,
            frozen: false,
            help: false,
            quit: false,
            source: String::new(),
            l: Vec::with_capacity(FFT_N),
            r: Vec::with_capacity(FFT_N),
            spectrum: Spectrum::new(),
            meters: Meters::new(),
            scope_peak: 0.0,
        }
    }

    pub fn theme(&self) -> &'static Theme {
        &THEMES[self.theme_idx]
    }

    pub fn bar_count(&self, width: u16) -> usize {
        match BAR_COUNTS[self.bars_idx] {
            0 => (width as usize / 3).clamp(8, 160),
            n => n,
        }
    }

    pub fn bars_label(&self) -> String {
        match BAR_COUNTS[self.bars_idx] {
            0 => "auto".into(),
            n => n.to_string(),
        }
    }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('1') => self.view = View::Dashboard,
            KeyCode::Char('2') => self.view = View::Spectrum,
            KeyCode::Char('3') => self.view = View::Scope,
            KeyCode::Char('4') => self.view = View::Meters,
            KeyCode::Tab => {
                self.view = match self.view {
                    View::Dashboard => View::Spectrum,
                    View::Spectrum => View::Scope,
                    View::Scope => View::Meters,
                    View::Meters => View::Dashboard,
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Up => {
                self.sens_db = (self.sens_db + 3.0).min(30.0)
            }
            KeyCode::Char('-') | KeyCode::Char('_') | KeyCode::Down => {
                self.sens_db = (self.sens_db - 3.0).max(-30.0)
            }
            KeyCode::Char('0') => self.sens_db = 0.0,
            KeyCode::Char('c') => self.theme_idx = (self.theme_idx + 1) % THEMES.len(),
            KeyCode::Char('s') => self.bars_idx = (self.bars_idx + 1) % BAR_COUNTS.len(),
            KeyCode::Char('m') => {
                self.scope_mode = match self.scope_mode {
                    ScopeMode::Overlay => ScopeMode::Split,
                    ScopeMode::Split => ScopeMode::Lissajous,
                    ScopeMode::Lissajous => ScopeMode::Overlay,
                }
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => self.frozen = !self.frozen,
            KeyCode::Char('h') | KeyCode::Char('?') => self.help = !self.help,
            _ => {}
        }
    }
}

#[cfg(unix)]
const SOURCE_HELP: &str = "Capture from this source instead of the default
                          output's monitor (see --list)";
#[cfg(windows)]
const SOURCE_HELP: &str = "Capture this output device instead of the default
                          output (name as shown by --list)";

fn usage() -> String {
    format!(
        "\
visualizer-pw — terminal audio visualizer (PipeWire/PulseAudio, Windows WASAPI)

USAGE:
    visualizer-pw [--source <NAME>]

OPTIONS:
    -s, --source <NAME>   {SOURCE_HELP}
    -l, --list            List capturable sources and exit
    -h, --help            Show this help

KEYS (in the app): press h or ? for the full list."
    )
}

fn main() -> Result<()> {
    let mut source = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-s" | "--source" => {
                source = Some(args.next().ok_or_else(|| anyhow::anyhow!("--source needs a value"))?)
            }
            "-l" | "--list" => {
                for name in audio::list_sources()? {
                    println!("{name}");
                }
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{}", usage());
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}\n\n{}", usage()),
        }
    }

    let shared = audio::start(source);
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &shared);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, shared: &audio::Shared) -> Result<()> {
    let mut app = App::new();
    let mut last = Instant::now();

    while !app.quit {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;

        if !app.frozen {
            shared.ring.lock().unwrap().latest(FFT_N, &mut app.l, &mut app.r);
            app.meters.update(&app.l, &app.r, dt, app.sens_db);
        }
        app.source = shared.status.lock().unwrap().clone();

        terminal.draw(|f| ui::draw(f, &mut app, dt))?;

        let mut timeout = FRAME.saturating_sub(now.elapsed());
        while event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Release {
                    app.handle_key(key.code, key.modifiers);
                }
            }
            timeout = FRAME.saturating_sub(now.elapsed());
        }
    }
    Ok(())
}
