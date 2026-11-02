//! Recolours a visualizer's finished picture, alike for every visualization: a palette (the picture's
//! brightness mapped onto a gradient), a hue shift, or a colour of your own. Kept per visualization in
//! the settings as a spec: "palette:Fire", "hue:120" or "custom:#RRGGBB"; none means the picture as the
//! visualization drew it. The same choices and maths as the Windows app's VisualizationColors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Gradients from black (silence) to the brightest parts of the picture.
pub const PALETTES: &[(&str, &[u32])] = &[
    ("Fire", &[0x000000, 0x5A0000, 0xC81E00, 0xFF7A00, 0xFFD24A, 0xFFFBE6]),
    ("Sunset", &[0x000000, 0x2B0A3D, 0x8C1C5A, 0xE8505B, 0xF9A03F, 0xFFE7A0]),
    ("Gold", &[0x000000, 0x2E1F00, 0x7A5200, 0xD19A1E, 0xF7D774, 0xFFF8DC]),
    ("Rose", &[0x000000, 0x3D0A1E, 0x9E1F4F, 0xE85C8A, 0xF9B8CF, 0xFFF0F5]),
    ("Neon", &[0x000000, 0x2A0055, 0x9B00D9, 0xFF2FB3, 0x3FF2FF, 0xF0FFFF]),
    ("Ice", &[0x000000, 0x001A4D, 0x0059B3, 0x3FA9F5, 0xBFE9FF, 0xFFFFFF]),
    ("Ocean", &[0x000000, 0x021A2E, 0x07507A, 0x0E9AA7, 0x7FE3D6, 0xEFFFFB]),
    ("Emerald", &[0x000000, 0x00261A, 0x00733F, 0x19C26B, 0x9CF5B5, 0xF2FFF6]),
    ("Green Screen", &[0x000000, 0x002B0B, 0x00A02E, 0x33FF66, 0xCCFFD9]),
    ("Amber Screen", &[0x000000, 0x2E1600, 0xA85A00, 0xFFB000, 0xFFE6A8]),
    ("Black and White", &[0x000000, 0xFFFFFF]),
];

/// The hue shifts offered under Shift Colors, in degrees.
pub const HUE_SHIFTS: [u32; 5] = [60, 120, 180, 240, 300];

/// A per-pixel colour transform over 0x00RRGGBB pixels, built from lookup tables.
pub enum ColorMap {
    /// Brightness (0-255) to colour.
    Gradient(Box<[u32; 256]>),
    /// Each output channel as the sum of three tables, one per input channel, in 1/1024ths.
    Hue(Box<[i32; 9 * 256]>),
}

impl ColorMap {
    pub fn gradient(stops: &[u32]) -> ColorMap {
        let stops = if stops.len() < 2 { &[0x000000, 0xFFFFFF][..] } else { stops };
        let mut lut = Box::new([0u32; 256]);
        let channel = |c: u32, shift: u32| f64::from((c >> shift) & 255);
        for (i, out) in lut.iter_mut().enumerate() {
            let t = i as f64 / 255.0 * (stops.len() - 1) as f64;
            let k = (t as usize).min(stops.len() - 2);
            let f = t - k as f64;
            let (a, b) = (stops[k], stops[k + 1]);
            let mix = |shift: u32| (channel(a, shift) + (channel(b, shift) - channel(a, shift)) * f).round() as u32;
            *out = (mix(16) << 16) | (mix(8) << 8) | mix(0);
        }
        ColorMap::Gradient(lut)
    }

    /// Rotates hue and keeps brightness (the matrix of SVG's hueRotate filter).
    pub fn hue_shift(degrees: f64) -> ColorMap {
        let a = degrees.to_radians();
        let (cos, sin) = (a.cos(), a.sin());
        let m = [
            [0.213 + cos * 0.787 - sin * 0.213, 0.715 - cos * 0.715 - sin * 0.715, 0.072 - cos * 0.072 + sin * 0.928],
            [0.213 - cos * 0.213 + sin * 0.143, 0.715 + cos * 0.285 + sin * 0.140, 0.072 - cos * 0.072 - sin * 0.283],
            [0.213 - cos * 0.213 - sin * 0.787, 0.715 - cos * 0.715 + sin * 0.715, 0.072 + cos * 0.928 + sin * 0.072],
        ];
        let mut tables = Box::new([0i32; 9 * 256]);
        for (row, r) in m.iter().enumerate() {
            for (col, k) in r.iter().enumerate() {
                for v in 0..256 {
                    tables[(row * 3 + col) * 256 + v] = (k * v as f64 * 1024.0).round() as i32;
                }
            }
        }
        ColorMap::Hue(tables)
    }

