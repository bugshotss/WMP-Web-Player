//! Software drawing for the built-in visualizations, on 0x00RRGGBB pixels.

pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        PixelBuffer { width, height, pixels: vec![0; width * height] }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    pub fn set(&mut self, x: i32, y: i32, color: u32) {
        if (x as u32) < self.width as u32 && (y as u32) < self.height as u32 {
            self.pixels[y as usize * self.width + x as usize] = color;
        }
    }

    pub fn add(&mut self, x: i32, y: i32, color: u32) {
        if (x as u32) < self.width as u32 && (y as u32) < self.height as u32 {
            let px = &mut self.pixels[y as usize * self.width + x as usize];
            *px = add_sat(*px, color);
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let (x1, y1) = ((x + w).clamp(0, self.width as i32) as usize, (y + h).clamp(0, self.height as i32) as usize);
        for yy in y0..y1 {
            if x0 < x1 {
                self.pixels[yy * self.width + x0..yy * self.width + x1].fill(color);
            }
        }
    }

    /// Anti-aliased-ish additive line (Wu-style coverage, configurable thickness).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32, thickness: f32) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let steps = (dx.abs().max(dy.abs()).ceil() as i32).max(1);
        let half = (thickness / 2.0).max(0.5);
        let steep = dy.abs() > dx.abs();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let (x, y) = (x0 + dx * t, y0 + dy * t);
            let (along, across) = if steep { (y, x) } else { (x, y) };
            for a in (across - half).floor() as i32..=(across + half).ceil() as i32 {
                let cov = (half + 0.5 - (a as f32 - across).abs()).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let c = scale(color, cov);
                    if steep { self.add(a, along as i32, c) } else { self.add(along as i32, a, c) }
                }
            }
        }
    }

    /// Additive soft round dot.
    pub fn dot(&mut self, cx: f32, cy: f32, radius: f32, color: u32) {
        let r2 = radius * radius;
        for y in (cy - radius - 1.0) as i32..=(cy + radius + 1.0) as i32 {
            for x in (cx - radius - 1.0) as i32..=(cx + radius + 1.0) as i32 {
                let d2 = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                if d2 <= r2 {
                    let k = 1.0 - d2 / r2;
                    self.add(x, y, scale(color, k * k));
                }
            }
        }
    }

    /// Multiplies every pixel by `k` and subtracts a floor so trails die out.
    pub fn fade(&mut self, k: f32, floor: i32) {
        let kk = (k * 256.0) as i32;
        for px in self.pixels.iter_mut().filter(|p| **p != 0) {
            let ch = |shift: u32| (((((*px >> shift) & 255) as i32 * kk) >> 8) - floor).max(0) as u32;
            *px = ch(16) << 16 | ch(8) << 8 | ch(0);
        }
    }
}

pub fn rgb(r: i32, g: i32, b: i32) -> u32 {
    (r.clamp(0, 255) as u32) << 16 | (g.clamp(0, 255) as u32) << 8 | b.clamp(0, 255) as u32
}

pub fn hsv(h: f64, s: f64, v: f64) -> u32 {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h as i32;
    let f = h - i as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    rgb((r * 255.0) as i32, (g * 255.0) as i32, (b * 255.0) as i32)
}

pub fn scale(c: u32, k: f32) -> u32 {
    let ch = |shift: u32| (((c >> shift) & 255) as f32 * k) as i32;
    rgb(ch(16), ch(8), ch(0))
}

pub fn lerp(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let (x, y) = (((a >> shift) & 255) as f32, ((b >> shift) & 255) as f32);
        (x + (y - x) * t) as i32
    };
    rgb(ch(16), ch(8), ch(0))
}

fn add_sat(a: u32, b: u32) -> u32 {
    let ch = |shift: u32| (((a >> shift) & 255) + ((b >> shift) & 255)).min(255);
    ch(16) << 16 | ch(8) << 8 | ch(0)
}
