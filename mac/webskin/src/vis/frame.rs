//! The shared audio analysis every visualizer draws from, shaped like WMP's TimedLevel.

use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const BINS: usize = 1024;
const FFT_SIZE: usize = 2048;
const RING_SIZE: usize = 8192;

/// One analysis frame: 2 channels x 1024 of spectrum and waveform, plus float versions and band
/// energies for the built-in visualizations.
pub struct VisFrame {
    /// [channel * 1024 + bin], 0..255, linear frequency 0..Nyquist.
    pub frequency: [u8; 2 * BINS],
    /// [channel * 1024 + sample], 0..255 with 128 = silence.
    pub waveform: [u8; 2 * BINS],
    /// Mono spectrum 0..1 (log-magnitude scaled).
    pub spectrum: [f32; BINS],
    pub wave_l: [f32; BINS],
    pub wave_r: [f32; BINS],
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub level: f32,
    /// True on the frame a bass transient is detected.
    pub beat: bool,
    pub playing: bool,
    /// Seconds since the analyzer started.
    pub time: f64,
    pub sample_rate: u32,
}

impl Default for VisFrame {
    fn default() -> Self {
        VisFrame {
            frequency: [0; 2 * BINS],
            waveform: [128; 2 * BINS],
            spectrum: [0.0; BINS],
            wave_l: [0.0; BINS],
            wave_r: [0.0; BINS],
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            level: 0.0,
            beat: false,
            playing: false,
            time: 0.0,
            sample_rate: 48000,
        }
    }
}

/// The last few thousand samples of what the Mac is playing, written by the audio thread.
pub struct AudioRing {
    left: Vec<f32>,
    right: Vec<f32>,
    written: u64,
    last_data: Option<Instant>,
    pub sample_rate: u32,
}

impl AudioRing {
    pub fn shared() -> Arc<Mutex<AudioRing>> {
        Arc::new(Mutex::new(AudioRing { left: vec![0.0; RING_SIZE], right: vec![0.0; RING_SIZE], written: 0, last_data: None, sample_rate: 48000 }))
    }

    /// Appends interleaved samples with `channels` per frame (mono is copied to both sides).
    pub fn push_interleaved(&mut self, samples: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in samples.chunks_exact(channels) {
            let i = (self.written % RING_SIZE as u64) as usize;
            self.left[i] = frame[0];
            self.right[i] = if channels > 1 { frame[1] } else { frame[0] };
            self.written += 1;
        }
        self.last_data = Some(Instant::now());
    }
}

/// Turns the ring into frames once per display frame.
pub struct Analyzer {
    ring: Arc<Mutex<AudioRing>>,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    smooth: Vec<f32>,
    bass_avg: f32,
    last_beat: f64,
    started: Instant,
    test_signal: bool,
    pub frame: VisFrame,
}

