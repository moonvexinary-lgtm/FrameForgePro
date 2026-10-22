//! The Color workspace: the program monitor beside the selected clip's
//! FilmCraft effect stack — apply, toggle, remove and set parameters of the
//! editing engine's own video effects (Lumetri Color, ASC CDL, Brightness &
//! Contrast…) through engine commands, all undoable. The compositing engine's
//! effects live in the Effects workspace; this is the classic color-grade
//! panel.

use filmcraft_project::keyframe::ParamValue;

use crate::app::{App, Workspace};
use crate::edit;

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let Some((clip, _)) = app.selected_clip() else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("color"));
        area.add_space(16.0);
        area.heading("Color");
        area.label("Select a clip in the Edit timeline to grade it.");
        area.colored_label(
            egui::Color32::GRAY,
            "FilmCraft's own video effects (Lumetri Color, ASC CDL, curves…) apply to the clip here; EffectCraft's catalog is in the Effects workspace.",
        );
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
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("color-stack"));
    panel.add_space(10.0);
    panel.heading("Color · effect stack");
    panel.colored_label(egui::Color32::GRAY, "The editing engine's video effects on this clip — every change is an undoable engine command.");
    panel.separator();

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

    // Add effect: the engine's video-effect catalog.
    panel.separator();
    panel.label(egui::RichText::new("Add effect").strong());
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

    panel.separator();
    parameters(app, &mut panel, clip);
}

/// The selected effect's parameter rows, written through `effects.setParam`.
fn parameters(app: &mut App, panel: &mut egui::Ui, clip: filmcraft_project::ClipId) {
    let Some(effect_index) = app.color_effect else {
        panel.colored_label(egui::Color32::GRAY, "Select an effect above to edit its parameters.");
        return;
    };
    // Read-only pass: the def's parameter list and the instance's values.
    let Some(instance) = app.sequence().and_then(|s| s.find_item(clip).map(|(_, i)| i.effects.get(effect_index).cloned())).flatten() else {
        panel.label("The effect is gone.");
        return;
    };
    let Some(def) = instance.def() else {
        panel.label(format!("Unknown effect {}", instance.effect));
        return;
    };
    panel.label(egui::RichText::new(def.name).strong());

    let mut writes: Vec<(String, serde_json::Value)> = Vec::new();
    egui::ScrollArea::vertical().show(panel, |ui| {
        for p in &def.params {
            let current = instance.params.get(p.id).map(|v| v.value.clone()).unwrap_or_else(|| p.default.clone());
            let animated = instance.params.get(p.id).is_some_and(|v| v.is_animated());
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
                        writes.push((p.id.to_string(), serde_json::json!(value)));
                    }
                }
                filmcraft_project::ParamKind::Bool => {
                    let mut value = current.as_bool().unwrap_or_default();
                    if ui.checkbox(&mut value, p.label).changed() {
                        writes.push((p.id.to_string(), serde_json::json!(value)));
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
                        writes.push((p.id.to_string(), serde_json::json!(new_index)));
                    }
                }
                _ => {
                    ui.label(format!("{} — not editable in this workspace yet", p.label));
                }
            }
        }
    });
    for (param, value) in writes {
        let params = serde_json::json!({"clip": clip.0, "effect": effect_index, "param": param, "value": value});
        if let Err(e) = app.execute("effects.setParam", params) {
            app.status = e;
        }
    }
}
