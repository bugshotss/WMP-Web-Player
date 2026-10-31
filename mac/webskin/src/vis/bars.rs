//! Recreation of WMP's "Bars and Waves" visualization and its four presets.

use super::draw::{PixelBuffer, hsv, rgb, scale};
use super::{BINS, VisContext, VisFrame, Visualization};

#[derive(Default)]
pub struct BarsAndWaves {
    bars: Vec<f32>,
    peaks: Vec<f32>,
    peak_hold: Vec<f32>,
    scratch: Vec<u32>,
    hue: f64,
    rng: u64,
}

impl Visualization for BarsAndWaves {
    fn render(&mut self, f: &VisFrame, p: &mut PixelBuffer, preset: usize, _: &VisContext) {
        match preset {
            1 => self.ocean_mist(f, p),
            2 => self.fire_storm(f, p),
            3 => self.scope(f, p),
            _ => self.bars(f, p),
        }
    }
}

/// Log-spaced band value (0..1) between two fractions of the usable spectrum.
pub fn band(f: &VisFrame, band: usize, bands: usize) -> f32 {
    let (lo, hi) = (40.0f64, 16000.0f64);
    let bin_hz = f.sample_rate as f64 / 2048.0;
    let f0 = lo * (hi / lo).powf(band as f64 / bands as f64);
    let f1 = lo * (hi / lo).powf((band + 1) as f64 / bands as f64);
    let (c0, c1) = (f0 / bin_hz, f1 / bin_hz);
    if c1 - c0 < 1.5 {
        // Narrower than an FFT bin (the bass end): interpolate at the band centre.
        let c = ((c0 + c1) / 2.0).clamp(1.0, (BINS - 2) as f64);
        let i0 = c as usize;
        let t = (c - i0 as f64) as f32;
        return f.spectrum[i0] * (1.0 - t) + f.spectrum[i0 + 1] * t;
    }
    let b0 = (c0 as usize).clamp(1, BINS - 1);
    let b1 = (c1.ceil() as usize).clamp(b0 + 1, BINS);
    f.spectrum[b0..b1].iter().copied().fold(0.0, f32::max)
}