impl Analyzer {
    pub fn new(ring: Arc<Mutex<AudioRing>>) -> Self {
        let window = (0..FFT_SIZE).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FFT_SIZE - 1) as f64).cos()) as f32).collect();
        Analyzer {
            ring,
            window,
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            smooth: vec![0.0; BINS],
            bass_avg: 0.0,
            last_beat: 0.0,
            started: Instant::now(),
            // WEBSKIN_TESTSIGNAL=1 feeds made-up music to the visualizers (tests without sound).
            test_signal: std::env::var("WEBSKIN_TESTSIGNAL").is_ok_and(|v| v == "1"),
            frame: VisFrame::default(),
        }
    }

    pub fn analyze(&mut self) -> &VisFrame {
        let sample_rate;
        let silent;
        {
            let ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            sample_rate = ring.sample_rate;
            // The tap delivers nothing while nothing plays; treat that as silence.
            silent = ring.last_data.is_none_or(|t| t.elapsed().as_secs_f64() > 0.2);
            let end = ring.written;
            for i in 0..FFT_SIZE {
                let src = end as i64 - FFT_SIZE as i64 + i as i64;
                let (l, r) = if src >= 0 && !silent {
                    let idx = (src as u64 % RING_SIZE as u64) as usize;
                    (ring.left[idx], ring.right[idx])
                } else {
                    (0.0, 0.0)
                };
                self.re[i] = (l + r) * 0.5 * self.window[i];
                self.im[i] = 0.0;
                if i >= FFT_SIZE - BINS {
                    let w = i - (FFT_SIZE - BINS);
                    self.frame.wave_l[w] = l;
                    self.frame.wave_r[w] = r;
                }
            }
        }
        fft(&mut self.re, &mut self.im);

        let f = &mut self.frame;
        let (mut bass, mut mid, mut treble, mut level) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
        for i in 0..BINS {
            let mag = (self.re[i] * self.re[i] + self.im[i] * self.im[i]).sqrt() * (4.0 / FFT_SIZE as f32);
            let db = 20.0 * (mag + 1e-9).log10();
            let v = ((db + 75.0) / 65.0).clamp(0.0, 1.0);
            // Fast attack, slower release, like WMP's own analyser.
            let s = self.smooth[i];
            let s = if v > s { v } else { s * 0.82 + v * 0.18 };
            self.smooth[i] = s;
            f.spectrum[i] = s;
            let b = (s * 255.0) as u8;
            f.frequency[i] = b;
            f.frequency[BINS + i] = b;
            let hz = i as f32 * bin_hz;
            if hz < 250.0 {
                bass = bass.max(v);
            } else if hz < 2500.0 {
                mid += v;
            } else if hz < 12000.0 {
                treble += v;
            }
        }
        mid /= ((2250.0 / bin_hz) as i32).max(1) as f32;
        treble /= ((9500.0 / bin_hz) as i32).max(1) as f32;
        for i in 0..BINS {
            let (l, r) = (f.wave_l[i], f.wave_r[i]);
            level = level.max(l.abs().max(r.abs()));
            f.waveform[i] = (128.0 + l * 127.0).clamp(0.0, 255.0) as u8;
            f.waveform[BINS + i] = (128.0 + r * 127.0).clamp(0.0, 255.0) as u8;
        }
        let now = self.started.elapsed().as_secs_f64();
        self.bass_avg = self.bass_avg * 0.92 + bass * 0.08;
        f.beat = bass > 0.35 && bass > self.bass_avg * 1.25 && now - self.last_beat > 0.18;
        if f.beat {
            self.last_beat = now;
        }
        f.bass = bass;
        f.mid = (mid * 1.6).clamp(0.0, 1.0);
        f.treble = (treble * 2.2).clamp(0.0, 1.0);
        f.level = level.clamp(0.0, 1.0);
        f.playing = !silent;
        f.time = now;
        f.sample_rate = sample_rate;
        if self.test_signal {
            synthesize(f, now);
        }
        &self.frame
    }
}

fn synthesize(f: &mut VisFrame, t: f64) {
    let beat = 0.5 + 0.5 * (t * std::f64::consts::PI * 4.0).sin();
    for ch in 0..2 {
        for i in 0..BINS {
            let s = (i as f64 * 0.05 + t * 7.0 + ch as f64).sin() * 0.5 + (i as f64 * 0.013 + t * 3.0).sin() * 0.3 * beat;
            f.waveform[ch * BINS + i] = (128.0 + s * 110.0).clamp(0.0, 255.0) as u8;
            let level = 220.0 * (-(i as f64) / 160.0).exp() * (0.55 + 0.45 * (t * 5.0 + i as f64 * 0.03).sin()) * (0.6 + 0.4 * beat);
            f.frequency[ch * BINS + i] = level.clamp(0.0, 255.0) as u8;
            let wave = s as f32;
            if ch == 0 {
                f.wave_l[i] = wave;
                f.spectrum[i] = (level / 255.0).clamp(0.0, 1.0) as f32;
            } else {
                f.wave_r[i] = wave;
            }
        }
    }
    f.bass = f.spectrum[4];
    f.mid = f.spectrum[60];
    f.treble = f.spectrum[300];
    f.level = (0.4 + 0.4 * beat) as f32;
    f.beat = beat > 0.97;
    f.playing = true;
}

/// In-place iterative radix-2 FFT. Length must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos() as f32, ang.sin() as f32);
        for i in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let xr = re[b] * cr - im[b] * ci;
                let xi = re[b] * ci + im[b] * cr;
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let t = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = t;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_lands_in_its_bin_and_silence_reads_as_not_playing() {
        let ring = AudioRing::shared();
        let mut analyzer = Analyzer::new(ring.clone());
        assert!(!analyzer.analyze().playing);
        // 1500 Hz at 48 kHz: bin 1500 / (48000 / 2048) = 64.
        let samples: Vec<f32> = (0..4096)
            .flat_map(|i| {
                let s = (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 48000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        ring.lock().unwrap().push_interleaved(&samples, 2);
        let f = analyzer.analyze();
        assert!(f.playing);
        let peak = (1..BINS).max_by(|&a, &b| f.spectrum[a].total_cmp(&f.spectrum[b])).unwrap();
        assert_eq!(peak, 64);
        assert!(f.level > 0.45);
    }
}
