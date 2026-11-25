//! The Color workspace: the program monitor beside the selected clip's
//! FilmCraft effect stack — apply, toggle, remove and set parameters of the
//! editing engine's own video effects (Lumetri Color, ASC CDL, Brightness &
//! Contrast…) through engine commands, all undoable. The compositing engine's
//! effects live in the Effects workspace; this is the classic color-grade
//! panel.

use filmcraft_project::keyframe::ParamValue;

use crate::app::App;
use crate::edit;

/// One hex draft per colour parameter (effect index plus parameter id): the
/// document value it was loaded from and the text being typed. A shared draft
/// would be rewritten by every colour row on every frame - the bug T53's test
/// caught - so each row keeps its own.
#[derive(Default)]
pub struct HexDrafts {
    rows: std::collections::BTreeMap<(usize, String), HexDraft>,
}

#[derive(Default)]
pub struct HexDraft {
    pub loaded: String,
    pub text: String,
}

impl HexDrafts {
    /// The draft for one colour parameter, reloaded when the document value it
    /// was loaded from changes (an undo, a reset, another editor).
    pub fn row(&mut self, effect: usize, param: &str, current: &str) -> &mut HexDraft {
        let draft = self.rows.entry((effect, param.to_string())).or_default();
        if draft.loaded != current {
            draft.loaded = current.to_string();
            draft.text = current.to_string();
        }
        draft
    }

    /// The text of one parameter's draft. The panel reads the draft it holds
    /// directly; this accessor exists so a test can see what a row is showing.
    #[cfg(test)]
    pub fn text(&self, effect: usize, param: &str) -> Option<&str> {
        self.rows.get(&(effect, param.to_string())).map(|draft| draft.text.as_str())
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let Some((clip, _)) = app.selected_clip() else {
        ui.painter().rect_filled(body, 0.0, crate::ui::BG_APP);
        let mut area = crate::ui::panel(ui, body, "color");
        crate::ui::panel_header(&mut area, "Color");
        crate::ui::inset(&mut area);
        area.label(egui::RichText::new("No clip selected").size(14.0).strong().color(crate::ui::TEXT_HI));
        area.add_space(4.0);
        area.label(crate::ui::hint("Select a clip in the Edit timeline to grade it."));
        crate::ui::big_gap(&mut area);
        area.label(crate::ui::hint(
            "FilmCraft's own video effects (Lumetri Color, ASC CDL, curves…) apply to the clip here; EffectCraft's catalog is in the Effects workspace.",
        ));
        return;
    };

    let stack_w = app.color_stack_w.clamp(280.0, body.width() - 320.0);
    let stack = egui::Rect::from_min_max(egui::pos2(body.min.x + stack_w, body.min.y), body.max);
    let monitor = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + stack_w, body.max.y));
    edit::monitor_panel(app, ui, monitor);
    let mut stack_w_value = app.color_stack_w;
    crate::app::gutter_v(ui, body.min.x + stack_w, body.min.y, body.max.y, &mut stack_w_value);
    app.color_stack_w = stack_w_value;
    stack_panel(app, ui, stack, clip);
}

