//! The Compositing workspace: the selected media item's real EffectCraft
//! composition, its layers and their video switches. Creating a composition
//! from nothing is not exposed by the document model yet, so the panel says so
//! instead of inventing layers.

use filmcraft_project::{ItemId, ItemKind};

use crate::app::App;
use crate::ui;

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let item = app.selected_clip().map(|(_, item)| item).or(app.effects_item).or_else(|| first_composed_item(app));

    let left_w = 320.0_f32.clamp(260.0, (body.width() - 560.0).max(260.0));
    let right_w = 400.0_f32.clamp(300.0, (body.width() - left_w - 360.0).max(300.0));
    let left = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + left_w, body.max.y));
    let right = egui::Rect::from_min_max(egui::pos2(body.max.x - right_w, body.min.y), body.max);
    let centre = egui::Rect::from_min_max(egui::pos2(left.max.x, body.min.y), egui::pos2(right.min.x, body.max.y));

    media_browser(app, ui, left, item);
    crate::edit::monitor_panel(app, ui, centre);
    layer_panel(app, ui, right, item);
}

fn first_composed_item(app: &App) -> Option<ItemId> {
    app.compositions.keys().copied().next()
}

fn media_items(app: &App) -> Vec<(ItemId, String, usize)> {
    let mut ids = Vec::new();
    app.session.project.root.all_items(&mut ids);
    ids.into_iter()
        .filter_map(|id| {
            let item = app.session.project.items.get(&id)?;
            matches!(&item.kind, ItemKind::Media(_)).then(|| (id, item.name.clone(), app.compositions.get(&id).map(|c| c.comp_layers().len()).unwrap_or(0)))
        })
        .collect()
}

fn media_browser(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, selected: Option<ItemId>) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.right_top(), rect.right_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("comp-browser"));
    ui::panel_header(&mut panel, "Compositing");
    panel.add_space(6.0);
    panel.label(ui::hint("A composition renders its clip after the effect chain. Pick the media item to inspect."));
    ui::spaced_rule(&mut panel);
    let rows = media_items(app);
    let mut chosen = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut panel, |ui| {
        for (id, name, layers) in rows {
            let is_selected = selected == Some(id);
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 3.0, if is_selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
            } else {
                ui::card(ui, row, is_selected);
            }
            if is_selected {
                ui::selected_bar(ui.painter(), row);
            }
            ui.painter().circle_filled(row.left_center() + egui::vec2(11.0, 0.0), 4.0, if layers > 0 { ui::ACCENT } else { ui::TEXT_OFF });
            ui.painter().text(
                row.left_center() + egui::vec2(23.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(11.0),
                if is_selected { ui::TEXT_HI } else { ui::TEXT_MID },
            );
            ui.painter().text(
                row.right_center() - egui::vec2(7.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                if layers > 0 { format!("{layers} layers") } else { "no comp".to_string() },
                egui::FontId::proportional(9.0),
                ui::TEXT_LO,
            );
            if response.clicked() {
                chosen = Some(id);
            }
        }
    });
    if let Some(id) = chosen {
        app.effects_item = Some(id);
        app.comp_layer = 0;
        app.status = "Composition target selected".to_string();
    }
    panel.add_space(6.0);
    panel.label(ui::hint(
        "Text and solid layers are real EffectCraft layers. Footage layers from an imported project remain visible but only the clip's own media has a picture.",
    ));
}

