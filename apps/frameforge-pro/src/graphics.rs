//! The Text & Graphics workspace: real graphic clips and their real layer
//! stack, edited through FilmCraft's `graphics.*` commands. The centre is the
//! normal program monitor, so what is edited is exactly what renders.

use filmcraft_project::{ClipId, ItemKind};

use crate::app::App;
use crate::ui;

#[derive(Clone, Copy)]
enum Shape {
    Rectangle,
    Ellipse,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let clip = current_graphic(app);
    app.graphics_clip = clip;

    let left_w = 300.0_f32.clamp(240.0, (body.width() - 560.0).max(240.0));
    let right_w = 380.0_f32.clamp(280.0, (body.width() - left_w - 360.0).max(280.0));
    let left = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + left_w, body.max.y));
    let right = egui::Rect::from_min_max(egui::pos2(body.max.x - right_w, body.min.y), body.max);
    let centre = egui::Rect::from_min_max(egui::pos2(left.max.x, body.min.y), egui::pos2(right.min.x, body.max.y));

    graphic_list(app, ui, left);
    crate::edit::monitor_panel(app, ui, centre);
    graphic_inspector(app, ui, right, clip);
}

fn is_graphic(app: &App, clip: ClipId) -> bool {
    app.sequence().is_some_and(|seq| {
        seq.find_item(clip).and_then(|(_, item)| app.session.project.items.get(&item.item)).is_some_and(|item| matches!(&item.kind, ItemKind::Graphic { .. }))
    })
}

/// The graphic selected in the workspace, the timeline selection if it is a
/// graphic, or the first graphic in the sequence.
fn current_graphic(app: &App) -> Option<ClipId> {
    if let Some(clip) = app.graphics_clip.filter(|clip| is_graphic(app, *clip)) {
        return Some(clip);
    }
    if let Some(clip) = app.selected_clip().map(|(clip, _)| clip).filter(|clip| is_graphic(app, *clip)) {
        return Some(clip);
    }
    let seq = app.sequence()?;
    seq.video_tracks
        .iter()
        .flat_map(|track| track.items.iter())
        .find_map(|item| app.session.project.items.get(&item.item).filter(|i| matches!(&i.kind, ItemKind::Graphic { .. })).map(|_| item.id))
}

fn graphic_clips(app: &App) -> Vec<(ClipId, String, String)> {
    let Some(seq) = app.sequence() else { return Vec::new() };
    let mut out = Vec::new();
    for (index, track) in seq.video_tracks.iter().enumerate() {
        for item in &track.items {
            let Some(media) = app.session.project.items.get(&item.item) else { continue };
            if matches!(&media.kind, ItemKind::Graphic { .. }) {
                out.push((item.id, item.name.clone(), format!("V{}", index + 1)));
            }
        }
    }
    out
}

