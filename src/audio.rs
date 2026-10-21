//! Audio capture from PipeWire (via its PulseAudio layer).
//!
//! A background thread records the monitor of the default output (or a
//! user-chosen source) and writes stereo frames into a ring buffer that the
//! UI thread snapshots once per frame.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

pub const RATE: u32 = 48_000;
const RING_LEN: usize = 16_384;
/// Frames per read: ~10 ms keeps latency low without hammering the server.
const CHUNK_FRAMES: usize = 512;

pub struct Ring {
    l: Vec<f32>,
    r: Vec<f32>,
    pos: usize,
}

impl Ring {
    fn new() -> Self {
        Self { l: vec![0.0; RING_LEN], r: vec![0.0; RING_LEN], pos: 0 }
    }

    fn push(&mut self, l: f32, r: f32) {
        self.l[self.pos] = l;
        self.r[self.pos] = r;
        self.pos = (self.pos + 1) % RING_LEN;
    }

    /// Copies the most recent `n` frames, oldest first.
    pub fn latest(&self, n: usize, out_l: &mut Vec<f32>, out_r: &mut Vec<f32>) {
        let n = n.min(RING_LEN);
        out_l.clear();
        out_r.clear();
        let start = (self.pos + RING_LEN - n) % RING_LEN;
        for i in 0..n {
            let idx = (start + i) % RING_LEN;
            out_l.push(self.l[idx]);
            out_r.push(self.r[idx]);
        }
    }
}

pub struct Shared {
    pub ring: Mutex<Ring>,
    /// Human-readable description of what we are capturing (or the last error).
    pub status: Mutex<String>,
}

/// Starts capturing. With `source == None` the default output's monitor is
/// used, and capture reconnects whenever the default output changes.
pub fn start(source: Option<String>) -> Arc<Shared> {
    let shared = Arc::new(Shared {
        ring: Mutex::new(Ring::new()),
        status: Mutex::new("connecting…".into()),
    });
    let reconnect = Arc::new(AtomicBool::new(false));
    let sink_name = Arc::new(Mutex::new(default_sink().unwrap_or_default()));

    if source.is_none() {
        let reconnect = reconnect.clone();
        let sink_name = sink_name.clone();
        thread::spawn(move || watch_default_sink(reconnect, sink_name));
    }

    let s = shared.clone();
    thread::spawn(move || capture_loop(s, source, reconnect, sink_name));
    shared
}

fn capture_loop(
    shared: Arc<Shared>,
    source: Option<String>,
    reconnect: Arc<AtomicBool>,
    sink_name: Arc<Mutex<String>>,
) {
    let spec = Spec { format: Format::F32le, channels: 2, rate: RATE };
    let frame_bytes = 8;
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: (CHUNK_FRAMES * frame_bytes) as u32,
    };
    let device = source.clone().unwrap_or_else(|| "@DEFAULT_MONITOR@".into());
    let mut buf = vec![0u8; CHUNK_FRAMES * frame_bytes];

    loop {
        let stream = Simple::new(
            None,
            "visualizer-pw",
            Direction::Record,
            Some(&device),
            "visualizer",
            &spec,
            None,
            Some(&attr),
        );
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                set_status(&shared, format!("cannot open {device}: {e}"));
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let label = match &source {
            Some(s) => s.clone(),
            None => {
                let sink = sink_name.lock().unwrap().clone();
                if sink.is_empty() { "default output".into() } else { sink }
            }
        };
        set_status(&shared, label);
        reconnect.store(false, Ordering::Relaxed);

        loop {
            if reconnect.swap(false, Ordering::Relaxed) {
                break;
            }
            if let Err(e) = stream.read(&mut buf) {
                set_status(&shared, format!("read error: {e}"));
                thread::sleep(Duration::from_millis(500));
                break;
            }
            let mut ring = shared.ring.lock().unwrap();
            for frame in buf.chunks_exact(frame_bytes) {
                let l = f32::from_le_bytes(frame[0..4].try_into().unwrap());
                let r = f32::from_le_bytes(frame[4..8].try_into().unwrap());
                ring.push(l, r);
            }
        }
    }
}

fn set_status(shared: &Shared, msg: String) {
    *shared.status.lock().unwrap() = msg;
}

/// Polls the default sink so capture can follow it when the user switches
/// outputs (e.g. speakers → Bluetooth headphones).
fn watch_default_sink(reconnect: Arc<AtomicBool>, sink_name: Arc<Mutex<String>>) {
    loop {
        thread::sleep(Duration::from_secs(2));
        let Some(current) = default_sink() else { continue };
        let mut name = sink_name.lock().unwrap();
        if *name != current {
            *name = current;
            reconnect.store(true, Ordering::Relaxed);
        }
    }
}

fn default_sink() -> Option<String> {
    let out = Command::new("pactl").arg("get-default-sink").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}
