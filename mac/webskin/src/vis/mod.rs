//! Visualizations: recreations of WMP's built-in Bars and Waves, Battery and Album Art, all fed
//! by one analysis of what the Mac is playing.

mod album_art;
mod bars;
mod battery;
mod draw;
mod frame;

use crate::images::Artwork;
use draw::PixelBuffer;
pub use frame::{Analyzer, AudioRing, BINS, VisFrame};
use std::rc::Rc;
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// What a visualization may want to know about the current track.
#[derive(Default)]
pub struct VisContext {
    pub title: String,
    pub artist: String,
    pub artwork: Option<Rc<Artwork>>,
    /// Bumped whenever the track or its artwork changes.
    pub version: u32,
}

pub trait Visualization {
    fn render(&mut self, frame: &VisFrame, target: &mut PixelBuffer, preset: usize, ctx: &VisContext);
    /// Largest internal resolution worth rendering at (0 = no preference); output is scaled up.
    fn max_pixels(&self) -> usize {
        0
    }
}

pub struct Descriptor {
    /// Stable id used by settings and skins ("Bars", "Battery").
    pub id: &'static str,
    pub title: &'static str,
    /// The class id WMP reported in effectType, when the original had one.
    pub class_id: Option<&'static str>,
    pub presets: &'static [&'static str],
    create: fn() -> Box<dyn Visualization>,
}

impl Descriptor {
    /// What skins see in effectType: the class id when there is one.
    pub fn key(&self) -> &'static str {
        self.class_id.unwrap_or(self.id)
    }
}

pub const ALL: [Descriptor; 3] = [
    Descriptor {
        id: "Bars",
        title: "Bars and Waves",
        class_id: Some("{48501FF0-F6A9-11D2-9435-00A0C92A2F2D}"),
        presets: &["Bars", "Ocean Mist", "Fire Storm", "Scope"],
        create: || Box::new(bars::BarsAndWaves::default()),
    },
    Descriptor {
        id: "Battery",
        title: "Battery",
        class_id: Some("{8D2A317B-D98B-4efa-89D6-E49F0ACF98A1}"),
        presets: &["Whirlpool", "Event Horizon", "Spider Silk", "Smoke and Mirrors", "Kaleidoscope", "Pixel-ate", "Gravity", "Starburst"],
        create: || Box::new(battery::Battery::default()),
    },
    Descriptor {
        id: "AlbumArt",
        title: "Album Art",
        class_id: None,
        presets: &["Album Art and Info", "Album Art"],
        create: || Box::new(album_art::AlbumArt::default()),
    },
];

/// Resolves an id, title or class id (with or without braces), or one of the names older WMP
/// versions and skins used, to an index into [`ALL`].
pub fn find(key: &str) -> Option<usize> {
    let k = key.trim();
    if k.is_empty() {
        return None;
    }
    let bare = k.trim_matches(['{', '}']);
    if let Some(i) = ALL.iter().position(|d| {
        d.id.eq_ignore_ascii_case(k)
            || d.title.eq_ignore_ascii_case(k)
            || d.class_id.is_some_and(|c| c.trim_matches(['{', '}']).eq_ignore_ascii_case(bare))
    }) {
        return Some(i);
    }
    // WMP 7/XP shipped Spikes, Ambience, Musical Colors, Particle and Plenoptic; map them onto the
    // closest effect we have.
    let lower = k.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&["bars", "wave", "spike", "scope"]) {
        Some(0)
    } else if has(&["battery", "ambience", "musical", "particle", "plenoptic"]) {
        Some(1)
    } else if has(&["album"]) {
        Some(2)
    } else {
        None
    }
}

/// Upper bound on a skin visualizer's internal resolution; larger areas are scaled up.
const MAX_PIXELS: usize = 640 * 480;

