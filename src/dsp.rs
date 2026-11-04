//! Signal analysis: log-spaced FFT spectrum with falling peaks, and
//! level metering (sample peak + VU needle ballistics).

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::audio::RATE;

pub const FFT_N: usize = 4096;
pub const F_MIN: f32 = 30.0;
pub const F_MAX: f32 = 16_000.0;
/// dB range mapped onto bar height 0..1.
const FLOOR_DB: f32 = -70.0;
const CEIL_DB: f32 = -10.0;
/// Pink-noise compensation so treble isn't permanently buried under bass.
const TILT_DB_PER_OCT: f32 = 3.0;
/// Fraction of the distance to the target left after one 60 Hz frame.
const BAR_RISE_LEFT: f32 = 0.3;
const BAR_FALL_PER_S: f32 = 1.4;
const PEAK_HOLD_S: f32 = 0.5;
const PEAK_GRAVITY: f32 = 2.0;

pub fn to_db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    amp: Vec<f32>,
    pub bars: Vec<f32>,
    pub peaks: Vec<f32>,
    peak_hold: Vec<f32>,
    peak_vel: Vec<f32>,
}

impl Spectrum {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_N);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..FFT_N)
            .map(|i| {
                let t = i as f32 / (FFT_N - 1) as f32;
                0.5 - 0.5 * (std::f32::consts::TAU * t).cos()
            })
            .collect();
        Self {
            fft,
            window,
            buf: vec![Complex::default(); FFT_N],
            scratch,
            amp: vec![0.0; FFT_N / 2],
            bars: Vec::new(),
            peaks: Vec::new(),
            peak_hold: Vec::new(),
            peak_vel: Vec::new(),
        }
    }

    pub fn resize(&mut self, n: usize) {
        if self.bars.len() != n {
            self.bars = vec![0.0; n];
            self.peaks = vec![0.0; n];
            self.peak_hold = vec![0.0; n];
            self.peak_vel = vec![0.0; n];
        }
    }

    pub fn update(&mut self, l: &[f32], r: &[f32], n: usize, sens_db: f32, dt: f32) {
        self.resize(n);
        if l.len() < FFT_N || n == 0 {
            return;
        }
        let off = l.len() - FFT_N;
        for i in 0..FFT_N {
            let mono = (l[off + i] + r[off + i]) * 0.5;
            self.buf[i] = Complex::new(mono * self.window[i], 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);

        // Hann coherent gain is 0.5, so a full-scale sine reads 1.0 (0 dBFS).
        let norm = 4.0 / FFT_N as f32;
        for (a, c) in self.amp.iter_mut().zip(&self.buf) {
            *a = c.norm() * norm;
        }

        let bin_hz = RATE as f32 / FFT_N as f32;
        let ratio = F_MAX / F_MIN;
        for i in 0..n {
            let f_lo = F_MIN * ratio.powf(i as f32 / n as f32);
            let f_hi = F_MIN * ratio.powf((i + 1) as f32 / n as f32);
            let (lo, hi) = (f_lo / bin_hz, f_hi / bin_hz);
            let a = if hi - lo < 1.0 {
                // Narrower than a bin (bass end): interpolate at the centre.
                let c = (lo + hi) * 0.5;
                let k = c.floor() as usize;
                let frac = c - k as f32;
                self.amp[k] * (1.0 - frac) + self.amp[(k + 1).min(self.amp.len() - 1)] * frac
            } else {
                let k0 = lo.round() as usize;
                let k1 = (hi.round() as usize).clamp(k0, self.amp.len() - 1);
                self.amp[k0..=k1].iter().copied().fold(0.0, f32::max)
            };
            let fc = (f_lo * f_hi).sqrt();
            let db = to_db(a) + TILT_DB_PER_OCT * (fc / 1000.0).log2() + sens_db;
            let target = ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0);

            let bar = &mut self.bars[i];
            if target > *bar {
                *bar += (target - *bar) * (1.0 - BAR_RISE_LEFT.powf(dt * 60.0));
            } else {
                *bar = (*bar - BAR_FALL_PER_S * dt).max(target);
            }

            if *bar >= self.peaks[i] {
                self.peaks[i] = *bar;
                self.peak_hold[i] = PEAK_HOLD_S;
                self.peak_vel[i] = 0.0;
            } else if self.peak_hold[i] > 0.0 {
                self.peak_hold[i] -= dt;
            } else {
                self.peak_vel[i] += PEAK_GRAVITY * dt;
                self.peaks[i] = (self.peaks[i] - self.peak_vel[i] * dt).max(*bar);
            }
        }
    }
}

/// RMS level (dBFS) that reads 0 VU at sensitivity 0.
const VU_REF_DBFS: f32 = -16.0;
pub const VU_MIN_DB: f32 = -20.0;
pub const VU_MAX_DB: f32 = 3.0;
const PEAK_FALL_DB_PER_S: f32 = 24.0;
const HOLD_S: f32 = 1.5;
const HOLD_FALL_DB_PER_S: f32 = 12.0;

