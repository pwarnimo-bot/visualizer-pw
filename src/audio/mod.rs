//! Audio capture of whatever the system is playing.
//!
//! A platform backend runs on a background thread and writes stereo frames
//! into a ring buffer that the UI thread snapshots once per frame:
//! - Linux: PipeWire via its PulseAudio layer (monitor of the default output)
//! - Windows: WASAPI loopback of the default output device

use std::sync::{Arc, Mutex};

#[cfg(unix)]
mod pulse;
#[cfg(unix)]
use pulse as backend;

#[cfg(windows)]
mod wasapi_loopback;
#[cfg(windows)]
use wasapi_loopback as backend;

/// Every backend delivers (or is resampled to) this rate.
pub const RATE: u32 = 48_000;
const RING_LEN: usize = 16_384;

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

    /// Pushes interleaved little-endian f32 stereo frames.
    fn push_f32le(&mut self, bytes: &[u8]) {
        for frame in bytes.chunks_exact(8) {
            let l = f32::from_le_bytes(frame[0..4].try_into().unwrap());
            let r = f32::from_le_bytes(frame[4..8].try_into().unwrap());
            self.push(l, r);
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn push_silence(&mut self, frames: usize) {
        for _ in 0..frames.min(RING_LEN) {
            self.push(0.0, 0.0);
        }
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

fn set_status(shared: &Shared, msg: String) {
    *shared.status.lock().unwrap() = msg;
}

/// Starts capturing. With `source == None` the default output is used, and
/// capture reconnects whenever the default output changes.
pub fn start(source: Option<String>) -> Arc<Shared> {
    let shared = Arc::new(Shared {
        ring: Mutex::new(Ring::new()),
        status: Mutex::new("connecting…".into()),
    });
    backend::spawn(shared.clone(), source);
    shared
}

/// Names accepted by `--source` on this platform.
pub fn list_sources() -> anyhow::Result<Vec<String>> {
    backend::list_sources()
}