/// One visualization instance plus the picture it renders into.
pub struct Renderer {
    pub effect: usize,
    pub preset: usize,
    vis: Box<dyn Visualization>,
    buffer: PixelBuffer,
    pub image: Option<Pixmap>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Renderer({} / {})", ALL[self.effect].title, ALL[self.effect].presets[self.preset])
    }
}

impl Renderer {
    pub fn new(effect: usize, preset: usize) -> Self {
        let effect = effect.min(ALL.len() - 1);
        Renderer {
            effect,
            preset: preset.min(ALL[effect].presets.len() - 1),
            vis: (ALL[effect].create)(),
            buffer: PixelBuffer::new(1, 1),
            image: None,
        }
    }

    pub fn set(&mut self, effect: usize, preset: usize) {
        if effect != self.effect && effect < ALL.len() {
            *self = Renderer::new(effect, preset);
        }
        self.preset = preset.min(ALL[self.effect].presets.len() - 1);
    }

    /// Draws the next frame for an area of `width` x `height` device pixels.
    pub fn render(&mut self, frame: &VisFrame, ctx: &VisContext, width: usize, height: usize) {
        let hint = self.vis.max_pixels();
        let cap = if hint > 0 { hint.min(MAX_PIXELS) } else { MAX_PIXELS };
        let (width, height) = (width.max(1), height.max(1));
        let k = (cap as f64 / (width * height) as f64).min(1.0).sqrt();
        let (w, h) = (((width as f64 * k) as usize).max(1), ((height as f64 * k) as usize).max(1));
        if self.buffer.width != w || self.buffer.height != h {
            self.buffer = PixelBuffer::new(w, h);
        }
        self.vis.render(frame, &mut self.buffer, self.preset, ctx);
        let image = match self.image.take() {
            Some(img) if img.width() as usize == w && img.height() as usize == h => img,
            _ => match Pixmap::new(w as u32, h as u32) {
                Some(img) => img,
                None => return,
            },
        };
        let mut image = image;
        for (dst, &c) in image.pixels_mut().iter_mut().zip(&self.buffer.pixels) {
            if let Some(px) = PremultipliedColorU8::from_rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255) {
                *dst = px;
            }
        }
        self.image = Some(image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_effects_by_id_title_class_id_and_old_names() {
        assert_eq!(find("Battery"), Some(1));
        assert_eq!(find("bars and waves"), Some(0));
        assert_eq!(find("48501ff0-f6a9-11d2-9435-00a0c92a2f2d"), Some(0));
        assert_eq!(find("{8D2A317B-D98B-4EFA-89D6-E49F0ACF98A1}"), Some(1));
        assert_eq!(find("Spikes"), Some(0));
        assert_eq!(find("Ambience"), Some(1));
        assert_eq!(find("random"), None);
    }

    #[test]
    fn every_preset_draws_something_from_music() {
        let ring = AudioRing::shared();
        // A bass tone over broadband noise, like music.
        let mut seed = 1u32;
        let samples: Vec<f32> = (0..8192)
            .flat_map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                let s = (i as f32 / 48000.0 * 2.0 * std::f32::consts::PI * 110.0).sin() * 0.5 + noise * 0.4;
                [s, s * 0.8]
            })
            .collect();
        let mut analyzer = Analyzer::new(ring.clone());
        let ctx = VisContext { title: "Glue".into(), artist: "Bicep".into(), ..Default::default() };
        for (effect, d) in ALL.iter().enumerate() {
            for preset in 0..d.presets.len() {
                let mut r = Renderer::new(effect, preset);
                for _ in 0..30 {
                    ring.lock().unwrap().push_interleaved(&samples[..1600], 2);
                    let frame = analyzer.analyze();
                    r.render(frame, &ctx, 200, 120);
                }
                let img = r.image.as_ref().unwrap();
                let lit = img.pixels().iter().filter(|p| p.red() > 20 || p.green() > 20 || p.blue() > 20).count();
                assert!(lit > 50, "{} / {} drew {lit} lit pixels", d.title, d.presets[preset]);
            }
        }
    }
}