/// Needle position (0..1) for a VU reading. Real VU scales are linear in
/// voltage, which is why 0 VU sits around two thirds of the way across.
pub fn vu_pos(db: f32) -> f32 {
    let amp = |d: f32| 10f32.powf(d / 20.0);
    (amp(db) - amp(VU_MIN_DB)) / (amp(VU_MAX_DB) - amp(VU_MIN_DB))
}

#[derive(Default)]
pub struct Meters {
    pub peak_db: [f32; 2],
    pub hold_db: [f32; 2],
    hold_t: [f32; 2],
    pub clip_t: [f32; 2],
    pub vu_db: [f32; 2],
    pub needle: [f32; 2],
    needle_vel: [f32; 2],
    /// Phase correlation, -1 (out of phase) … 0 (wide/uncorrelated) … +1 (mono).
    pub corr: f32,
    /// False while the input is silent and `corr` is meaningless.
    pub corr_active: bool,
}

impl Meters {
    pub fn new() -> Self {
        Self { peak_db: [-120.0; 2], hold_db: [-120.0; 2], vu_db: [-60.0; 2], ..Default::default() }
    }

    /// `speed` scales the meters' motion. `dt` stays real time for the peak
    /// window, which must cover every new sample, and the clip indicator.
    pub fn update(&mut self, l: &[f32], r: &[f32], dt: f32, speed: f32, sens_db: f32) {
        let real_dt = dt;
        let dt = dt * speed;
        self.update_correlation(l, r, dt);
        for (c, s) in [l, r].into_iter().enumerate() {
            if s.is_empty() {
                continue;
            }
            // Sample peak over (roughly) what arrived since the last frame.
            let n_pk = ((real_dt * RATE as f32) as usize + 256).min(s.len());
            let pk = s[s.len() - n_pk..].iter().fold(0f32, |a, &x| a.max(x.abs()));
            let pk_db = to_db(pk);

            self.peak_db[c] = if pk_db >= self.peak_db[c] {
                pk_db
            } else {
                (self.peak_db[c] - PEAK_FALL_DB_PER_S * dt).max(pk_db)
            };
            if pk_db >= self.hold_db[c] {
                self.hold_db[c] = pk_db;
                self.hold_t[c] = HOLD_S;
            } else if self.hold_t[c] > 0.0 {
                self.hold_t[c] -= dt;
            } else {
                self.hold_db[c] -= HOLD_FALL_DB_PER_S * dt;
            }
            self.clip_t[c] = if pk >= 0.999 { 2.0 } else { (self.clip_t[c] - real_dt).max(0.0) };

            let n_rms = (RATE as usize / 20).min(s.len());
            let ms = s[s.len() - n_rms..].iter().map(|x| x * x).sum::<f32>() / n_rms as f32;
            let vu = 10.0 * ms.max(1e-12).log10() - VU_REF_DBFS + sens_db;
            self.vu_db[c] += (vu - self.vu_db[c]) * (1.0 - (-dt / 0.15).exp());

            // Under-damped spring so the needle swings (and slightly overshoots)
            // like a mechanical movement.
            let target = vu_pos(vu);
            let (omega, zeta) = (14.0f32, 0.75f32);
            let steps = (dt / 0.004).ceil().max(1.0) as usize;
            let h = dt / steps as f32;
            for _ in 0..steps {
                let acc = omega * omega * (target - self.needle[c]) - 2.0 * zeta * omega * self.needle_vel[c];
                self.needle_vel[c] += acc * h;
                self.needle[c] += self.needle_vel[c] * h;
                if self.needle[c] < -0.02 {
                    self.needle[c] = -0.02;
                    self.needle_vel[c] = 0.0;
                } else if self.needle[c] > 1.04 {
                    self.needle[c] = 1.04;
                    self.needle_vel[c] = 0.0;
                }
            }
        }
    }

    /// Normalised cross-correlation of L and R over the last 100 ms.
    fn update_correlation(&mut self, l: &[f32], r: &[f32], dt: f32) {
        let n = (RATE as usize / 10).min(l.len());
        if n == 0 {
            return;
        }
        let (mut lr, mut ll, mut rr) = (0f32, 0f32, 0f32);
        for (&a, &b) in l[l.len() - n..].iter().zip(&r[r.len() - n..]) {
            lr += a * b;
            ll += a * a;
            rr += b * b;
        }
        // ~-80 dBFS RMS across both channels counts as silence.
        self.corr_active = (ll + rr) / n as f32 > 1e-8;
        if !self.corr_active {
            return;
        }
        // One silent channel has no defined correlation; treat it as 0.
        let denom = (ll * rr).sqrt();
        let target = if denom > 1e-12 { (lr / denom).clamp(-1.0, 1.0) } else { 0.0 };
        self.corr += (target - self.corr) * (1.0 - (-dt / 0.25).exp());
    }
}
