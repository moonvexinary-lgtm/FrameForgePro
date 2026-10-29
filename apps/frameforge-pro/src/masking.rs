//! The Masking workspace: real FilmCraft masks on the selected video clip,
//! their feather/opacity/mode, and the engine's own mask tracker. XMem status
//! is reported honestly: the app does not run or download inference here.

use filmcraft_project::{ClipId, EffectKind};

use crate::app::App;
use crate::ui;

#[derive(Clone, Copy)]
enum MaskShape {
    Ellipse,
    Rectangle,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let left_w = 320.0_f32.clamp(260.0, (body.width() - 560.0).max(260.0));
    let right_w = 380.0_f32.clamp(280.0, (body.width() - left_w - 360.0).max(280.0));
    let left = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + left_w, body.max.y));
    let right = egui::Rect::from_min_max(egui::pos2(body.max.x - right_w, body.min.y), body.max);
    let centre = egui::Rect::from_min_max(egui::pos2(left.max.x, body.min.y), egui::pos2(right.min.x, body.max.y));

    let clip = app.selection.filter(|clip| on_video_track(app, *clip)).or_else(|| first_video_clip(app));
    mask_tools(app, ui, left, clip);
    crate::edit::monitor_panel(app, ui, centre);
    mask_inspector(app, ui, right, clip);
}

fn on_video_track(app: &App, clip: ClipId) -> bool {
    app.sequence().is_some_and(|seq| seq.video_tracks.iter().any(|track| track.items.iter().any(|item| item.id == clip)))
}

fn first_video_clip(app: &App) -> Option<ClipId> {
    let seq = app.sequence()?;
    seq.video_tracks.iter().flat_map(|track| track.items.iter()).map(|item| item.id).next()
}

fn mask_effect(app: &App, clip: ClipId) -> Option<usize> {
    let seq = app.sequence()?;
    let (_, item) = seq.find_item(clip)?;
    item.effects.iter().position(|effect| effect.effect != "motion" && effect.def().is_some_and(|def| def.kind == EffectKind::Video))
}