fn graphic_list(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.right_top(), rect.right_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("graphic-list"));
    ui::panel_header(&mut panel, "Text & Graphics");
    panel.add_space(6.0);
    panel.horizontal(|ui| {
        if ui.button("+ New Text").clicked() {
            new_text(app, false);
        }
        if ui.button("+ Layer").clicked() {
            new_text(app, true);
        }
    });
    panel.horizontal(|ui| {
        if ui.button("+ Rectangle").clicked() {
            new_shape(app, Shape::Rectangle);
        }
        if ui.button("+ Ellipse").clicked() {
            new_shape(app, Shape::Ellipse);
        }
    });
    panel.horizontal(|ui| {
        ui.label(egui::RichText::new("New text size").size(10.0).color(ui::TEXT_LO));
        ui.add(egui::DragValue::new(&mut app.text_size).range(6.0..=400.0).suffix(" px"));
    });
    ui::spaced_rule(&mut panel);
    ui::section(&mut panel, "GRAPHIC CLIPS");

    let rows = graphic_clips(app);
    if rows.is_empty() {
        panel.add_space(4.0);
        panel.label(ui::hint("No graphic clips yet. New Text creates one at the playhead."));
    }
    let mut chosen = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut panel, |ui| {
        for (clip, name, track) in rows {
            let selected = app.graphics_clip == Some(clip);
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 3.0, if selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
            } else {
                ui::card(ui, row, selected);
            }
            if selected {
                ui::selected_bar(ui.painter(), row);
            }
            let icon = egui::Rect::from_min_size(row.min + egui::vec2(7.0, 6.0), egui::vec2(22.0, 22.0));
            ui.painter().rect_filled(icon, 2.0, ui::CLIP_TITLE);
            ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, "T", egui::FontId::proportional(10.0), ui::TEXT_HI);
            ui.painter().text(
                egui::pos2(icon.right() + 8.0, row.top() + 11.0),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(11.0),
                if selected { ui::TEXT_HI } else { ui::TEXT_MID },
            );
            ui.painter().text(
                egui::pos2(icon.right() + 8.0, row.bottom() - 10.0),
                egui::Align2::LEFT_CENTER,
                track,
                egui::FontId::proportional(9.0),
                ui::TEXT_OFF,
            );
            if response.clicked() {
                chosen = Some(clip);
            }
        }
    });
    if let Some(clip) = chosen {
        app.graphics_clip = Some(clip);
        app.selection = Some(clip);
        app.graphics_layer = 0;
        app.text_draft.clear();
        let _ = app.execute("timeline.select", serde_json::json!({ "clips": [clip.0] }));
        app.status = "Graphic selected".to_string();
    }

    panel.add_space(6.0);
    panel.label(ui::hint("Text, shapes and layers are real FilmCraft graphics. Editing a layer is one undoable engine command."));
}

