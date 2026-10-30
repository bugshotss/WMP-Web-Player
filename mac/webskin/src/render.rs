//! A software canvas for skin views: pixel-art images (nearest-neighbour scaling, so Retina screens
//! show crisp skins), colour fills, text, and layers for opacity and clipping masks.

use crate::text::Coverage;
use tiny_skia::{BlendMode, FilterQuality, Mask, Paint, Pattern, Pixmap, PixmapPaint, Shader, SpreadMode, Transform};

/// A rectangle in logical (skin) pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        Rect::new(x, y, (self.right().min(o.right()) - x).max(0.0), (self.bottom().min(o.bottom()) - y).max(0.0))
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }
}

pub struct Canvas {
    pub pixmap: Pixmap,
    /// Device pixels per logical pixel.
    pub scale: f32,
}

impl Canvas {
    /// A transparent canvas for a `width` x `height` logical area.
    pub fn new(width: u32, height: u32, scale: f32) -> Option<Self> {
        let w = ((width.max(1) as f32) * scale).round() as u32;
        let h = ((height.max(1) as f32) * scale).round() as u32;
        Some(Canvas { pixmap: Pixmap::new(w, h)?, scale })
    }

    /// An empty canvas of the same size, for drawing a layer.
    pub fn layer(&self) -> Option<Canvas> {
        Some(Canvas { pixmap: Pixmap::new(self.pixmap.width(), self.pixmap.height())?, scale: self.scale })
    }

    fn device(&self, r: &Rect) -> Option<tiny_skia::Rect> {
        let s = f64::from(self.scale);
        let (x0, y0) = ((r.x * s).round() as f32, (r.y * s).round() as f32);
        let (x1, y1) = ((r.right() * s).round() as f32, (r.bottom() * s).round() as f32);
        tiny_skia::Rect::from_ltrb(x0, y0, x1, y1).filter(|d| d.width() >= 1.0 && d.height() >= 1.0)
    }