fn layer_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, item: Option<ItemId>) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("comp-layers"));
    ui::panel_header(&mut panel, "Composition Layers");
    let Some(item) = item else {
        panel.add_space(6.0);
        panel.label(ui::hint("Select a media item in the browser or on the timeline."));
        return;
    };
    let name = app.session.project.items.get(&item).map(|item| item.name.clone()).unwrap_or_else(|| "Media".to_string());
    panel.label(egui::RichText::new(name).size(12.0).strong().color(ui::TEXT_HI));

    let Some(composition) = app.compositions.get(&item) else {
        panel.add_space(4.0);
        panel.label(ui::hint(
            "This media item has no composition. Compositions are created by importing an EffectCraft project; the shell cannot yet create one from scratch.",
        ));
        if ui::full_button(&mut panel, "Open Effects Workspace").clicked() {
            app.workspace = crate::app::Workspace::Effects;
        }
        return;
    };
    if !composition.has_comp() {
        panel.add_space(4.0);
        panel.label(ui::hint("The composition holds an effect chain but no compositing comp. Import an EffectCraft project to add layers."));
        if ui::full_button(&mut panel, "Open Effects Workspace").clicked() {
            app.workspace = crate::app::Workspace::Effects;
        }
        return;
    }
    let layers = composition.comp_layers();
    if layers.is_empty() {
        panel.add_space(4.0);
        panel.label(ui::hint("The comp has no layers. Add a text or solid layer below."));
    } else {
        ui::section(&mut panel, "LAYERS");
        let max = layers.len().saturating_sub(1);
        if app.comp_layer > max {
            app.comp_layer = max;
        }
        for (index, (name, video)) in layers.iter().enumerate() {
            let selected = app.comp_layer == index;
            let (row, response) = panel.allocate_exact_size(egui::vec2(panel.available_width(), 30.0), egui::Sense::click());
            if response.hovered() {
                panel.painter().rect_filled(row, 3.0, if selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
            } else if selected {
                panel.painter().rect_filled(row, 3.0, ui::ACCENT_SEL);
            }
            if selected {
                ui::selected_bar(panel.painter(), row);
            }
            let mut video = *video;
            if panel
                .put(egui::Rect::from_min_size(row.left_center() + egui::vec2(5.0, -7.0), egui::vec2(16.0, 14.0)), egui::Checkbox::without_text(&mut video))
                .changed()
            {
                toggle_layer(app, item, index);
            }
            panel.painter().text(
                row.left_center() + egui::vec2(28.0, 0.0),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(10.5),
                if selected { ui::TEXT_HI } else { ui::TEXT_MID },
            );
            panel.painter().text(
                row.right_center() - egui::vec2(6.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                format!("#{}", index + 1),
                egui::FontId::monospace(9.0),
                ui::TEXT_OFF,
            );
            if response.clicked() {
                app.comp_layer = index;
            }
        }
    }

    ui::section(&mut panel, "ADD LAYER");
    let width = panel.available_width();
    panel.add_sized([width, 44.0], egui::TextEdit::multiline(&mut app.text_draft).hint_text("Text for a new layer"));
    panel.horizontal(|ui| {
        if ui.small_button("+ Text").clicked() {
            let text = if app.text_draft.trim().is_empty() { "Text".to_string() } else { app.text_draft.clone() };
            if mutate(app, item, |comp| comp.comp_add_text_layer(&text)) {
                app.comp_layer = 0;
                app.status = "Text layer added".to_string();
            }
        }
        if ui.small_button("+ Solid").clicked() && mutate(app, item, |comp| comp.comp_add_solid_layer([0.08, 0.08, 0.08])) {
            app.comp_layer = 0;
            app.status = "Solid layer added".to_string();
        }
    });

    ui::section(&mut panel, "SELECTED LAYER");
    if layers.is_empty() {
        panel.label(ui::hint("Nothing selected."));
    } else {
        let index = app.comp_layer.min(layers.len() - 1);
        let (name, video) = &layers[index];
        ui::kv(&mut panel, "Layer", format!("{} of {}", index + 1, layers.len()));
        ui::kv(&mut panel, "Name", name.clone());
        ui::kv(&mut panel, "Video", if *video { "On" } else { "Off" });
        panel.horizontal(|ui| {
            if ui.small_button("Hide / Show").clicked() {
                toggle_layer(app, item, index);
            }
            if ui.small_button("Remove").clicked() && mutate(app, item, |comp| comp.comp_remove_layer(index)) {
                app.comp_layer = 0;
                app.status = "Layer removed".to_string();
            }
        });
    }

    ui::spaced_rule(&mut panel);
    if ui::full_button(&mut panel, "Edit Effect Chain in Effects").clicked() {
        app.effects_item = Some(item);
        app.workspace = crate::app::Workspace::Effects;
    }
    ui::footer_hint(&mut panel, "Layer edits join the same chronological undo order as timeline edits.");
}

fn toggle_layer(app: &mut App, item: ItemId, index: usize) {
    if mutate(app, item, |comp| comp.comp_toggle_layer(index)) {
        app.status = "Layer visibility toggled".to_string();
    }
}

fn mutate(app: &mut App, item: ItemId, f: impl FnOnce(&mut frameforge_bridge::Composition) -> bool) -> bool {
    let before = app.compositions.get(&item).cloned();
    let changed = {
        let Some(composition) = app.compositions.get_mut(&item) else { return false };
        f(composition)
    };
    if !changed {
        return false;
    }
    let after = app.compositions.get(&item).cloned();
    let gesture = app.history.begin_gesture(true);
    app.record_comp_change(item, before, after, gesture);
    app.history.end_gesture(false);
    true
}
