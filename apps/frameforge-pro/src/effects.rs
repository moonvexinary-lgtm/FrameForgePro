//! The Effects workspace: the selected clip's compositing composition — its
//! effect chain (real EffectCraft effects with live parameters) and the layers
//! of its compositing comp.

use std::collections::HashMap;

use filmcraft_project::ItemId;
use filmcraft_project::keyframe::{Param, ParamValue};
use frameforge_bridge::{EffectParamUi, EffectValue, effect_name, effect_params};

use crate::app::App;

/// The param kinds the shell can edit. The rest are shown read-only — never
/// faked.
fn edit_scalar(app: &App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let current = |app: &App| -> Option<f64> { app.doc.doc.compositions.get(&item)?.effects.get(effect)?.params.get(&param.id)?.value.as_f64() };
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
            ui.add(egui::DragValue::new(&mut value).speed(0.1).prefix(&format!("{} ", param.name))).changed()
        }
        _ => {
            ui.label(format!("{} — not editable in the shell yet", param.name));
            false
        }
    };
    if changed && let Some(link) = app.doc.doc.composition_mut(item).effects.get_mut(effect) {
        link.params.insert(param.id.clone(), Param::new(ParamValue::Float(value)));
    }
}

fn edit_bool(app: &App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let current = app
        .doc
        .doc
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| p.value.as_bool())
        .or_else(|| match &param.default {
            EffectValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    let mut value = current;
    if ui.checkbox(&mut value, &param.name).changed()
        && let Some(link) = app.doc.doc.composition_mut(item).effects.get_mut(effect)
    {
        link.params.insert(param.id.clone(), Param::new(ParamValue::Bool(value)));
    }
}

fn edit_choice(app: &App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let EffectParamUi::Popup { options } = &param.ui else { return };
    let current = app
        .doc
        .doc
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| match &p.value {
            ParamValue::Choice(c) => Some(*c),
            _ => None,
        })
        .or_else(|| match &param.default {
            EffectValue::Enum(i) => Some(*i),
            _ => None,
        })
        .unwrap_or(0);
    let selected = options.get(current as usize).cloned().unwrap_or_else(|| "?".to_string());
    let mut new_index = current;
    egui::ComboBox::from_id_salt(&param.id).selected_text(selected).show_ui(ui, 8, |ui| {
        for (i, option) in options.iter().enumerate() {
            ui.selectable_value(&mut new_index, i as u32, option.as_str());
        }
    });
    if new_index != current
        && let Some(link) = app.doc.doc.composition_mut(item).effects.get_mut(effect)
    {
        link.params.insert(param.id.clone(), Param::new(ParamValue::Choice(new_index)));
    }
}

fn edit_color(app: &App, item: ItemId, effect: usize, param: &frameforge_bridge::ParamInfo, ui: &mut egui::Ui) {
    let current = app
        .doc
        .doc
        .compositions
        .get(&item)
        .and_then(|c| c.effects.get(effect))
        .and_then(|l| l.params.get(&param.id))
        .and_then(|p| p.value.as_color())
        .or_else(|| match &param.default {
            EffectValue::Color(c) => Some([c[0], c[1], c[2], c[3]]),
            _ => None,
        })
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let mut rgb = [current[0], current[1], current[2]];
    ui.horizontal(|ui| {
        ui.label(&param.name);
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            let mut c = current;
            c[0] = rgb[0];
            c[1] = rgb[1];
            c[2] = rgb[2];
            if let Some(link) = app.doc.doc.composition_mut(item).effects.get_mut(effect) {
                link.params.insert(param.id.clone(), Param::new(ParamValue::Color(c)));
            }
        }
    });
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    // What the workspace edits: the selected clip's media item, or one picked
    // by hand.
    let effective_item = app.selected_clip().map(|(_, item)| item).or(app.effects_item);
    let Some(item) = effective_item else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("fx-pick"));
        area.add_space(12.0);
        area.heading("Effects");
        area.label("Select a clip in the Edit timeline, or choose a media item:");
        area.separator();
        let mut ids = Vec::new();
        app.doc.doc.project.root.all_items(&mut ids);
        egui::ScrollArea::vertical().show(&mut area, |ui| {
            for id in ids {
                let Some(it) = app.doc.doc.project.items.get(&id) else { continue };
                if !matches!(&it.kind, filmcraft_project::ItemKind::Media(_)) {
                    continue;
                }
                let name = it.name.clone();
                if ui.button(name).clicked() {
                    app.effects_item = Some(id);
                }
            }
        });
        return;
    };

    let list_w = 300.0_f32.clamp(220.0, body.width() - 300.0);
    let list = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + list_w, body.max.y));
    let params = egui::Rect::from_min_max(egui::pos2(body.min.x + list_w, body.min.y), body.max);
    effect_list(app, ui, list, item);
    parameters(app, ui, params, item);
}