    /// Draws `src` (a region of `image`, in image pixels; all of it when None) stretched over `dst`,
    /// showing only the part inside `clip`.
    pub fn draw_image(&mut self, image: &Pixmap, dst: Rect, src: Option<Rect>, clip: Rect, opacity: f32) {
        let src = src.unwrap_or(Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height())));
        if src.is_empty() || dst.is_empty() {
            return;
        }
        let Some(area) = self.device(&dst.intersect(&clip)) else { return };
        let s = f64::from(self.scale);
        let (sx, sy) = (dst.w / src.w * s, dst.h / src.h * s);
        let (tx, ty) = ((dst.x * s).round() - src.x * sx, (dst.y * s).round() - src.y * sy);
        let transform = Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, tx as f32, ty as f32);
        let shader = Pattern::new(image.as_ref(), SpreadMode::Pad, FilterQuality::Nearest, opacity.clamp(0.0, 1.0), transform);
        let paint = Paint { shader, blend_mode: BlendMode::SourceOver, anti_alias: false, ..Default::default() };
        self.pixmap.fill_rect(area, &paint, Transform::identity(), None);
    }

    /// Fills `rect` (inside `clip`) with an opaque RGB colour.
    pub fn fill(&mut self, rect: Rect, rgb: u32, clip: Rect) {
        let Some(area) = self.device(&rect.intersect(&clip)) else { return };
        let paint = Paint { shader: Shader::SolidColor(color(rgb, 255)), anti_alias: false, ..Default::default() };
        self.pixmap.fill_rect(area, &paint, Transform::identity(), None);
    }

    /// Draws rasterized text with its origin at logical (x, y), inside `clip`.
    pub fn draw_text(&mut self, text: &Coverage, x: f64, y: f64, rgb: u32, clip: Rect) {
        let s = f64::from(self.scale);
        let Some(c) = self.device(&clip) else { return };
        let (ox, oy) = ((x * s).round() as i32, (y * s).round() as i32);
        let (cx0, cy0, cx1, cy1) = (c.left() as i32, c.top() as i32, c.right() as i32, c.bottom() as i32);
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
        let pixels = self.pixmap.pixels_mut();
        for &(gx, gy, gw, gh, a) in &text.spans {
            for yy in (oy + gy).max(cy0).max(0)..(oy + gy + gh as i32).min(cy1).min(h) {
                for xx in (ox + gx).max(cx0).max(0)..(ox + gx + gw as i32).min(cx1).min(w) {
                    let p = &mut pixels[(yy * w + xx) as usize];
                    let a = u32::from(a);
                    let inv = 255 - a;
                    let blend = |src: u32, dst: u8| ((src * a + u32::from(dst) * inv) / 255) as u8;
                    let na = (a + u32::from(p.alpha()) * inv / 255).min(255) as u8;
                    let (nr, ng, nb) = (blend(r, p.red()), blend(g, p.green()), blend(b, p.blue()));
                    *p = tiny_skia::PremultipliedColorU8::from_rgba(nr.min(na), ng.min(na), nb.min(na), na).unwrap_or(*p);
                }
            }
        }
    }

    /// Draws a layer over this canvas with an opacity and an optional mask.
    pub fn composite(&mut self, layer: &Canvas, opacity: f32, mask: Option<&Mask>) {
        let paint = PixmapPaint { opacity: opacity.clamp(0.0, 1.0), blend_mode: BlendMode::SourceOver, quality: FilterQuality::Nearest };
        self.pixmap.draw_pixmap(0, 0, layer.pixmap.as_ref(), &paint, Transform::identity(), mask);
    }

    /// A device-sized mask from a logical-pixel alpha lookup placed at (ox, oy).
    pub fn mask(&self, ox: f64, oy: f64, alpha: impl Fn(i32, i32) -> u8) -> Option<Mask> {
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        let mut mask = Mask::new(w, h)?;
        let s = f64::from(self.scale);
        let data = mask.data_mut();
        for y in 0..h {
            let ly = ((f64::from(y) + 0.5) / s - oy).floor() as i32;
            for x in 0..w {
                let lx = ((f64::from(x) + 0.5) / s - ox).floor() as i32;
                data[(y * w + x) as usize] = alpha(lx, ly);
            }
        }
        Some(mask)
    }

    /// Clears every opaque pixel whose colour is one of `keys` (a VIEW's clippingColor and
    /// transparencyColor cut the composed window, whichever element drew them).
    pub fn key_out(&mut self, keys: &[u32]) {
        for p in self.pixmap.pixels_mut() {
            if p.alpha() == 255 {
                let rgb = (u32::from(p.red()) << 16) | (u32::from(p.green()) << 8) | u32::from(p.blue());
                if keys.contains(&rgb) {
                    *p = tiny_skia::PremultipliedColorU8::TRANSPARENT;
                }
            }
        }
    }
}

fn color(rgb: u32, alpha: u8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(((rgb >> 16) & 255) as u8, ((rgb >> 8) & 255) as u8, (rgb & 255) as u8, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_images_scaled_and_clipped() {
        let mut img = Pixmap::new(2, 1).unwrap();
        img.pixels_mut()[0] = tiny_skia::PremultipliedColorU8::from_rgba(255, 0, 0, 255).unwrap();
        img.pixels_mut()[1] = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 255, 255).unwrap();
        let mut c = Canvas::new(4, 2, 2.0).unwrap();
        c.draw_image(&img, Rect::new(1.0, 0.0, 2.0, 1.0), None, Rect::new(0.0, 0.0, 2.0, 2.0), 1.0);
        let px = |x: u32, y: u32| c.pixmap.pixel(x, y).unwrap();
        assert_eq!((px(2, 0).red(), px(3, 1).red()), (255, 255), "first image pixel covers 2x2 device pixels");
        assert_eq!(px(4, 0).alpha(), 0, "clipped at logical x=2");
        assert_eq!(px(1, 0).alpha(), 0);
    }

    #[test]
    fn keys_out_colours() {
        let mut c = Canvas::new(2, 1, 1.0).unwrap();
        c.fill(Rect::new(0.0, 0.0, 1.0, 1.0), 0xFF00FF, Rect::new(0.0, 0.0, 9.0, 9.0));
        c.fill(Rect::new(1.0, 0.0, 1.0, 1.0), 0x123456, Rect::new(0.0, 0.0, 9.0, 9.0));
        c.key_out(&[0xFF00FF]);
        assert_eq!(c.pixmap.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(c.pixmap.pixel(1, 0).unwrap().alpha(), 255);
    }
}