fn stack_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, clip: filmcraft_project::ClipId) {
    let mut panel = crate::ui::panel(ui, rect, "color-stack");
    panel.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, crate::ui::LINE_DARK));
    crate::ui::panel_header(&mut panel, "Color · effect stack");
    crate::ui::inset(&mut panel);
    panel.label(crate::ui::hint("The editing engine's video effects on this clip — every change is an undoable engine command."));
    crate::ui::section(&mut panel, "EFFECTS");

    // Read the clip's effect stack (read-only pass), then apply mutations.
    let stack: Vec<(usize, String, bool)> = app
        .sequence()
        .and_then(|s| {
            s.find_item(clip).map(|(_, item)| {
                item.effects
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.def().is_some_and(|d| d.kind == filmcraft_project::EffectKind::Video) || e.def().is_none())
                    .map(|(i, e)| (i, e.effect.clone(), e.enabled))
                    .collect()
            })
        })
        .unwrap_or_default();

    let mut to_toggle = None;
    let mut to_remove = None;
    let mut to_reset = None;
    if stack.is_empty() {
        panel.label(crate::ui::hint("No video effects on this clip yet — add one below."));
    }
    for (index, name, enabled) in &stack {
        let selected = app.color_effect == Some(*index);
        panel.horizontal(|ui| {
            let mut toggle = *enabled;
            if ui.checkbox(&mut toggle, "").changed() {
                to_toggle = Some(*index);
            }
            if ui.selectable_label(selected, name).clicked() {
                app.color_effect = Some(*index);
            }
            if reset_button(ui, &format!("ff-color-reset-effect-{index}"), "Reset every parameter of this effect").clicked() {
                to_reset = Some(*index);
            }
            if ui.small_button("✕").clicked() {
                to_remove = Some(*index);
            }
        });
    }
    if let Some(index) = to_toggle {
        match app.execute("effects.toggleEnabled", serde_json::json!({"clip": clip.0, "index": index})) {
            Ok(_) => app.status = "Effect toggled".to_string(),
            Err(e) => app.status = e,
        }
    }
    if let Some(index) = to_remove {
        match app.execute("effects.remove", serde_json::json!({"clip": clip.0, "index": index})) {
            Ok(_) => {
                app.color_effect = None;
                app.status = "Effect removed".to_string();
            }
            Err(e) => app.status = e,
        }
    }
    if let Some(index) = to_reset {
        match reset_effect(app, clip, index) {
            Ok(()) => app.status = "Effect reset".to_string(),
            Err(e) => app.status = e,
        }
    }

    // Add effect: the engine's video-effect catalog.
    crate::ui::section(&mut panel, "ADD EFFECT");
    {
        let mut pick = String::new();
        egui::ComboBox::from_id_salt("color-add").selected_text("Color Correction").show_ui(&mut panel, |ui| {
            for entry in frameforge_bridge::catalog() {
                if entry.engine != "filmcraft" || entry.kind != "video" {
                    continue;
                }
                if !entry.category.starts_with("Video Effects") {
                    continue;
                }
                if ui.selectable_label(false, &entry.name).clicked() {
                    pick = entry.name.clone();
                }
            }
        });
        if !pick.is_empty() {
            match app.execute("effects.apply", serde_json::json!({"clips": [clip.0], "effect": pick})) {
                Ok(_) => app.status = "Effect applied".to_string(),
                Err(e) => app.status = e,
            }
        }
    }

    crate::ui::spaced_rule(&mut panel);
    crate::ui::section(&mut panel, "PARAMETERS");
    parameters(app, &mut panel, clip);
}

/// A small, stable-id reset button for the effect rows (the shared glyph
/// button, so the id the test clicks cannot drift with layout).
fn reset_button(ui: &mut egui::Ui, id: &str, tip: &str) -> egui::Response {
    crate::ui::glyph_button(ui, id, "⟲", tip)
}

/// Reset one effect's parameters to their defaults through `effects.reset`.
fn reset_effect(app: &mut App, clip: filmcraft_project::ClipId, index: usize) -> Result<(), String> {
    app.execute("effects.reset", serde_json::json!({"clip": clip.0, "index": index}))?;
    Ok(())
}

/// Reset one parameter of an effect through `effects.resetParam`.
fn reset_parameter(app: &mut App, clip: filmcraft_project::ClipId, effect: usize, param: &str) -> Result<(), String> {
    app.execute("effects.resetParam", serde_json::json!({"clip": clip.0, "effect": effect, "param": param}))?;
    Ok(())
}

