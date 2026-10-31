//! EFFECTS elements and mediacenter.effectType: which visualization each visualizer shows, and
//! drawing their next frames.

use super::element::Kind;
use super::{Skin, ViewId};
use crate::value::{ObjRef, Value};
use crate::vis::{self, ALL, Renderer, VisFrame};

impl Skin {
    /// The player-wide visualization (WMP's mediacenter.effectType / effectPreset).
    pub fn global_vis(&self) -> (usize, usize) {
        let effect = vis::find(&self.settings.vis_effect).unwrap_or(0);
        (effect, self.settings.vis_preset.min(ALL[effect].presets.len() - 1))
    }

    fn set_global_vis(&mut self, effect: usize, preset: usize) {
        if self.global_vis() == (effect, preset) {
            return;
        }
        self.settings.vis_effect = ALL[effect].id.to_string();
        self.settings.vis_preset = preset;
        self.settings.save();
        for name in ["effecttype", "effectpreset", "effecttitle", "effectpresettitle"] {
            self.notify(ObjRef::MediaCenter, name);
        }
    }

    pub(super) fn mediacenter_vis(&self, name: &str) -> Value {
        let (effect, preset) = self.global_vis();
        match name {
            "effecttype" => ALL[effect].key().into(),
            "effectpreset" => (preset as f64).into(),
            "effecttitle" => ALL[effect].title.into(),
            _ => ALL[effect].presets[preset].into(),
        }
    }

    pub(super) fn set_mediacenter_vis(&mut self, name: &str, value: &Value) {
        let (effect, _) = self.global_vis();
        match name {
            "effecttype" => {
                if let Some(e) = vis::find(&value.to_str()).filter(|&e| e != effect) {
                    self.set_global_vis(e, 0);
                }
            }
            "effectpreset" => self.set_global_vis(effect, (value.to_number().max(0.0) as usize).min(ALL[effect].presets.len() - 1)),
            // The titles are read-only.
            _ => {}
        }
    }

    /// Every element that shows a visualization: EFFECTS, and VIDEO in skins without one.
    fn visualizers(&self) -> Vec<(ViewId, usize)> {
        self.open_views()
            .flat_map(|v| {
                let view = self.view(v);
                (0..view.elements.len()).map(move |e| (v, e))
            })
            .filter(|&(v, e)| match self.el(v, e).kind {
                Kind::Effects => true,
                Kind::Video => !self.has_effects,
                _ => false,
            })
            .collect()
    }

    /// What an element shows: its renderer's choice, or what its properties ask for.
    pub(super) fn shown_vis(&self, v: ViewId, e: usize) -> (usize, usize) {
        if let Some(r) = &self.el(v, e).state.vis {
            return (r.effect, r.preset);
        }
        let named = ["currenteffecttype", "effecttype"].iter().find_map(|n| vis::find(&self.get_str(v, e, n)));
        let (global, global_preset) = self.global_vis();
        let effect = named.unwrap_or(global);
        let preset = ["currentpreset", "preset"]
            .iter()
            .find(|n| self.el(v, e).values.contains_key(**n))
            .map(|n| self.get_int(v, e, n).max(0) as usize)
            .unwrap_or(if effect == global { global_preset } else { 0 });
        (effect, preset.min(ALL[effect].presets.len() - 1))
    }

    /// Shows `effect`/`preset` in a visualizer and tells the skin (currentEffectType and friends).
    fn apply_vis(&mut self, v: ViewId, e: usize, effect: usize, preset: usize, make_global: bool) {
        let state = &mut self.el_mut(v, e).state;
        let r = state.vis.get_or_insert_with(|| Renderer::new(effect, preset));
        r.set(effect, preset);
        let (effect, preset) = (r.effect, r.preset);
        state.applying = true;
        // A name the skin set itself ("Spikes") stays while it maps to the effect shown.
        if vis::find(&self.get_str(v, e, "currenteffecttype")) != Some(effect) {
            self.set_value(v, e, "currenteffecttype", ALL[effect].key().into(), true);
        }
        self.set_value(v, e, "currentpreset", (preset as f64).into(), true);
        self.el_mut(v, e).state.applying = false;
        for name in ["currenteffecttitle", "currentpresettitle"] {
            self.notify(ObjRef::Element { view: v, element: e }, name);
        }
        if make_global {
            self.set_global_vis(effect, preset);
        }
        self.view_mut(v).invalidate();
    }