fn mask_tools(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, clip: Option<ClipId>) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.right_top(), rect.right_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("mask-tools"));
    ui::panel_header(&mut panel, "Masking");
    let Some(clip) = clip else {
        panel.add_space(6.0);
        panel.label(ui::hint("Select a video clip on the timeline to mask it."));
        return;
    };
    let name = app.sequence().and_then(|seq| seq.find_item(clip).map(|(_, item)| item.name.clone())).unwrap_or_else(|| "Video clip".to_string());
    panel.label(egui::RichText::new(name).size(11.0).color(ui::TEXT_MID));
    let has_maskable = mask_effect(app, clip).is_some();
    if !has_maskable {
        panel.add_space(3.0);
        panel.label(ui::hint("This clip has no maskable video effect yet."));
        if ui::accent_button(&mut panel, "Add Opacity Effect").clicked() {
            match app.execute("effects.apply", serde_json::json!({ "clips": [clip.0], "effect": "opacity" })) {
                Ok(_) => app.status = "Opacity effect added; masks can now be created".to_string(),
                Err(e) => app.status = e,
            }
        }
        return;
    }

    ui::section(&mut panel, "NEW MASK");
    panel.horizontal(|ui| {
        if ui.small_button("Ellipse").clicked() {
            create_mask(app, clip, MaskShape::Ellipse);
        }
        if ui.small_button("Rectangle").clicked() {
            create_mask(app, clip, MaskShape::Rectangle);
        }
    });
    panel.add(egui::Slider::new(&mut app.mask_feather, 0.0..=200.0).text("Feather"));
    panel.add(egui::Slider::new(&mut app.mask_opacity, 0.0..=100.0).text("Opacity"));
    panel.checkbox(&mut app.mask_inverted, "Invert");

    let masks = list_masks(app, clip);
    ui::spaced_rule(&mut panel);
    ui::section(&mut panel, "MASKS ON THIS CLIP");
    if masks.is_empty() {
        panel.label(ui::hint("No masks yet. Add an ellipse or rectangle above."));
    }
    let mut chosen = None;
    for mask in &masks {
        let layer = mask["mask"].as_u64().unwrap_or(0);
        let effect = mask["effect"].as_u64().unwrap_or(0) as usize;
        let name = mask["name"].as_str().unwrap_or("Mask").to_string();
        let selected = mask["selected"].as_bool().unwrap_or(false);
        let (row, response) = panel.allocate_exact_size(egui::vec2(panel.available_width(), 30.0), egui::Sense::click());
        if response.hovered() {
            panel.painter().rect_filled(row, 3.0, if selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
        } else if selected {
            panel.painter().rect_filled(row, 3.0, ui::ACCENT_SEL);
        }
        if selected {
            ui::selected_bar(panel.painter(), row);
        }
        panel.painter().circle_stroke(row.left_center() + egui::vec2(11.0, 0.0), 5.0, egui::Stroke::new(1.0, ui::ACCENT));
        panel.painter().text(
            row.left_center() + egui::vec2(22.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(10.5),
            if selected { ui::TEXT_HI } else { ui::TEXT_MID },
        );
        panel.painter().text(
            row.right_center() - egui::vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("E{} M{}", effect + 1, layer + 1),
            egui::FontId::monospace(9.0),
            ui::TEXT_OFF,
        );
        if response.clicked() {
            chosen = Some((effect, layer as usize, name));
        }
    }
    if let Some((effect, mask, name)) = chosen {
        let _ = app.execute("masks.select", serde_json::json!({ "clip": clip.0, "effect": effect, "mask": mask }));
        app.mask_name = name;
        app.status = "Mask selected".to_string();
    }

    ui::spaced_rule(&mut panel);
    panel.label(ui::hint(
        "Masks are applied by the renderer. The engine's mask tracker follows the selected mask on this clip; XMem is a separate, not-yet-wired R3 path.",
    ));
}

fn create_mask(app: &mut App, clip: ClipId, shape: MaskShape) {
    let Some(effect) = mask_effect(app, clip) else {
        app.status = "Add a video effect before creating a mask".to_string();
        return;
    };
    let (w, h) = app.sequence().map(|seq| (seq.settings.width as f64, seq.settings.height as f64)).unwrap_or((1280.0, 720.0));
    let params = serde_json::json!({
        "clip": clip.0,
        "effect": effect,
        "shape": match shape {
            MaskShape::Ellipse => "ellipse",
            MaskShape::Rectangle => "rectangle",
        },
        "center": [w / 2.0, h / 2.0],
        "size": [w * 0.36, h * 0.46],
    });
    let feather = app.mask_feather;
    let opacity = app.mask_opacity;
    let inverted = app.mask_inverted;
    match app.execute("masks.add", params) {
        Ok(value) => {
            let mask = value["mask"].as_u64().unwrap_or(0) as usize;
            let effect = value["effect"].as_u64().unwrap_or(0) as usize;
            let name = value["name"].as_str().unwrap_or("Mask").to_string();
            let _ = app.execute(
                "masks.set",
                serde_json::json!({
                    "clip": clip.0,
                    "effect": effect,
                    "mask": mask,
                    "feather": feather,
                    "opacity": opacity,
                    "inverted": inverted,
                }),
            );
            app.mask_name = name;
            app.status = "Mask created".to_string();
        }
        Err(e) => app.status = e,
    }
}

fn list_masks(app: &mut App, clip: ClipId) -> Vec<serde_json::Value> {
    app.session
        .execute("masks.list", serde_json::json!({ "clip": clip.0 }))
        .ok()
        .and_then(|value| value["masks"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|mask| mask["clip"].as_u64() == Some(clip.0))
        .collect()
}

fn mask_inspector(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, clip: Option<ClipId>) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("mask-inspector"));
    ui::panel_header(&mut panel, "Mask Properties");
    let Some(clip) = clip else {
        panel.add_space(6.0);
        panel.label(ui::hint("No video clip selected."));
        return;
    };
    let masks = list_masks(app, clip);
    let selected = masks.iter().find(|mask| mask["selected"].as_bool().unwrap_or(false)).or_else(|| masks.first()).cloned();
    if let Some(mask) = selected {
        let effect = mask["effect"].as_u64().unwrap_or(0) as usize;
        let index = mask["mask"].as_u64().unwrap_or(0) as usize;
        let name = mask["name"].as_str().unwrap_or("Mask").to_string();
        let mode = mask["mode"].as_str().unwrap_or("Add").to_string();
        let inverted = mask["inverted"].as_bool().unwrap_or(false);
        let mut feather = mask["feather"].as_f64().unwrap_or(0.0) as f32;
        let mut opacity = mask["opacity"].as_f64().unwrap_or(100.0) as f32;
        let mut expansion = mask["expansion"].as_f64().unwrap_or(0.0) as f32;
        let vertices = mask["path"]["vertices"].as_array().map(|a| a.len()).unwrap_or(0);
        let keyframes = mask["pathKeyframes"].as_array().map(|a| a.len()).unwrap_or(0);

        panel.label(egui::RichText::new(&name).size(13.0).strong().color(ui::TEXT_HI));
        panel.label(ui::hint(&format!("Effect {}  -  mask {}  -  {} vertices", effect + 1, index + 1, vertices)));
        ui::section(&mut panel, "NAME");
        let width = panel.available_width();
        panel.add_sized([width, 22.0], egui::TextEdit::singleline(&mut app.mask_name));
        panel.horizontal(|ui| {
            if ui.small_button("Rename").clicked() {
                let new_name = app.mask_name.clone();
                set_mask(app, clip, effect, index, serde_json::json!({ "name": new_name }));
            }
            if ui.small_button("Load Name").clicked() {
                app.mask_name = name.clone();
            }
        });
        ui::section(&mut panel, "MASK");
        let labels = ["Add", "Subtract", "Intersect", "Lighten", "Darken", "Difference", "None"];
        egui::ComboBox::from_id_salt("mask-mode").selected_text(&mode).show_ui(&mut panel, |ui| {
            for label in labels {
                if ui.selectable_label(mode == label, label).clicked() {
                    set_mask(app, clip, effect, index, serde_json::json!({ "mode": label }));
                }
            }
        });
        let mut any = false;
        any |= panel.add(egui::Slider::new(&mut feather, 0.0..=200.0).text("Feather")).changed();
        any |= panel.add(egui::Slider::new(&mut opacity, 0.0..=100.0).text("Opacity")).changed();
        any |= panel.add(egui::Slider::new(&mut expansion, -200.0..=200.0).text("Expansion")).changed();
        let mut inv = inverted;
        let inv_changed = panel.checkbox(&mut inv, "Inverted").changed();
        if any {
            set_mask(app, clip, effect, index, serde_json::json!({ "feather": feather, "opacity": opacity, "expansion": expansion }));
        }
        if inv_changed {
            set_mask(app, clip, effect, index, serde_json::json!({ "inverted": inv }));
        }
        ui::section(&mut panel, "TRACKING");
        panel.label(ui::hint(&format!("{keyframes} path keyframe(s)")));
        panel.horizontal(|ui| {
            if ui.small_button("Track Forward").clicked() {
                track_mask(app, clip, effect, index, false);
            }
            if ui.small_button("Track Backward").clicked() {
                track_mask(app, clip, effect, index, true);
            }
        });
        if ui::full_button(&mut panel, "Delete Mask").clicked() {
            match app.execute("masks.remove", serde_json::json!({ "clip": clip.0, "effect": effect, "mask": index })) {
                Ok(_) => app.status = "Mask removed".to_string(),
                Err(e) => app.status = e,
            }
        }
    } else {
        panel.add_space(6.0);
        panel.label(ui::hint("No mask selected. Create one in the Masking panel."));
    }

    ui::spaced_rule(&mut panel);
    ui::section(&mut panel, "ROTO AI (XMEM)");
    panel.label(egui::RichText::new("Not wired in this build").size(11.0).strong().color(ui::WARN));
    panel.label(ui::hint(
        "XMem is the intended temporal VOS engine. The app job runner, timeout/kill policy, resume-from-corrections and enforced memory ceiling are still R3 work, so this panel never claims inference. Weights are never downloaded or bundled.",
    ));
    panel.add_space(3.0);
    ui::kv(&mut panel, "Local probe", "measured separately");
    ui::kv(&mut panel, "CI inference", "not measured");
    ui::kv(&mut panel, "Runtime/weights", "not packaged");
    ui::footer_hint(&mut panel, "The mask tracker above is FilmCraft's engine path, not XMem.");
}

fn set_mask(app: &mut App, clip: ClipId, effect: usize, mask: usize, extra: serde_json::Value) {
    let mut params = serde_json::json!({ "clip": clip.0, "effect": effect, "mask": mask });
    if let (Some(dst), Some(src)) = (params.as_object_mut(), extra.as_object()) {
        for (key, value) in src {
            dst.insert(key.clone(), value.clone());
        }
    }
    match app.execute("masks.set", params) {
        Ok(_) => app.status = "Mask updated".to_string(),
        Err(e) => app.status = e,
    }
}

fn track_mask(app: &mut App, clip: ClipId, effect: usize, mask: usize, backward: bool) {
    match app.execute(
        "masks.track",
        serde_json::json!({
            "clip": clip.0,
            "effect": effect,
            "mask": mask,
            "direction": if backward { "backward" } else { "forward" },
        }),
    ) {
        Ok(_) => app.status = "Mask tracking started".to_string(),
        Err(e) => app.status = e,
    }
}