fn graphic_inspector(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, clip: Option<ClipId>) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("graphic-inspector"));
    ui::panel_header(&mut panel, "Text Inspector");
    let Some(clip) = clip else {
        panel.add_space(6.0);
        panel.label(ui::hint("Create a text graphic, or select one on the timeline."));
        return;
    };
    let listing = app.session.execute("graphics.list", serde_json::json!({ "clip": clip.0 })).ok();
    let Some(listing) = listing else {
        panel.add_space(6.0);
        panel.label(ui::hint("The graphic could not be read; it may have been removed."));
        return;
    };
    let layers: Vec<serde_json::Value> = listing["layers"].as_array().map(|a| a.iter().filter(|v| v.is_object()).cloned().collect()).unwrap_or_default();
    if layers.is_empty() {
        panel.add_space(6.0);
        panel.label(ui::hint("This graphic has no layers. Add a text or shape layer."));
        if ui::accent_button(&mut panel, "Add Text Layer").clicked() {
            new_text(app, true);
        }
        return;
    }
    let max_layer = layers.len().saturating_sub(1);
    if app.graphics_layer > max_layer {
        app.graphics_layer = max_layer;
    }
    let mut selected_layer = app.graphics_layer;
    let mut layer_action = None;
    egui::ScrollArea::vertical().max_height(170.0).show(&mut panel, |ui| {
        for layer in &layers {
            let index = layer["layer"].as_u64().unwrap_or(0) as usize;
            let name = layer["name"].as_str().unwrap_or("Layer");
            let kind = layer["kind"].as_str().unwrap_or("layer");
            let enabled = layer["enabled"].as_bool().unwrap_or(true);
            let row_height = 30.0;
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::click());
            let selected = index == selected_layer;
            if response.hovered() {
                ui.painter().rect_filled(row, 3.0, if selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
            } else if selected {
                ui.painter().rect_filled(row, 3.0, ui::ACCENT_SEL);
            }
            ui::dot(ui, if enabled { ui::ACCENT } else { ui::TEXT_OFF });
            ui.painter().text(
                egui::pos2(row.left() + 18.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{} {}", if kind == "text" { "T" } else { "S" }, name),
                egui::FontId::proportional(10.5),
                if selected { ui::TEXT_HI } else { ui::TEXT_MID },
            );
            ui.painter().text(
                row.right_center() - egui::vec2(6.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                format!("#{}", index + 1),
                egui::FontId::monospace(9.0),
                ui::TEXT_OFF,
            );
            if response.clicked() {
                selected_layer = index;
            }
        }
    });
    if selected_layer != app.graphics_layer {
        app.graphics_layer = selected_layer;
        if let Some(layer) = layers.iter().find(|l| l["layer"].as_u64() == Some(selected_layer as u64)) {
            if let Some(text) = layer["text"].as_str() {
                app.text_draft = text.to_string();
            }
            let _ = app.execute("graphics.selectLayer", serde_json::json!({ "clip": clip.0, "layers": [selected_layer] }));
        }
    }
    let layer = layers.iter().find(|l| l["layer"].as_u64() == Some(selected_layer as u64)).cloned();
    let Some(layer) = layer else {
        panel.label(ui::hint("That layer no longer exists."));
        return;
    };
    let kind = layer["kind"].as_str().unwrap_or("layer").to_string();
    let name = layer["name"].as_str().unwrap_or("Layer").to_string();
    let enabled = layer["enabled"].as_bool().unwrap_or(true);
    let text_type = layer["textType"].as_str().unwrap_or("point").to_string();

    ui::spaced_rule(&mut panel);
    panel.label(egui::RichText::new(name).size(13.0).strong().color(ui::TEXT_HI));
    panel.label(ui::hint(&format!("{} layer  -  index {}", kind, selected_layer + 1)));
    panel.horizontal(|ui| {
        let mut on = enabled;
        if ui.checkbox(&mut on, "Visible").changed() {
            layer_action = Some(serde_json::json!({ "enabled": on }));
        }
        if ui.small_button("Forward").on_hover_text("Bring this layer forward").clicked() {
            run_layer(app, "graphics.bringForward", clip, selected_layer, serde_json::json!({}));
        }
        if ui.small_button("Back").on_hover_text("Send this layer backward").clicked() {
            run_layer(app, "graphics.sendBackward", clip, selected_layer, serde_json::json!({}));
        }
    });

    if kind == "text" {
        ui::section(&mut panel, "TEXT");
        let width = panel.available_width();
        panel.add_sized([width, 58.0], egui::TextEdit::multiline(&mut app.text_draft).hint_text("Text"));
        panel.horizontal(|ui| {
            if ui.small_button("Apply Text").clicked() {
                let text = app.text_draft.clone();
                match app.execute(
                    "graphics.setText",
                    serde_json::json!({
                        "clip": clip.0,
                        "layer": selected_layer,
                        "text": text,
                    }),
                ) {
                    Ok(_) => app.status = "Text updated".to_string(),
                    Err(e) => app.status = e,
                }
            }
            if ui.small_button("Load Layer Text").clicked() {
                app.text_draft = layer["text"].as_str().unwrap_or_default().to_string();
            }
        });
        panel.horizontal(|ui| {
            let point = text_type != "paragraph";
            if ui.radio(point, "Point text").clicked() && !point {
                set_text_type(app, clip, selected_layer, false);
            }
            if ui.radio(!point, "Paragraph").clicked() && point {
                set_text_type(app, clip, selected_layer, true);
            }
        });
    }

    ui::section(&mut panel, "TRANSFORM");
    let position = layer["position"].as_array();
    let mut x = position.and_then(|a| a.first()).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut y = position.and_then(|a| a.get(1)).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut scale = layer["scale"].as_f64().unwrap_or(100.0);
    panel.horizontal(|ui| {
        let mut changed = false;
        changed |= ui.add(egui::DragValue::new(&mut x).prefix("X ").speed(1.0)).changed();
        changed |= ui.add(egui::DragValue::new(&mut y).prefix("Y ").speed(1.0)).changed();
        if changed {
            run_layer(app, "graphics.set", clip, selected_layer, serde_json::json!({ "props": { "position": [x, y] } }));
        }
    });
    if panel.add(egui::Slider::new(&mut scale, 1.0..=400.0).text("Scale %")).changed() {
        run_layer(app, "graphics.set", clip, selected_layer, serde_json::json!({ "props": { "scale": scale } }));
    }
    panel.horizontal(|ui| {
        if ui.small_button("Center H").clicked() {
            run_layer(app, "graphics.alignFrame.hcenter", clip, selected_layer, serde_json::json!({}));
        }
        if ui.small_button("Center V").clicked() {
            run_layer(app, "graphics.alignFrame.vcenter", clip, selected_layer, serde_json::json!({}));
        }
    });

    if let Some(props) = layer_action {
        run_layer(app, "graphics.set", clip, selected_layer, serde_json::json!({ "props": props }));
    }
    ui::section(&mut panel, "LAYERS");
    panel.horizontal(|ui| {
        if ui.small_button("+ Text").clicked() {
            new_text(app, true);
        }
        if ui.small_button("+ Rect").clicked() {
            new_shape_into(app, Shape::Rectangle, Some(clip));
        }
        if ui.small_button("+ Ellipse").clicked() {
            new_shape_into(app, Shape::Ellipse, Some(clip));
        }
    });
    if ui::full_button(&mut panel, "Delete This Layer").clicked() {
        match app.execute("graphics.deleteLayer", serde_json::json!({ "clip": clip.0, "layer": selected_layer })) {
            Ok(_) => {
                app.graphics_layer = 0;
                app.status = "Layer removed".to_string();
            }
            Err(e) => app.status = e,
        }
    }
    ui::footer_hint(&mut panel, "Position and scale are read from the evaluated layer each frame; commands are undoable.");
}

fn set_text_type(app: &mut App, clip: ClipId, layer: usize, paragraph: bool) {
    match app.execute(
        "graphics.setTextType",
        serde_json::json!({
            "clip": clip.0,
            "layer": layer,
            "type": if paragraph { "paragraph" } else { "point" },
        }),
    ) {
        Ok(_) => app.status = if paragraph { "Paragraph text".to_string() } else { "Point text".to_string() },
        Err(e) => app.status = e,
    }
}

fn run_layer(app: &mut App, command: &str, clip: ClipId, layer: usize, extra: serde_json::Value) {
    let mut params = serde_json::json!({ "clip": clip.0, "layer": layer });
    if let (Some(dst), Some(src)) = (params.as_object_mut(), extra.as_object()) {
        for (key, value) in src {
            dst.insert(key.clone(), value.clone());
        }
    }
    match app.execute(command, params) {
        Ok(_) => app.status = "Graphic updated".to_string(),
        Err(e) => app.status = e,
    }
}

fn new_text(app: &mut App, into_current: bool) {
    let text = if app.text_draft.trim().is_empty() { "New Text".to_string() } else { app.text_draft.clone() };
    let mut params = serde_json::json!({
        "text": text,
        "size": app.text_size,
        "seconds": 5.0,
    });
    if into_current {
        if let Some(clip) = current_graphic(app) {
            params["clip"] = serde_json::json!(clip.0);
        } else {
            params["newClip"] = serde_json::json!(true);
        }
    } else {
        params["newClip"] = serde_json::json!(true);
    }
    match app.execute("graphics.newText", params) {
        Ok(value) => adopt_graphic(app, &value, text),
        Err(e) => app.status = e,
    }
}

fn new_shape(app: &mut App, shape: Shape) {
    new_shape_into(app, shape, current_graphic(app));
}

fn new_shape_into(app: &mut App, shape: Shape, into: Option<ClipId>) {
    let mut params = serde_json::json!({
        "shape": match shape {
            Shape::Rectangle => "rectangle",
            Shape::Ellipse => "ellipse",
        },
        "size": [400.0, 220.0],
        "seconds": 5.0,
    });
    if let Some(clip) = into {
        params["clip"] = serde_json::json!(clip.0);
    }
    match app.execute("graphics.newShape", params) {
        Ok(value) => adopt_graphic(app, &value, String::new()),
        Err(e) => app.status = e,
    }
}

fn adopt_graphic(app: &mut App, value: &serde_json::Value, text: String) {
    let Some(clip) = value["clip"].as_u64().map(ClipId) else {
        app.status = "The graphic was created but its clip id was missing".to_string();
        return;
    };
    app.graphics_clip = Some(clip);
    app.selection = Some(clip);
    app.graphics_layer = value["layer"].as_u64().unwrap_or(0) as usize;
    app.text_draft = text;
    let _ = app.execute("timeline.select", serde_json::json!({ "clips": [clip.0] }));
    app.status = "Graphic layer created".to_string();
}
