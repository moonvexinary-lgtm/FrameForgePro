//! The Effects workspace: the selected clip's compositing composition — its
//! effect chain (real EffectCraft effects with live parameters) and the layers
//! of its compositing comp.

use std::collections::HashMap;

use filmcraft_project::ItemId;
use filmcraft_project::keyframe::{Param, ParamValue};
use frameforge_bridge::{EffectParamUi, EffectValue, effect_name, effect_params};

use crate::app::App;

/// The param kinds the shell can edit. The rest are shown read-only — never
/// faked. Scalar parameters also carry the keyframe editor (the bridge's
/// keyframes ride the editing engine's keyframe system, in media time).
fn edit_scalar(app: &mut App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, key_time: filmcraft_time::Tick, ui: &mut egui::Ui) {
    // The value shown is the parameter evaluated at the playhead (media
    // time) when animated, else its static value — the same evaluation the
    // renderer performs.
    let current = |app: &App| -> Option<f64> {
        let p = app.compositions.get(&item)?.effects.get(effect)?.params.get(&param.id)?;
        p.value_at(key_time).as_f64()
    };
    let mut value = current(app).unwrap_or_else(|| param.default.as_f64());

    let changed = match &param.ui {
        EffectParamUi::Slider { min, max, decimals, .. } => {
            let range = if max > min { *min..=*max } else { 0.0..=100.0 };
            let mut slider = egui::Slider::new(&mut value, range).text(param.name.clone());
            if *decimals <= 1 {
                slider = slider.integer();
            }
            ui.add(slider).changed()
        }
        EffectParamUi::Number | EffectParamUi::Percent | EffectParamUi::Angle | EffectParamUi::Pixels => {
            ui.add(egui::DragValue::new(&mut value).speed(0.1).prefix(format!("{} ", param.name))).changed()
        }
        _ => {
            ui.label(format!("{} — not editable in the shell yet", param.name));
            false
        }
    };
    if changed && let Some(composition) = app.compositions.get_mut(&item) {
        // Keyframe-preserving: an animated parameter gains a key at the
        // playhead (the engine's own setParam semantics) instead of losing
        // its track; see `Composition::set_scalar`.
        composition.set_scalar(effect, &param.id, key_time, value);
        app.mark_dirty();
    }

    // Keyframing: the stopwatch adds a key at the playhead (media time); the
    // renderer evaluates the track at every request — M2 proved this exact
    // path (direction-independent, byte-identical after save/reopen).
    let animated = app.compositions.get(&item).and_then(|c| c.effects.get(effect)).and_then(|l| l.params.get(&param.id)).is_some_and(|p| p.is_animated());
    ui.horizontal(|ui| {
        let mut touched = false;
        if ui.small_button("◆+").on_hover_text(format!("Add keyframe at the playhead (media time {:.2}s)", key_time.seconds())).clicked()
            && let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect))
        {
            let entry = link.params.entry(param.id.clone()).or_insert_with(|| Param::new(ParamValue::Float(value)));
            entry.put_keyframe(key_time, ParamValue::Float(value));
            touched = true;
        }
        if animated {
            let keys: Vec<(filmcraft_time::Tick, f64)> = app
                .compositions
                .get(&item)
                .and_then(|c| c.effects.get(effect))
                .and_then(|l| l.params.get(&param.id))
                .map(|p| p.keyframes.iter().map(|k| (k.time, k.value.as_f64().unwrap_or_default())).collect())
                .unwrap_or_default();
            ui.label(format!("{} key{}: ", keys.len(), if keys.len() == 1 { "" } else { "s" }));
            for (t, v) in keys {
                if ui.small_button(format!("×{:.1}@{:.2}s", v, t.seconds())).on_hover_text("Delete this keyframe").clicked()
                    && let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect))
                    && let Some(p) = link.params.get_mut(&param.id)
                {
                    p.remove_keyframe_at(t);
                    touched = true;
                }
            }
            if ui.small_button("clear").on_hover_text("Remove all keys of this parameter").clicked()
                && let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect))
                && let Some(p) = link.params.get_mut(&param.id)
            {
                let hold = p.value_at(key_time);
                p.value = hold;
                p.keyframes.clear();
                touched = true;
            }
        }
        if touched {
            app.mark_dirty();
        }
    });
}