/// The selected effect's parameter rows, written through `effects.setParam`.
fn parameters(app: &mut App, panel: &mut egui::Ui, clip: filmcraft_project::ClipId) {
    let Some(effect_index) = app.color_effect else {
        panel.label(crate::ui::hint("Select an effect above to edit its parameters."));
        return;
    };
    // Read-only pass: the def's parameter list and the instance's values.
    let Some(instance) = app.sequence().and_then(|s| s.find_item(clip).map(|(_, i)| i.effects.get(effect_index).cloned())).flatten() else {
        panel.label(crate::ui::hint("The effect is gone."));
        return;
    };
    let Some(def) = instance.def() else {
        panel.label(crate::ui::hint(&format!("Unknown effect {}", instance.effect)));
        return;
    };
    panel.label(egui::RichText::new(def.name).size(12.0).strong().color(crate::ui::ACCENT_HI));

    // Keys live in media time — the playhead's offset into the clip, through
    // the same conversion the engine uses, so the stopwatch and the key chips
    // agree even when the clip is trimmed, retimed or on a nonzero start.
    let key_time = app
        .sequence()
        .and_then(|sequence| sequence.find_item(clip))
        .map(|(_, item)| item.source_time_at(app.playhead.clamp(item.start, (item.end() - filmcraft_time::Tick(1)).max(item.start))))
        .unwrap_or(app.playhead);

    let mut writes: Vec<(String, serde_json::Value, Option<i64>)> = Vec::new();
    let mut resets: Vec<String> = Vec::new();
    let mut key_toggles: Vec<String> = Vec::new();
    let mut deletions: Vec<(String, i64)> = Vec::new();
    let mut invalid: Option<String> = None;
    egui::ScrollArea::vertical().show(panel, |ui| {
        for p in &def.params {
            let current = instance.params.get(p.id).map(|v| v.value.clone()).unwrap_or_else(|| p.default.clone());
            let animated = instance.params.get(p.id).is_some_and(|v| v.is_animated());
            ui.horizontal(|ui| {
                if reset_button(ui, &format!("ff-color-reset-param-{}", p.id), "Reset this parameter to its default").clicked() {
                    resets.push(p.id.to_string());
                }
                match &p.kind {
                    filmcraft_project::ParamKind::Float { min, max, soft_min, soft_max, decimals, .. } => {
                        let mut value = current.as_f64().unwrap_or_default();
                        let lo = if soft_min < soft_max { *soft_min } else { *min };
                        let hi = if soft_min < soft_max { *soft_max } else { *max };
                        let mut slider = egui::Slider::new(&mut value, lo..=hi).text(format!("{}{}", p.label, if animated { " ◆" } else { "" }));
                        if *decimals <= 1 {
                            slider = slider.integer();
                        }
                        if ui.add(slider).changed() {
                            writes.push((p.id.to_string(), serde_json::json!(value), animated.then_some(app.playhead.0)));
                        }
                    }
                    filmcraft_project::ParamKind::Bool => {
                        let value = current.as_bool().unwrap_or_default();
                        if crate::ui::check_button(ui, &format!("ff-color-toggle-{}", p.id), value, p.label, "Toggle this parameter").clicked() {
                            writes.push((p.id.to_string(), serde_json::json!(!value), None));
                        }
                    }
                    filmcraft_project::ParamKind::Choice(options) => {
                        let current_index = match &current {
                            ParamValue::Choice(c) => *c as usize,
                            _ => 0,
                        };
                        let selected_text = options.get(current_index).map(|s| s.to_string()).unwrap_or_else(|| "?".to_string());
                        let mut new_index = current_index;
                        egui::ComboBox::from_id_salt(p.id).selected_text(selected_text).show_ui(ui, |ui| {
                            for (i, option) in options.iter().enumerate() {
                                ui.selectable_value(&mut new_index, i, *option);
                            }
                        });
                        if new_index != current_index {
                            writes.push((p.id.to_string(), serde_json::json!(new_index), None));
                        }
                    }
                    filmcraft_project::ParamKind::Color => {
                        // The engine accepts `#rrggbb`/`#rrggbbaa` strings for
                        // colour parameters, so the row edits that spelling and
                        // writes it back canonically. The draft is per parameter
                        // and reloads when the document value it was loaded from
                        // changes (an undo, a reset, a keyframe).
                        let current_hex = filmcraft_color::to_hex(current.as_color().unwrap_or([0.0, 0.0, 0.0, 1.0]));
                        let draft = app.color_hex.row(effect_index, p.id, &current_hex);
                        ui.add(egui::TextEdit::singleline(&mut draft.text).id(egui::Id::new(format!("ff-color-hex-{}", p.id))).desired_width(80.0));
                        if crate::ui::chip_button(ui, &format!("ff-color-hex-set-{}", p.id), "Set", "Set this colour from #rrggbb or #rrggbbaa").clicked() {
                            match filmcraft_color::parse_hex(&draft.text) {
                                Some(color) => {
                                    let hex = filmcraft_color::to_hex(color);
                                    draft.loaded = hex.clone();
                                    draft.text = hex.clone();
                                    writes.push((p.id.to_string(), serde_json::json!(hex), animated.then_some(app.playhead.0)));
                                }
                                None => invalid = Some("A colour is #rrggbb or #rrggbbaa".to_string()),
                            }
                        }
                    }
                    _ => {
                        ui.label(format!("{} — not editable in this workspace yet", p.label));
                    }
                }
            });
            // Animatable parameters whose value the panel can render carry the
            // keyframe row: a Float reads as a number, a Color as hex. The
            // stopwatch keys the value at the playhead and a chip per key
            // deletes exactly that key (the engine toggles and restores the
            // removed value when the track empties). Bool and Choice are not
            // animatable in the project crate, and the other kinds have no
            // value editor here yet.
            if p.animatable && matches!(&p.kind, filmcraft_project::ParamKind::Float { .. } | filmcraft_project::ParamKind::Color) {
                ui.horizontal(|ui| {
                    if crate::ui::glyph_button(
                        ui,
                        &format!("ff-color-key-{}", p.id),
                        "◆+",
                        &format!("Keyframe at the playhead (media time {:.2}s)", key_time.seconds()),
                    )
                    .clicked()
                    {
                        key_toggles.push(p.id.to_string());
                    }
                    if animated {
                        let keys: Vec<(i64, ParamValue)> = instance
                            .params
                            .get(p.id)
                            .map(|param| param.keyframes.iter().map(|key| (key.time.0, key.value.clone())).collect())
                            .unwrap_or_default();
                        ui.label(format!("{} key{}: ", keys.len(), if keys.len() == 1 { "" } else { "s" }));
                        for (t, value) in keys {
                            // A colour key reads as the engine's hex spelling; a
                            // scalar key keeps its one-decimal number.
                            let text = match value.as_color() {
                                Some(colour) => format!("×{}@{:.2}s", filmcraft_color::to_hex(colour), filmcraft_time::Tick(t).seconds()),
                                None => format!("×{:.1}@{:.2}s", value.as_f64().unwrap_or_default(), filmcraft_time::Tick(t).seconds()),
                            };
                            if crate::ui::chip_button(ui, &format!("ff-color-key-del-{}-{}", p.id, t), &text, "Delete this keyframe").clicked() {
                                deletions.push((p.id.to_string(), t));
                            }
                        }
                    }
                });
            }
        }
    });
    if let Some(message) = invalid {
        app.status = message;
    }
    for (param, value, time) in writes {
        let mut params = serde_json::json!({"clip": clip.0, "effect": effect_index, "param": param, "value": value});
        // An animated parameter's write is a keyframe write: name the
        // playhead the row displays, rather than letting the engine fall back
        // to the session playhead. They agree while the app keeps them in
        // step, and the stopwatch already keys at `app.playhead`, so the row
        // must not write somewhere else.
        if let Some(t) = time {
            params["time"] = serde_json::json!(t);
        }
        if let Err(e) = app.execute("effects.setParam", params) {
            app.status = e;
        }
    }
    for param in resets {
        match reset_parameter(app, clip, effect_index, &param) {
            Ok(()) => app.status = "Parameter reset".to_string(),
            Err(e) => app.status = e,
        }
    }
    for param in key_toggles {
        // The engine's `effects.addKeyframe` toggles at the playhead, so read
        // the track after the edit to name what actually happened.
        let params = serde_json::json!({"clip": clip.0, "effect": effect_index, "param": param, "time": app.playhead.0});
        match app.execute("effects.addKeyframe", params) {
            Ok(_) => {
                let present = app
                    .sequence()
                    .and_then(|sequence| sequence.find_item(clip))
                    .and_then(|(_, item)| item.effects.get(effect_index))
                    .and_then(|effect| effect.params.get(&param))
                    .is_some_and(|param| param.keyframes.binary_search_by_key(&key_time, |key| key.time).is_ok());
                app.status = if present { "Keyframe added" } else { "Keyframe removed" }.to_string();
            }
            Err(e) => app.status = e,
        }
    }
    for (param, tick) in deletions {
        let params = serde_json::json!({"clip": clip.0, "effect": effect_index, "param": param, "mediaTime": tick});
        match app.execute("effects.deleteKeyframe", params) {
            Ok(_) => app.status = "Keyframe removed".to_string(),
            Err(e) => app.status = e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use filmcraft_project::ClipId;
    use serde_json::json;

    /// A 24 fps sequence with one 10 s Bars & Tone pair placed at zero.
    fn color_app() -> (App, ClipId) {
        let mut app = App::new();
        app.execute("file.newSequence", json!({"name": "Color", "fps": 24, "video": 1, "audio": 1})).expect("create the sequence");
        let item = app.execute("file.newBarsAndTone", json!({"seconds": 10.0})).expect("add bars")["item"].as_u64().expect("the item id");
        let placed = app.execute("timeline.place", json!({"item": item, "track": "V1", "audioTrack": "A1", "seconds": 0.0})).expect("place the clip");
        (app, ClipId(placed["clips"][0].as_u64().expect("the video clip id")))
    }

    fn effect_index(app: &App, clip: ClipId) -> usize {
        app.sequence()
            .and_then(|sequence| sequence.find_item(clip))
            .and_then(|(_, item)| item.effects.iter().position(|effect| effect.effect == "motion"))
            .expect("the clip carries the intrinsic motion effect")
    }

    fn value(app: &App, clip: ClipId, effect: &str, param: &str) -> f64 {
        app.sequence()
            .and_then(|sequence| sequence.find_item(clip))
            .and_then(|(_, item)| item.effect(effect))
            .and_then(|effect| effect.param(param))
            .and_then(|param| param.value.as_f64())
            .unwrap_or_else(|| panic!("{effect}/{param} is set"))
    }

    fn set(app: &mut App, clip: ClipId, effect: &str, param: &str, value: f64) {
        app.execute("effects.setParam", json!({"clip": clip.0, "effect": effect, "param": param, "value": value})).expect("set the parameter");
    }

    #[test]
    fn resetting_an_effect_restores_every_parameter_in_one_undo_step() {
        let (mut app, clip) = color_app();
        let index = effect_index(&app, clip);
        set(&mut app, clip, "motion", "scale", 150.0);
        set(&mut app, clip, "motion", "rotation", 30.0);

        reset_effect(&mut app, clip, index).expect("the effect resets");

        assert_eq!(value(&app, clip, "motion", "scale"), 100.0, "scale returns to its default");
        assert_eq!(value(&app, clip, "motion", "rotation"), 0.0, "rotation returns to its default");
        app.session.execute("edit.undo", json!({})).expect("one undo");
        assert_eq!(value(&app, clip, "motion", "scale"), 150.0, "one Ctrl+Z restores the whole effect");
        assert_eq!(value(&app, clip, "motion", "rotation"), 30.0, "both parameters come back together");
    }

    #[test]
    fn resetting_one_parameter_leaves_the_others_alone() {
        let (mut app, clip) = color_app();
        let index = effect_index(&app, clip);
        set(&mut app, clip, "motion", "scale", 150.0);
        set(&mut app, clip, "motion", "rotation", 30.0);

        reset_parameter(&mut app, clip, index, "rotation").expect("the parameter resets");

        assert_eq!(value(&app, clip, "motion", "rotation"), 0.0, "rotation returns to its default");
        assert_eq!(value(&app, clip, "motion", "scale"), 150.0, "the other parameter is untouched");
        app.session.execute("edit.undo", json!({})).expect("one undo");
        assert_eq!(value(&app, clip, "motion", "rotation"), 30.0, "one Ctrl+Z restores just that parameter");
    }

    #[test]
    fn resetting_an_unknown_parameter_reports_the_engine_error() {
        let (mut app, clip) = color_app();
        let index = effect_index(&app, clip);
        set(&mut app, clip, "motion", "rotation", 30.0);

        let error = reset_parameter(&mut app, clip, index, "nope").expect_err("there is no such parameter");

        assert!(error.contains("no param `nope`"), "the engine's own message reaches the panel: {error}");
        assert_eq!(value(&app, clip, "motion", "rotation"), 30.0, "a refused reset changes nothing");
    }
}
