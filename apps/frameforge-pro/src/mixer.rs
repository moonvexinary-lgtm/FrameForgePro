//! The Audio workspace: the sequence's mixer — one strip per audio track plus
//! the Mix, with fader, pan, mute and solo, all written through the engine's
//! `mixer.setStrip` command (undoable, the same state the audio clock's mixer
//! graph reads).

use crate::app::App;
use crate::ui;

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let Some(s) = app.sequence() else {
        ui.painter().rect_filled(body, 0.0, ui::BG_APP);
        let mut area = ui::panel(ui, body, "mixer");
        ui::panel_header(&mut area, "Audio");
        ui::inset(&mut area);
        area.label(ui::hint("No sequence. Create one to mix its tracks."));
        return;
    };

    // Read-only pass: every strip's state (the command writes go after; the
    // borrow of the sequence ends at the last read).
    let tracks: Vec<(String, bool, bool, f64, f64)> = s.audio_tracks.iter().map(|t| (t.name.clone(), t.muted, t.solo, t.volume_db, t.pan)).collect();
    let master: f64 = s.master_volume_db;

    let mut area = ui::panel(ui, body, "mixer");
    ui::panel_header(&mut area, "Audio Mixer");
    ui::inset(&mut area);
    area.label(ui::hint(
        "Faders, pan, mute and solo — through the engine's mixer.setStrip command (undoable). The audio clock's mixer graph reads exactly this state.",
    ));
    ui::section(&mut area, "TRACK STRIPS");

    // The strips: one per audio track, horizontally.
    let strip_w = 190.0;
    let mut writes: Vec<(String, serde_json::Value)> = Vec::new();
    egui::ScrollArea::horizontal().show(&mut area, |ui| {
        ui.horizontal_top(|ui| {
            for (name, muted, solo, volume_db, pan) in &tracks {
                ui.vertical(|ui| {
                    ui.set_min_width(strip_w);
                    ui.set_max_width(strip_w);
                    ui.label(egui::RichText::new(name).size(11.0).strong().color(ui::TEXT_HI));
                    ui.label(ui::meta("AUDIO TRACK"));
                    ui.add_space(4.0);
                    if ui.selectable_label(*muted, if *muted { "Mute ●" } else { "Mute" }).clicked() {
                        writes.push((name.clone(), serde_json::json!({"strip": name, "muted": !*muted})));
                    }
                    if ui.selectable_label(*solo, if *solo { "Solo ●" } else { "Solo" }).clicked() {
                        writes.push((name.clone(), serde_json::json!({"strip": name, "solo": !*solo})));
                    }
                    let mut vol = *volume_db;
                    if ui.add(egui::Slider::new(&mut vol, -96.0..=12.0).vertical().text("dB")).changed() {
                        writes.push((name.clone(), serde_json::json!({"strip": name, "volumeDb": vol})));
                    }
                    ui.label(egui::RichText::new(format!("{vol:+.1} dB")).size(10.5).color(ui::TEXT_LO));
                    let mut p = *pan;
                    if ui.add(egui::Slider::new(&mut p, -100.0..=100.0).text("Pan")).changed() {
                        writes.push((name.clone(), serde_json::json!({"strip": name, "pan": p})));
                    }
                });
                ui.add_space(6.0);
            }
            // The Mix (master).
            ui.vertical(|ui| {
                ui.set_min_width(strip_w);
                ui.set_max_width(strip_w);
                ui.label(egui::RichText::new("Mix").size(11.0).strong().color(ui::ACCENT_HI));
                ui.label(ui::meta("MASTER"));
                ui.add_space(4.0);
                let mut vol = master;
                if ui.add(egui::Slider::new(&mut vol, -96.0..=12.0).vertical().text("dB")).changed() {
                    writes.push(("Mix".to_string(), serde_json::json!({"strip": "Mix", "volumeDb": vol})));
                }
                ui.label(egui::RichText::new(format!("{vol:+.1} dB")).size(10.5).color(ui::TEXT_LO));
            });
        });
    });

    for (_, params) in writes {
        if let Err(e) = app.execute("mixer.setStrip", params) {
            app.status = e;
        }
    }

    ui::footer_hint(&mut area, "Changes go through the same mixer graph the audio clock renders, so what you hear is the mix you see.");
}