fn edit_bool(app: &mut App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let current = app
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| p.value.as_bool())
        .or(match &param.default {
            EffectValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    let mut value = current;
    let changed = ui.checkbox(&mut value, &param.name).changed();
    if changed && let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect)) {
        link.params.insert(param.id.clone(), Param::new(ParamValue::Bool(value)));
    }
    if changed {
        app.mark_dirty();
    }
}

fn edit_choice(app: &mut App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let EffectParamUi::Popup { options } = &param.ui else { return };
    let current = app
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| match &p.value {
            ParamValue::Choice(c) => Some(*c),
            _ => None,
        })
        .or(match &param.default {
            EffectValue::Enum(i) => Some(*i),
            _ => None,
        })
        .unwrap_or(0);
    let selected = options.get(current as usize).cloned().unwrap_or_else(|| "?".to_string());
    let mut new_index = current;
    egui::ComboBox::from_id_salt(&param.id).selected_text(selected).show_ui(ui, |ui| {
        for (i, option) in options.iter().enumerate() {
            ui.selectable_value(&mut new_index, i as u32, option.as_str());
        }
    });
    if new_index != current
        && let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect))
    {
        link.params.insert(param.id.clone(), Param::new(ParamValue::Choice(new_index)));
    }
    if new_index != current {
        app.mark_dirty();
    }
}

fn edit_color(app: &mut App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let current = app
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| p.value.as_color())
        .or(match &param.default {
            EffectValue::Color(c) => Some([c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]),
            _ => None,
        })
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let mut rgb = [current[0], current[1], current[2]];
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(&param.name);
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            changed = true;
        }
    });
    if changed {
        let mut c = current;
        c[0] = rgb[0];
        c[1] = rgb[1];
        c[2] = rgb[2];
        if let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(effect)) {
            link.params.insert(param.id.clone(), Param::new(ParamValue::Color(c)));
        }
        app.mark_dirty();
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    // What the workspace edits: the selected clip's media item, or one picked
    // by hand.
    let effective_item = app.selected_clip().map(|(_, item)| item).or(app.effects_item);
    let Some(item) = effective_item else {
        app.history.end_gesture(false);
        ui.painter().rect_filled(body, 0.0, crate::ui::BG_APP);
        let mut area = crate::ui::panel(ui, body, "fx-pick");
        crate::ui::panel_header(&mut area, "Effects");
        crate::ui::inset(&mut area);
        area.label(crate::ui::hint("Select a clip in the Edit timeline, or choose a media item:"));
        crate::ui::section(&mut area, "MEDIA");
        let mut ids = Vec::new();
        app.session.project.root.all_items(&mut ids);
        egui::ScrollArea::vertical().show(&mut area, |ui| {
            for id in ids {
                let Some(it) = app.session.project.items.get(&id) else { continue };
                if !matches!(&it.kind, filmcraft_project::ItemKind::Media(_)) {
                    continue;
                }
                let name = it.name.clone();
                if ui.selectable_label(false, name).clicked() {
                    app.effects_item = Some(id);
                }
            }
        });
        return;
    };

    // One undo step per pointer gesture: snapshot the selected composition
    // before the widgets run, then record the difference. A slider, colour
    // picker or drag-value drag therefore coalesces into one step instead of
    // one step per frame; a new click or keyboard change starts a new step.
    let (pressed, dragging) = ui.ctx().input(|i| (i.pointer.any_pressed(), i.pointer.any_down()));
    let gesture = app.history.begin_gesture(pressed);
    let before = app.compositions.get(&item).cloned();

    let list_w = 300.0_f32.clamp(220.0, body.width() - 300.0);
    let list = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + list_w, body.max.y));
    let params = egui::Rect::from_min_max(egui::pos2(body.min.x + list_w, body.min.y), body.max);
    effect_list(app, ui, list, item);
    parameters(app, ui, params, item);

    let after = app.compositions.get(&item).cloned();
    if before != after {
        app.record_comp_change(item, before, after, gesture);
    }
    app.history.end_gesture(dragging);
}

