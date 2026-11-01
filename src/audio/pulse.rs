//! Linux backend: PipeWire via its PulseAudio layer.
//!
//! Records the monitor of the default output (or a user-chosen source).

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use super::{RATE, Shared, set_status};

/// Frames per read: ~10 ms keeps latency low without hammering the server.
const CHUNK_FRAMES: usize = 512;

pub fn spawn(shared: Arc<Shared>, source: Option<String>) {
    let reconnect = Arc::new(AtomicBool::new(false));
    let sink_name = Arc::new(Mutex::new(default_sink().unwrap_or_default()));

    if source.is_none() {
        let reconnect = reconnect.clone();
        let sink_name = sink_name.clone();
        thread::spawn(move || watch_default_sink(reconnect, sink_name));
    }

    thread::spawn(move || capture_loop(shared, source, reconnect, sink_name));
}

pub fn list_sources() -> anyhow::Result<Vec<String>> {
    let out = Command::new("pactl").args(["list", "short", "sources"]).output()?;
    anyhow::ensure!(out.status.success(), "pactl failed");
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
        .collect())
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
            shared.ring.lock().unwrap().push_f32le(&buf);
        }
    }
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