    /// Recolours 0x00RRGGBB pixels in place.
    pub fn apply(&self, pixels: &mut [u32]) {
        match self {
            ColorMap::Gradient(lut) => {
                for p in pixels {
                    let c = *p;
                    let luma = (((c >> 16) & 255) * 77 + ((c >> 8) & 255) * 150 + (c & 255) * 29) >> 8;
                    *p = lut[luma as usize];
                }
            }
            ColorMap::Hue(t) => {
                for p in pixels {
                    let c = *p;
                    let (r, g, b) = (((c >> 16) & 255) as usize, ((c >> 8) & 255) as usize, (c & 255) as usize);
                    let nr = (t[r] + t[256 + g] + t[512 + b]) >> 10;
                    let ng = (t[768 + r] + t[1024 + g] + t[1280 + b]) >> 10;
                    let nb = (t[1536 + r] + t[1792 + g] + t[2048 + b]) >> 10;
                    *p = ((nr.clamp(0, 255) as u32) << 16) | ((ng.clamp(0, 255) as u32) << 8) | nb.clamp(0, 255) as u32;
                }
            }
        }
    }
}

fn split(spec: &str) -> (String, &str) {
    match spec.split_once(':') {
        Some((kind, value)) => (kind.trim().to_lowercase(), value.trim()),
        None => (spec.trim().to_lowercase(), ""),
    }
}

/// "#RRGGBB" (or "RRGGBB") as 0xRRGGBB.
pub fn parse_hex(text: &str) -> Option<u32> {
    let hex = text.trim().trim_start_matches('#');
    (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten()
}

/// The transform a spec describes, or None for the visualization's own colours (or a spec that makes no sense).
pub fn parse(spec: &str) -> Option<ColorMap> {
    let (kind, value) = split(spec);
    match kind.as_str() {
        "palette" => PALETTES.iter().find(|(name, _)| name.eq_ignore_ascii_case(value)).map(|(_, stops)| ColorMap::gradient(stops)),
        "hue" => value.parse::<f64>().ok().map(ColorMap::hue_shift),
        "custom" => {
            // Dark parts fade to black, the middle takes the colour, the brightest go towards white.
            let c = parse_hex(value)?;
            let scale = |k: f64| {
                let ch = |s: u32| ((f64::from((c >> s) & 255) * k) as u32) << s;
                ch(16) | ch(8) | ch(0)
            };
            let lighten = |k: f64| {
                let ch = |s: u32| {
                    let v = f64::from((c >> s) & 255);
                    ((v + (255.0 - v) * k) as u32) << s
                };
                ch(16) | ch(8) | ch(0)
            };
            Some(ColorMap::gradient(&[0x000000, scale(0.35), c, lighten(0.6)]))
        }
        _ => None,
    }
}

/// What a spec is called in menus.
pub fn describe(spec: &str) -> String {
    let (kind, value) = split(spec);
    match kind.as_str() {
        "palette" => value.to_string(),
        "hue" => format!("Colors Shifted {value}°"),
        "custom" => format!("Custom Color {value}"),
        _ => "Original Colors".into(),
    }
}

thread_local! {
    static MAPS: RefCell<HashMap<String, Option<Rc<ColorMap>>>> = RefCell::default();
}

/// The transform for a spec, built once per spec.
pub fn map_for(spec: &str) -> Option<Rc<ColorMap>> {
    if spec.trim().is_empty() {
        return None;
    }
    MAPS.with(|m| m.borrow_mut().entry(spec.to_string()).or_insert_with(|| parse(spec).map(Rc::new)).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_palette_maps_brightness_onto_its_gradient() {
        let map = parse("palette:Black and White").unwrap();
        let mut px = [0x000000, 0xFFFFFF, 0x808080, 0xFF0000];
        map.apply(&mut px);
        assert_eq!(px[0], 0x000000);
        assert_eq!(px[1], 0xFFFFFF);
        assert_eq!(px[2] & 0xFF, (px[2] >> 8) & 0xFF, "grey stays grey");
        // Pure red is dark (77/256 of full brightness).
        assert!((px[3] & 0xFF) < 0x60);
    }

    #[test]
    fn a_hue_shift_of_120_degrees_turns_red_greenish_and_keeps_black() {
        let map = parse("hue:120").unwrap();
        let mut px = [0xFF0000, 0x000000];
        map.apply(&mut px);
        let (r, g) = ((px[0] >> 16) & 255, (px[0] >> 8) & 255);
        assert!(g > r, "red shifted by 120° is mostly green: {:06X}", px[0]);
        assert_eq!(px[1], 0);
    }

    #[test]
    fn custom_colours_put_the_colour_in_the_middle() {
        let map = parse("custom:#3366FF").unwrap();
        let ColorMap::Gradient(lut) = &map else { panic!("gradient") };
        assert_eq!(lut[0], 0);
        assert_eq!(lut[170], 0x3366FF);
        assert!(parse("custom:nonsense").is_none());
    }

    #[test]
    fn specs_read_back_as_their_menu_names() {
        assert_eq!(describe(""), "Original Colors");
        assert_eq!(describe("palette:Fire"), "Fire");
        assert_eq!(describe("hue:60"), "Colors Shifted 60°");
        assert_eq!(describe("custom:#FF0000"), "Custom Color #FF0000");
        assert!(map_for("").is_none());
        assert!(map_for("palette:Ice").is_some());
        assert!(map_for("palette:Nope").is_none());
    }
}
