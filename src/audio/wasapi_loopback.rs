//! Windows backend: WASAPI loopback capture of an output device.
//!
//! Loopback records exactly what an output device is playing, the Windows
//! equivalent of a PulseAudio monitor source. We ask WASAPI to convert to
//! 48 kHz f32 stereo so the DSP code is the same on every platform.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta};

use super::{RATE, Shared, set_status};

const FRAME_BYTES: usize = 8;
/// WASAPI buffer length (100 ns units): 200 ms of headroom between polls.
const BUFFER_HNS: i64 = 2_000_000;
const POLL: Duration = Duration::from_millis(5);
/// Windows delivers no loopback packets while nothing is playing. After this
/// long without data we feed silence so the display decays instead of freezing.
const SILENCE_AFTER: Duration = Duration::from_millis(40);
const DEFAULT_CHECK: Duration = Duration::from_secs(2);

pub fn spawn(shared: Arc<Shared>, source: Option<String>) {
    thread::spawn(move || {
        // COM must be initialised on the thread that uses WASAPI.
        let _ = initialize_mta();
        loop {
            if let Err(e) = run_stream(&shared, source.as_deref()) {
                set_status(&shared, format!("{e:#}"));
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

pub fn list_sources() -> Result<Vec<String>> {
    let _ = initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    let mut names = Vec::new();
    for device in &enumerator.get_device_collection(&Direction::Render)? {
        names.push(device?.get_friendlyname()?);
    }
    Ok(names)
}

/// Captures until the default output changes (returns `Ok`) or the device
/// fails, e.g. because it was unplugged (returns `Err`).
fn run_stream(shared: &Shared, source: Option<&str>) -> Result<()> {
    let enumerator = DeviceEnumerator::new()?;
    let device = match source {
        Some(name) => enumerator
            .get_device_collection(&Direction::Render)?
            .get_device_with_name(name)
            .with_context(|| format!("no output device named {name:?} (see --list)"))?,
        None => enumerator
            .get_default_device(&Direction::Render)
            .context("no default output device")?,
    };
    let id = device.get_id()?;
    let name = device.get_friendlyname()?;

    // Opening a *render* device for *capture* is what makes this loopback.
    let mut client = device.get_iaudioclient()?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, 2, None);
    let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: BUFFER_HNS };
    client
        .initialize_client(&format, &Direction::Capture, &mode)
        .with_context(|| format!("cannot open {name}"))?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    set_status(shared, name);

    let mut buf = vec![0u8; 4096 * FRAME_BYTES];
    let mut last_data = Instant::now();
    let mut last_check = Instant::now();

    loop {
        thread::sleep(POLL);

        let mut got_data = false;
        while let Some(frames) = capture.get_next_packet_size()?
            && frames > 0
        {
            let needed = frames as usize * FRAME_BYTES;
            if buf.len() < needed {
                buf.resize(needed, 0);
            }
            let (read, info) = capture.read_from_device(&mut buf)?;
            let mut ring = shared.ring.lock().unwrap();
            if info.flags.silent {
                ring.push_silence(read as usize);
            } else {
                ring.push_f32le(&buf[..read as usize * FRAME_BYTES]);
            }
            got_data = true;
        }

        let now = Instant::now();
        if got_data {
            last_data = now;
        } else if now - last_data >= SILENCE_AFTER {
            let frames = ((now - last_data).as_secs_f64() * RATE as f64) as usize;
            shared.ring.lock().unwrap().push_silence(frames);
            last_data = now;
        }

        if source.is_none() && now - last_check >= DEFAULT_CHECK {
            last_check = now;
            let current = enumerator.get_default_device(&Direction::Render)?.get_id()?;
            if current != id {
                let _ = client.stop_stream();
                return Ok(());
            }
        }
    }
}
