//! Battery-style visualization: every frame warps the previous frame (zoom / rotate / swirl /
//! drift / ripple), fades it, and draws fresh audio-driven shapes on top.

use super::draw::{PixelBuffer, hsv};
use super::{BINS, VisContext, VisFrame, Visualization};
use std::f64::consts::TAU;

/// Whole-frame warping is per-pixel work; WMP's own Battery also ran at a reduced resolution.
pub const MAX_PIXELS: usize = 560 * 350;

#[derive(Default)]
pub struct Battery {
    back: Vec<u32>,
    row_ripple: Vec<f32>,
    col_ripple: Vec<f32>,
    hue: f64,
    spin: f64,
    rng: u64,
    sparks: Vec<Spark>,
}

struct Spark {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    color: u32,
    life: f32,
}

struct Warp {
    zoom: f32,
    rotation: f32,
    swirl: f32,
    dy: f32,
    ripple: f32,
    ripple_freq: f32,
    decay: f32,
}

impl Visualization for Battery {
    fn render(&mut self, f: &VisFrame, p: &mut PixelBuffer, preset: usize, _: &VisContext) {
        self.hue += 0.0012 + f64::from(f.level) * 0.004;
        self.spin += 0.01 + f64::from(f.bass) * 0.03;
        let mut wp = Warp { zoom: 1.0, rotation: 0.0, swirl: 0.0, dy: 0.0, ripple: 0.0, ripple_freq: 0.0, decay: 0.95 };
        match preset {
            0 => (wp.zoom, wp.rotation, wp.swirl, wp.decay) = (0.975, 0.015 + f.bass * 0.04, 0.07, 0.955),
            1 => (wp.zoom, wp.rotation, wp.decay) = (0.94 - f.bass * 0.03, 0.004, 0.94),
            2 => (wp.zoom, wp.rotation, wp.ripple, wp.ripple_freq, wp.decay) = (0.995, 0.012, 0.6, 0.05, 0.965),
            3 => (wp.dy, wp.ripple, wp.ripple_freq, wp.decay, wp.zoom) = (-1.6 - f.mid * 2.0, 1.6, 0.035, 0.95, 0.998),
            4 => (wp.zoom, wp.rotation, wp.decay) = (0.985, 0.03 * (self.spin as f32 * 0.3).sin(), 0.94),
            5 => (wp.zoom, wp.decay) = (1.035, 0.90),
            6 => (wp.zoom, wp.rotation, wp.swirl, wp.decay) = (1.025 + f.bass * 0.02, -0.012, -0.05, 0.955),
            7 => (wp.zoom, wp.rotation, wp.decay) = (0.955, 0.006, 0.93),
            _ => {}
        }
        self.warp(p, &wp, f.time);
        self.draw_shapes(f, p, preset);
        if preset == 3 {
            mirror_x(p);
        }
        if preset == 4 {
            mirror_x(p);
            mirror_y(p);
        }
        if preset == 5 {
            pixelate(p, (p.width.min(p.height) / 40).max(4));
        }
    }

    fn max_pixels(&self) -> usize {
        MAX_PIXELS
    }
}