impl BarsAndWaves {
    fn random(&mut self) -> f32 {
        // xorshift: the flames only need to flicker.
        self.rng = if self.rng == 0 { 0x9E37_79B9_7F4A_7C15 } else { self.rng };
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn bars(&mut self, f: &VisFrame, p: &mut PixelBuffer) {
        let (w, h) = (p.width as i32, p.height as i32);
        let n = (w / 7).clamp(12, 80) as usize;
        if self.bars.len() != n {
            self.bars = vec![0.0; n];
            self.peaks = vec![0.0; n];
            self.peak_hold = vec![0.0; n];
        }
        p.clear();
        self.hue += 0.0015;
        let bar_w = w as f32 / n as f32;
        let gap = i32::from(bar_w >= 5.0);
        for i in 0..n {
            let v = band(f, i, n).powf(1.4);
            self.bars[i] = if v > self.bars[i] { v } else { v.max(self.bars[i] - 0.035) };
            if self.bars[i] >= self.peaks[i] {
                self.peaks[i] = self.bars[i];
                self.peak_hold[i] = 18.0;
            } else if self.peak_hold[i] > 0.0 {
                self.peak_hold[i] -= 1.0;
            } else {
                self.peaks[i] = (self.peaks[i] - 0.012).max(0.0);
            }
            let (x0, x1) = ((i as f32 * bar_w) as i32, ((i + 1) as f32 * bar_w) as i32 - gap);
            let top = h - (self.bars[i] * (h - 3) as f32) as i32;
            for y in top..h {
                let t = 1.0 - y as f64 / h as f64;
                let c = hsv(self.hue + 0.55 + t * 0.18, 0.85 - t * 0.5, 0.45 + t * 0.55);
                for x in x0..x1 {
                    p.set(x, y, c);
                }
            }
            let py = h - 2 - (self.peaks[i] * (h - 3) as f32) as i32;
            p.fill_rect(x0, py, (x1 - x0).max(1), 2, rgb(230, 235, 255));
        }
    }

    fn ocean_mist(&mut self, f: &VisFrame, p: &mut PixelBuffer) {
        let (w, h) = (p.width as f32, p.height as f32);
        self.drift_and_blur(p, -1, 0.93);
        self.hue += 0.002;
        let n = (p.width / 3).max(16);
        const BANDS: usize = 48;
        let mut bv = [0.0f32; BANDS + 1];
        for (b, slot) in bv.iter_mut().enumerate().take(BANDS) {
            *slot = band(f, b, BANDS);
        }
        bv[BANDS] = bv[BANDS - 1];
        let mid = h * 0.5;
        let (mut px, mut py_top, mut py_bot) = (0.0, mid, mid);
        for i in 0..=n {
            // Smoothstep between neighbouring bands so the mist is a curve, not steps.
            let pos = i as f32 / n as f32 * (BANDS - 1) as f32;
            let b0 = pos as usize;
            let t = pos - b0 as f32;
            let t = t * t * (3.0 - 2.0 * t);
            let v = bv[b0] * (1.0 - t) + bv[b0 + 1] * t;
            let x = i as f32 * w / n as f32;
            let amp = v * h * 0.42;
            let (y_top, y_bot) = (mid - amp, mid + amp);
            let c = hsv(self.hue + 0.5 + f64::from(v) * 0.12, 0.55, 0.35 + f64::from(v) * 0.65);
            if i > 0 {
                p.line(px, py_top, x, y_top, c, 1.5);
                p.line(px, py_bot, x, y_bot, scale(c, 0.7), 1.5);
            }
            (px, py_top, py_bot) = (x, y_top, y_bot);
        }
    }

    fn fire_storm(&mut self, f: &VisFrame, p: &mut PixelBuffer) {
        let (w, h) = (p.width as f32, p.height as f32);
        self.drift_and_blur(p, -2, 0.90);
        let n = (p.width / 2).max(16);
        for i in 0..n {
            let v = band(f, (i as f32 / n as f32 * 40.0) as usize, 40);
            let x = i as f32 * w / n as f32;
            let flame = v * h * 0.55;
            for _ in 0..2 {
                let y = h - 1.0 - self.random() * flame;
                let c = rgb(255, (120.0 + 135.0 * v) as i32, (60.0 * v) as i32);
                let dx = self.random() * 2.0;
                p.dot(x + dx, y, 1.5 + v * 2.5, c);
            }
        }
    }

    fn scope(&mut self, f: &VisFrame, p: &mut PixelBuffer) {
        let (w, h) = (p.width, p.height as f32);
        p.fade(0.55, 4);
        self.hue += 0.004;
        let c = hsv(self.hue, 0.7, 1.0);
        let (mid, amp) = (h * 0.5, h * 0.45);
        let (mut lx, mut ly) = (0.0, mid);
        for x in 0..w {
            let i = (x as f32 / w as f32 * (BINS - 1) as f32) as usize;
            let y = mid - (f.wave_l[i] + f.wave_r[i]) * 0.5 * amp * 1.6;
            if x > 0 {
                p.line(lx, ly, x as f32, y, c, 1.4);
            }
            (lx, ly) = (x as f32, y);
        }
    }

    /// Scrolls the image vertically by `dy` with a light blur and fade.
    fn drift_and_blur(&mut self, p: &mut PixelBuffer, dy: i32, decay: f32) {
        let (w, h) = (p.width, p.height);
        self.scratch.resize(w * h, 0);
        let k = (decay * 256.0) as u32;
        for y in 0..h {
            let sy = y as i32 - dy;
            for x in 0..w {
                if sy < 0 || sy >= h as i32 {
                    self.scratch[y * w + x] = 0;
                    continue;
                }
                let row = sy as usize * w;
                let (a, b, c) = (p.pixels[row + x.saturating_sub(1)], p.pixels[row + x], p.pixels[row + (x + 1).min(w - 1)]);
                let ch = |s: u32| (((((a >> s) & 255) + 2 * ((b >> s) & 255) + ((c >> s) & 255)) * k) >> 10).saturating_sub(1);
                self.scratch[y * w + x] = ch(16) << 16 | ch(8) << 8 | ch(0);
            }
        }
        p.pixels.copy_from_slice(&self.scratch);
    }
}