    /// A skin changed currentEffectType, effectType, currentPreset or preset.
    pub(super) fn effects_changed(&mut self, v: ViewId, e: usize, name: &str) {
        if self.el(v, e).state.applying {
            return;
        }
        let (effect, preset) = self.shown_vis(v, e);
        match name {
            "currenteffecttype" | "effecttype" => {
                let Some(wanted) = vis::find(&self.get_str(v, e, name)) else { return };
                let (global, global_preset) = self.global_vis();
                let preset = if wanted == effect {
                    preset
                } else if wanted == global {
                    global_preset
                } else {
                    0
                };
                self.apply_vis(v, e, wanted, preset, true);
            }
            "currentpreset" | "preset" => {
                let wanted = self.get_int(v, e, name).max(0) as usize;
                if self.el(v, e).state.vis.as_ref().is_none_or(|r| r.preset != wanted) {
                    self.apply_vis(v, e, effect, wanted, true);
                }
            }
            _ => {}
        }
    }

    pub(super) fn effects_call(&mut self, v: ViewId, e: usize, method: &str, index: Option<usize>) -> Value {
        let (effect, preset) = self.shown_vis(v, e);
        let presets = ALL[effect].presets.len();
        let step_effect = |dir: isize| (effect as isize + dir).rem_euclid(ALL.len() as isize) as usize;
        let target = match method {
            "nextpreset" => Some((effect, (preset + 1) % presets)),
            "previouspreset" => Some((effect, (preset + presets - 1) % presets)),
            "nexteffect" => Some((step_effect(1), 0)),
            "previouseffect" => {
                let to = step_effect(-1);
                Some((to, ALL[to].presets.len() - 1))
            }
            // next()/previous() walk the presets, rolling over into the neighbouring visualization.
            "next" if preset + 1 < presets => Some((effect, preset + 1)),
            "next" => Some((step_effect(1), 0)),
            "previous" if preset > 0 => Some((effect, preset - 1)),
            "previous" => {
                let to = step_effect(-1);
                Some((to, ALL[to].presets.len() - 1))
            }
            _ => None,
        };
        if let Some((to, p)) = target {
            self.apply_vis(v, e, to, p, true);
            return Value::Null;
        }
        let i = index.unwrap_or(0);
        match method {
            "geteffecttitle" => ALL.get(i).map_or("", |d| d.title).into(),
            "geteffecttype" => ALL.get(i).map_or("", |d| d.key()).into(),
            "getpresettitle" => ALL[effect].presets.get(i).copied().unwrap_or("").into(),
            _ => Value::Null,
        }
    }

    pub(super) fn effects_computed(&self, v: ViewId, e: usize, name: &str) -> Option<Value> {
        if !matches!(
            name,
            "currenteffecttitle" | "currentpresettitle" | "effectcount" | "presetcount" | "effectcangofullscreen" | "effecthaspropertypage"
        ) {
            return None;
        }
        let (effect, preset) = self.shown_vis(v, e);
        Some(match name {
            "currenteffecttitle" => ALL[effect].title.into(),
            "currentpresettitle" => ALL[effect].presets[preset].into(),
            "effectcount" => (ALL.len() as f64).into(),
            "presetcount" => (ALL[effect].presets.len() as f64).into(),
            _ => false.into(),
        })
    }

    /// Picked from the player's own menu: every visualizer switches to it, as in WMP.
    pub fn choose_vis(&mut self, effect: usize, preset: usize) {
        self.set_global_vis(effect, preset);
        for (v, e) in self.visualizers() {
            self.apply_vis(v, e, effect, preset, false);
        }
    }

    /// Whether any visualizer can be seen, so the host knows to listen to the audio.
    pub fn wants_visualization(&self) -> bool {
        self.visualizers().into_iter().any(|(v, e)| self.effectively_visible(v, e))
    }

    /// Draws the next frame of every visible visualizer at `scale` device pixels per skin pixel.
    pub fn render_visualizers(&mut self, frame: &VisFrame, scale: f64) {
        for (v, e) in self.visualizers() {
            if !self.effectively_visible(v, e) {
                continue;
            }
            let b = self.bounds(v, e);
            if b.w < 1.0 || b.h < 1.0 {
                continue;
            }
            if self.el(v, e).state.vis.is_none() {
                let (effect, preset) = self.shown_vis(v, e);
                self.apply_vis(v, e, effect, preset, false);
            }
            let Some(mut r) = self.el_mut(v, e).state.vis.take() else { continue };
            r.render(frame, &self.vis_context, (b.w * scale).round() as usize, (b.h * scale).round() as usize);
            self.el_mut(v, e).state.vis = Some(r);
            self.view_mut(v).invalidate_region(b);
        }
    }

    /// The track changed: the Album Art visualization follows it.
    pub(super) fn refresh_vis_context(&mut self) {
        let m = self.player.source.snapshot();
        let ctx = &mut self.vis_context;
        ctx.title = m.title;
        ctx.artist = m.artist;
        ctx.artwork = self.player.source.artwork();
        ctx.version = ctx.version.wrapping_add(1);
    }
}