fn effect_list(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, item: ItemId) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.right_top(), rect.right_bottom()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("fx-list"));
    panel.add_space(8.0);
    panel.heading("Composition");
    let media_name = app.doc.doc.project.items.get(&item).map(|it| it.name.clone()).unwrap_or_default();
    panel.label(format!("Clip: {media_name}"));
    panel.separator();

    // The effect chain.
    let count = app.doc.doc.compositions.get(&item).map(|c| c.effects.len()).unwrap_or(0);
    let mut to_remove = None;
    for index in 0..count {
        let (name, enabled) = {
            let Some(link) = app.doc.doc.compositions.get(&item).and_then(|c| c.effects.get(index)) else { continue };
            (effect_name(&link.effect).map(str::to_string).unwrap_or_else(|| link.effect.clone()), link.enabled)
        };
        let selected = app.selected_effect == Some(index);
        panel.horizontal(|ui| {
            let mut toggle = enabled;
            if ui.checkbox(&mut toggle, "").changed() {
                if let Some(link) = app.doc.doc.composition_mut(item).effects.get_mut(index) {
                    link.enabled = toggle;
                }
                app.doc.mark_dirty();
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
        if let Some(composition) = app.doc.doc.composition_mut(item) {
            if index < composition.effects.len() {
                composition.effects.remove(index);
            }
        }
        if app.selected_effect == Some(index) {
            app.selected_effect = None;
        }
        app.doc.mark_dirty();
        app.status = "Effect removed".to_string();
    }

    panel.separator();
    panel.label(egui::RichText::new("Add effect").strong());
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
                let composition = app.doc.doc.composition_mut(item);
                let index = composition.effects.len();
                composition.effects.push(frameforge_bridge::EffectLink {
                    effect: entry.id.clone(),
                    enabled: true,
                    params: HashMap::new(),
                    ec_project: None,
                    comp: None,
                    source_item: None,
                });
                app.selected_effect = Some(index);
                app.doc.mark_dirty();
                app.status = format!("Added {}", entry.name);
            }
        }
    });
    panel.separator();
    panel.colored_label(egui::Color32::GRAY, "Parameters apply at once; keyframing arrives with the engine session (M6).");

    // The compositing comp, if the clip has one.
    let has_comp = app.doc.doc.compositions.get(&item).map(|c| c.has_comp()).unwrap_or(false);
    if has_comp {
        panel.separator();
        panel.label(egui::RichText::new("Compositing comp layers").strong());
        let mut names: Vec<String> = Vec::new();
        if let Some(composition) = app.doc.doc.compositions.get(&item)
            && let Some(project) = &composition.ec_project
            && let Some(comp_id) = composition.comp
            && let Some(comp) = project.comp(comp_id)
        {
            for layer in &comp.layers {
                names.push(layer.name.clone());
            }
        }
        egui::ScrollArea::vertical().show(&mut panel, |ui| {
            for name in names {
                ui.label(format!("  {name}"));
            }
        });
        panel.colored_label(egui::Color32::GRAY, "Layer editing arrives with the compositing workspace (M6).");
    }
}

fn parameters(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, item: ItemId) {
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("fx-params"));
    area.add_space(12.0);
    let Some(effect) = app.selected_effect else {
        area.label("Select an effect on the left to edit its parameters.");
        return;
    };
    let Some(effect_id) = app.doc.doc.compositions.get(&item).and_then(|c| c.effects.get(effect)).map(|l| l.effect.clone()) else {
        area.label("The effect is gone");
        return;
    };
    let Some(name) = effect_name(&effect_id) else {
        area.label(format!("Unknown effect {effect_id}"));
        return;
    };
    area.heading(name);
    let Some(params) = effect_params(&effect_id) else { return };
    if params.is_empty() {
        area.label("This effect has no parameters.");
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
                        edit_scalar(app, item, effect, param, ui)
                    }
                    _ => {
                        ui.label(format!("{} — not editable in the shell yet", param.name));
                    }
                });
            }
        });
    });
}