fn effect_list(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, item: ItemId) {
    let mut panel = crate::ui::panel(ui, rect, "fx-list");
    panel.painter().line_segment([rect.right_top(), rect.right_bottom()], egui::Stroke::new(1.0, crate::ui::LINE_DARK));
    crate::ui::panel_header(&mut panel, "Effects · composition");
    crate::ui::inset(&mut panel);
    let media_name = app.session.project.items.get(&item).map(|it| it.name.clone()).unwrap_or_default();
    panel.label(crate::ui::hint(format!("Clip: {media_name}")));
    crate::ui::section(&mut panel, "EFFECT CHAIN");

    // The effect chain.
    let count = app.compositions.get(&item).map(|c| c.effects.len()).unwrap_or(0);
    let mut to_remove = None;
    for index in 0..count {
        let (name, enabled) = {
            let Some(link) = app.compositions.get(&item).and_then(|c| c.effects.get(index)) else { continue };
            (effect_name(&link.effect).map(str::to_string).unwrap_or_else(|| link.effect.clone()), link.enabled)
        };
        let selected = app.selected_effect == Some(index);
        panel.horizontal(|ui| {
            let mut toggle = enabled;
            if ui.checkbox(&mut toggle, "").changed() {
                if let Some(link) = app.compositions.get_mut(&item).and_then(|c| c.effects.get_mut(index)) {
                    link.enabled = toggle;
                }
                app.mark_dirty();
            }
            if ui.selectable_label(selected, &name).clicked() {
                app.selected_effect = Some(index);
            }
            if ui.small_button("✕").clicked() {
                to_remove = Some(index);
            }
        });
    }
    if let Some(index) = to_remove {
        let composition = app.compositions.entry(item).or_default();
        if index < composition.effects.len() {
            composition.effects.remove(index);
        }
        if app.selected_effect == Some(index) {
            app.selected_effect = None;
        } else if let Some(selected) = app.selected_effect
            && selected > index
        {
            app.selected_effect = Some(selected - 1);
        }
        app.mark_dirty();
        app.status = "Effect removed".to_string();
    }

    crate::ui::section(&mut panel, "ADD EFFECT");
    panel.add(egui::TextEdit::singleline(&mut app.effect_search).hint_text("Search the effect catalog…"));
    let needle = app.effect_search.to_lowercase();
    egui::ScrollArea::vertical().show(&mut panel, |ui| {
        for entry in frameforge_bridge::catalog() {
            if entry.engine != "effectcraft" {
                continue;
            }
            if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) && !entry.id.to_lowercase().contains(&needle) {
                continue;
            }
            let label = format!("{} · {}", entry.category, entry.name);
            if ui.small_button(&label).on_hover_text(format!("id {}", entry.id)).clicked() {
                let composition = app.compositions.entry(item).or_default();
                let index = composition.effects.len();
                composition.effects.push(frameforge_bridge::EffectLink { effect: entry.id.clone(), enabled: true, params: HashMap::new() });
                app.selected_effect = Some(index);
                app.mark_dirty();
                app.status = format!("Added {}", entry.name);
            }
        }
    });
    crate::ui::spaced_rule(&mut panel);
    panel.label(crate::ui::hint(
        "Parameters apply at once; ◆+ keys the parameter at the playhead (media time) — keys evaluate through the same keyframe system the timeline uses.",
    ));

    // The compositing comp, if the clip has one: real layer editing —
    // visibility toggles, removal, and new text/solid layers (all through
    // the bridge's Composition methods, which own the EffectCraft project).
    let has_comp = app.compositions.get(&item).map(|c| c.has_comp()).unwrap_or(false);
    if has_comp {
        crate::ui::section(&mut panel, "COMPOSITING COMP LAYERS");
        let layers: Vec<(String, bool)> = app.compositions.get(&item).map(|c| c.comp_layers()).unwrap_or_default();
        let mut toggles: Vec<usize> = Vec::new();
        let mut removes: Vec<usize> = Vec::new();
        egui::ScrollArea::vertical().show(&mut panel, |ui| {
            for (index, (name, video)) in layers.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut toggle = *video;
                    if ui.checkbox(&mut toggle, "").changed() {
                        toggles.push(index);
                    }
                    ui.label(name);
                    if ui.small_button("✕").clicked() {
                        removes.push(index);
                    }
                });
            }
        });
        for index in toggles {
            if let Some(composition) = app.compositions.get_mut(&item)
                && composition.comp_toggle_layer(index)
            {
                app.mark_dirty();
            }
        }
        for index in removes.iter().rev() {
            if let Some(composition) = app.compositions.get_mut(&item)
                && composition.comp_remove_layer(*index)
            {
                app.mark_dirty();
                app.status = "Layer removed".to_string();
            }
        }
        panel.horizontal(|ui| {
            if ui.button("+ Text layer").clicked()
                && let Some(composition) = app.compositions.get_mut(&item)
                && composition.comp_add_text_layer("FrameForge")
            {
                app.mark_dirty();
                app.status = "Text layer added".to_string();
            }
            if ui.button("+ Solid layer").clicked()
                && let Some(composition) = app.compositions.get_mut(&item)
                && composition.comp_add_solid_layer([1.0, 0.0, 0.0])
            {
                app.mark_dirty();
                app.status = "Solid layer added".to_string();
            }
        });
        panel.label(crate::ui::hint("New layers land on top (index 0). The M4 acceptance test covers this exact render path."));
    }
}

