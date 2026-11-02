//! WMP 11-style "Album Art" view: the track's artwork with title and artist.

use super::draw::{PixelBuffer, hsv, lerp, rgb};
use super::{VisContext, VisFrame, Visualization};
use crate::text::{self, Align, TextStyle};

#[derive(Default)]
pub struct AlbumArt {
    /// What the picture shows now: (artwork version, preset, width, height).
    drawn: Option<(u32, usize, usize, usize)>,
}

impl Visualization for AlbumArt {
    fn render(&mut self, _: &VisFrame, p: &mut PixelBuffer, preset: usize, ctx: &VisContext) {
        // Static content: only redraw when the track, preset or size changes.
        let now = (ctx.version, preset, p.width, p.height);
        if self.drawn == Some(now) {
            return;
        }
        self.drawn = Some(now);
        let (w, h) = (p.width, p.height);
        for y in 0..h {
            p.pixels[y * w..(y + 1) * w].fill(lerp(rgb(10, 18, 40), 0, y as f32 / h as f32));
        }
        let info = preset == 0 && h > 60;
        let text_space = if info { (h / 6).max(28) } else { 0 };
        let size = (w as i32 - 8).min(h as i32 - text_space as i32 - 8).max(8) as usize;
        let (ox, oy) = ((w as i32 - size as i32) / 2, (h as i32 - text_space as i32 - size as i32) / 2);
        match &ctx.artwork {
            Some(art) if art.width > 0 && art.height > 0 => {
                let (aw, ah) = (art.width as usize, art.height as usize);
                // Box-filtered downscale (or nearest upscale) into the square.
                for y in 0..size {
                    let (sy0, sy1) = (y * ah / size, ((y + 1) * ah / size).max(y * ah / size + 1));
                    for x in 0..size {
                        let (sx0, sx1) = (x * aw / size, ((x + 1) * aw / size).max(x * aw / size + 1));
                        let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                        for sy in sy0..sy1 {
                            for &c in &art.argb[sy * aw + sx0..sy * aw + sx1] {
                                (r, g, b, n) = (r + ((c >> 16) & 255), g + ((c >> 8) & 255), b + (c & 255), n + 1);
                            }
                        }
                        p.set(ox + x as i32, oy + y as i32, rgb((r / n) as i32, (g / n) as i32, (b / n) as i32));
                    }
                }
            }
            _ => {
                // Placeholder disc, like WMP's default album art.
                let (cx, cy, rad) = (ox as f32 + size as f32 / 2.0, oy as f32 + size as f32 / 2.0, size as f32 * 0.42);
                for y in oy..oy + size as i32 {
                    for x in ox..ox + size as i32 {
                        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / rad;
                        if d <= 1.0 {
                            let k = if d < 0.12 { 0.1 } else { 0.35 + 0.5 * (1.0 - d) + 0.15 * (d * 60.0).sin() };
                            p.set(x, y, hsv(0.6, 0.35, f64::from(k.clamp(0.0, 1.0))));
                        }
                    }
                }
            }
        }
        if info {
            let line = if ctx.artist.is_empty() { ctx.title.clone() } else { format!("{} - {}", ctx.title, ctx.artist) };
            draw_text(p, &line, (h - text_space) as i32, text_space as i32, (text_space / 2).clamp(11, 22) as f32);
        }
    }
}

/// One centred, ellipsized line of light text in the band starting at `top`.
fn draw_text(p: &mut PixelBuffer, line: &str, top: i32, band: i32, px: f32) {
    if line.is_empty() {
        return;
    }
    let style = TextStyle { face: "Helvetica Neue".into(), px, bold: false, italic: false, underline: false };
    let width = p.width as f32 - 8.0;
    let line = text::ellipsize(line, &style, width);
    let (_, line_h) = text::measure(&line, &style, None);
    let coverage = text::rasterize(&line, &style, 1.0, Some((width, Align::Center)));
    let (ox, oy) = (4, top + (band - line_h as i32) / 2);
    for (x, y, w, h, a) in coverage.spans {
        let ink = super::draw::scale(rgb(240, 240, 240), f32::from(a) / 255.0);
        for yy in y..y + h as i32 {
            for xx in x..x + w as i32 {
                p.add(ox + xx, oy + yy, ink);
            }
        }
    }
}