impl Battery {
    fn random(&mut self) -> f64 {
        self.rng = if self.rng == 0 { 7 } else { self.rng };
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn warp(&mut self, p: &mut PixelBuffer, wp: &Warp, time: f64) {
        let (w, h) = (p.width, p.height);
        self.back.clear();
        self.back.extend_from_slice(&p.pixels);
        let t = time as f32 * 2.2;
        self.row_ripple = (0..h).map(|y| wp.ripple * (y as f32 * wp.ripple_freq + t).sin()).collect();
        self.col_ripple = (0..w).map(|x| wp.ripple * (x as f32 * wp.ripple_freq * 1.3 + t * 0.8).cos()).collect();
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let rmax = (cx * cx + cy * cy).sqrt();
        let (cos_r, sin_r) = (wp.rotation.cos(), wp.rotation.sin());
        let decay = (wp.decay * 256.0) as u32;
        for y in 0..h {
            let dy0 = y as f32 - cy;
            for x in 0..w {
                let dx0 = x as f32 - cx;
                let (c, s) = if wp.swirl != 0.0 {
                    let r = (dx0 * dx0 + dy0 * dy0).sqrt() / rmax;
                    let a = wp.rotation + wp.swirl * (1.0 - r) * (1.0 - r);
                    (a.cos(), a.sin())
                } else {
                    (cos_r, sin_r)
                };
                let u = cx + (dx0 * c - dy0 * s) * wp.zoom + self.row_ripple[y];
                let v = cy + (dx0 * s + dy0 * c) * wp.zoom - wp.dy + self.col_ripple[x];
                p.pixels[y * w + x] = sample(&self.back, w, h, u, v, decay);
            }
        }
    }

    fn draw_shapes(&mut self, f: &VisFrame, p: &mut PixelBuffer, preset: usize) {
        let (w, h) = (p.width as f32, p.height as f32);
        let (cx, cy, m) = (w / 2.0, h / 2.0, w.min(h));
        let polar = |a: f64, r: f32| (cx + a.cos() as f32 * r, cy + a.sin() as f32 * r);
        match preset {
            0 => {
                // Whirlpool: spectrum ring
                let n = 64;
                for i in 0..n {
                    let v = f.spectrum[2 + i * 6];
                    let (x, y) = polar(i as f64 / n as f64 * TAU + self.spin, m * 0.18 + v * m * 0.22);
                    p.dot(x, y, 1.2 + v * 2.5, hsv(self.hue + i as f64 / n as f64 * 0.3, 0.9, 0.6 + f64::from(v) * 0.4));
                }
            }
            1 => {
                // Event Horizon: waveform circle
                let n = 180;
                let c = hsv(self.hue + 0.6, 0.35, 1.0);
                let mut prev = (0.0, 0.0);
                for i in 0..=n {
                    let r = m * 0.22 * (1.0 + f.wave_l[(i * 5) % BINS] * 0.8);
                    let pt = polar(i as f64 / n as f64 * TAU, r);
                    if i > 0 {
                        p.line(prev.0, prev.1, pt.0, pt.1, c, 1.3);
                    }
                    prev = pt;
                }
                if f.beat {
                    p.dot(cx, cy, m * 0.12, hsv(self.hue, 0.6, 0.7));
                }
            }
            2 => {
                // Spider Silk: radial threads
                let n = 24;
                for i in 0..n {
                    let v = f.spectrum[3 + i * 8].max(f.spectrum[4 + i * 8]);
                    let (x, y) = polar(i as f64 / n as f64 * TAU + self.spin * 0.2, m * (0.08 + v * 0.45));
                    p.line(cx, cy, x, y, hsv(self.hue + 0.75, 0.25, 0.5 + f64::from(v) * 0.5), 1.0);
                }
            }
            3 => {
                // Smoke and Mirrors: waveform near the bottom (mirrored later)
                let y0 = h * 0.78;
                let c = hsv(self.hue + 0.08, 0.8, 0.9);
                let mut prev = (0.0, y0);
                for x in (0..=p.width / 2).step_by(2) {
                    let y = y0 + f.wave_l[(x * 3) % BINS] * h * 0.25;
                    if x > 0 {
                        p.line(prev.0, prev.1, x as f32, y, c, 2.0);
                    }
                    prev = (x as f32, y);
                }
            }
            4 => {
                // Kaleidoscope: spectrum confetti in one quadrant (mirrored later)
                for _ in 0..24 {
                    let bin = 2 + (self.random() * 200.0) as usize;
                    let v = f.spectrum[bin];
                    if v < 0.25 {
                        continue;
                    }
                    let (x, y) = (self.random() as f32 * cx, self.random() as f32 * cy);
                    p.dot(x, y, 1.0 + v * m * 0.025, hsv(self.hue + bin as f64 / 300.0, 0.85, f64::from(v)));
                }
            }
            5 => {
                // Pixel-ate: blocks driven by bands
                let bands = 16;
                for i in 0..bands {
                    let v = f.spectrum[2 + i * 12];
                    if v < 0.3 {
                        continue;
                    }
                    let (x, y) = (self.random() as f32 * w, self.random() as f32 * h);
                    let s = (m * 0.03 + v * m * 0.06) as i32;
                    p.fill_rect(x as i32, y as i32, s, s, hsv(self.hue + i as f64 / bands as f64, 0.9, f64::from(v)));
                }
            }
            6 => {
                // Gravity: sparks thrown from the rim, pulled inward by the warp
                if f.beat || self.random() < f64::from(f.level) * 0.5 {
                    for _ in 0..6 {
                        let (x, y) = polar(self.random() * TAU, m * 0.45);
                        let c = hsv(self.hue + self.random() * 0.2, 0.8, 1.0);
                        p.dot(x, y, 2.0 + f.bass * 5.0, c);
                    }
                }
                let mut prev = (0.0, 0.0);
                for i in 0..=120 {
                    let pt = polar(i as f64 / 120.0 * TAU, m * 0.4 * (1.0 + f.wave_r[(i * 8) % BINS] * 0.3));
                    if i > 0 {
                        p.line(prev.0, prev.1, pt.0, pt.1, hsv(self.hue + 0.5, 0.6, 0.6), 1.0);
                    }
                    prev = pt;
                }
            }
            7 => {
                // Starburst: sparks flying out from the centre
                let spawn = (2.0 + f.level * 10.0) as usize + if f.beat { 30 } else { 0 };
                for _ in 0..spawn {
                    if self.sparks.len() >= 600 {
                        break;
                    }
                    let a = self.random() * TAU;
                    let sp = (0.5 + self.random() * 2.5) as f32 * (m / 200.0) * (1.0 + f.bass);
                    let color = hsv(self.hue + self.random() * 0.25, 0.7, 1.0);
                    self.sparks.push(Spark { x: cx, y: cy, vx: a.cos() as f32 * sp, vy: a.sin() as f32 * sp, color, life: 1.0 });
                }
                self.sparks.retain_mut(|s| {
                    s.x += s.vx;
                    s.y += s.vy;
                    s.vx *= 1.02;
                    s.vy *= 1.02;
                    s.life -= 0.012;
                    let alive = s.life > 0.0 && s.x >= 0.0 && s.y >= 0.0 && s.x < w && s.y < h;
                    if alive {
                        p.add(s.x as i32, s.y as i32, super::draw::scale(s.color, s.life));
                    }
                    alive
                });
            }
            _ => {}
        }
    }
}

fn sample(src: &[u32], w: usize, h: usize, u: f32, v: f32, decay: u32) -> u32 {
    if u < 0.0 || v < 0.0 || u >= (w - 1) as f32 || v >= (h - 1) as f32 {
        return 0;
    }
    let (x0, y0) = (u as usize, v as usize);
    let (fx, fy) = (((u - x0 as f32) * 256.0) as u32, ((v - y0 as f32) * 256.0) as u32);
    let i = y0 * w + x0;
    let (c00, c10, c01, c11) = (src[i], src[i + 1], src[i + w], src[i + w + 1]);
    let ch = |s: u32| {
        let top = ((c00 >> s) & 255) * (256 - fx) + ((c10 >> s) & 255) * fx;
        let bot = ((c01 >> s) & 255) * (256 - fx) + ((c11 >> s) & 255) * fx;
        ((((top * (256 - fy) + bot * fy) >> 16) * decay) >> 8).saturating_sub(1)
    };
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn mirror_x(p: &mut PixelBuffer) {
    let w = p.width;
    for row in p.pixels.chunks_exact_mut(w) {
        for x in 0..w / 2 {
            row[w - 1 - x] = row[x];
        }
    }
}

fn mirror_y(p: &mut PixelBuffer) {
    let (w, h) = (p.width, p.height);
    for y in 0..h / 2 {
        p.pixels.copy_within(y * w..(y + 1) * w, (h - 1 - y) * w);
    }
}

fn pixelate(p: &mut PixelBuffer, block: usize) {
    let (w, h) = (p.width, p.height);
    for by in (0..h).step_by(block) {
        for bx in (0..w).step_by(block) {
            let c = p.pixels[(by + block / 2).min(h - 1) * w + (bx + block / 2).min(w - 1)];
            for y in by..(by + block).min(h) {
                p.pixels[y * w + bx..y * w + (bx + block).min(w)].fill(c);
            }
        }
    }
}