fn parameters(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, item: ItemId) {
    let mut area = crate::ui::panel(ui, rect, "fx-params");
    area.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, crate::ui::LINE_DARK));
    crate::ui::panel_header(&mut area, "Effect Controls");
    crate::ui::inset(&mut area);
    let Some(effect) = app.selected_effect else {
        area.label(crate::ui::hint("Select an effect on the left to edit its parameters."));
        return;
    };
    let Some(effect_id) = app.compositions.get(&item).and_then(|c| c.effects.get(effect)).map(|l| l.effect.clone()) else {
        area.label(crate::ui::hint("The effect is gone."));
        return;
    };
    let Some(name) = effect_name(&effect_id) else {
        area.label(crate::ui::hint(format!("Unknown effect {effect_id}")));
        return;
    };
    area.label(egui::RichText::new(name).size(13.0).strong().color(crate::ui::ACCENT_HI));
    // Keyframes live in media time: the playhead's offset into the selected
    // clip (keys follow the clip when it moves — the renderer evaluates in
    // media time, so trims and splits never move keyframes).
    let key_time = app
        .selected_clip()
        .and_then(|(clip, _)| app.sequence().and_then(|s| s.find_item(clip).map(|(_, i)| i.start)))
        .map(|start| (app.playhead - start).max(filmcraft_time::Tick::ZERO))
        .unwrap_or(app.playhead);
    area.label(crate::ui::hint(&format!("Keyframes land at media time {:.2}s (the playhead's offset into the clip)", key_time.seconds())));
    let Some(params) = effect_params(&effect_id) else { return };
    if params.is_empty() {
        area.label(crate::ui::hint("This effect has no parameters."));
        return;
    }
    egui::ScrollArea::vertical().show(&mut area, |ui| {
        egui::Grid::new("params").num_columns(1).spacing([8.0, 6.0]).show(ui, |ui| {
            for param in &params {
                ui.vertical(|ui| match &param.ui {
                    EffectParamUi::Checkbox => edit_bool(app, item, effect, param, ui),
                    EffectParamUi::Popup { .. } => edit_choice(app, item, effect, param, ui),
                    EffectParamUi::Color => edit_color(app, item, effect, param, ui),
                    EffectParamUi::Number | EffectParamUi::Slider { .. } | EffectParamUi::Percent | EffectParamUi::Angle | EffectParamUi::Pixels => {
                        edit_scalar(app, item, effect, param, key_time, ui)
                    }
                    _ => {
                        ui.label(format!("{} — not editable in the shell yet", param.name));
                    }
                });
            }
        });
    });
}
