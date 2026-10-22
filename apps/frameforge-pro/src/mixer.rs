//! The Audio workspace: the sequence's mixer — one strip per audio track plus
//! the Mix, with fader, pan, mute and solo, all written through the engine's
//! `mixer.setStrip` command (undoable, the same state the audio clock's mixer
//! graph reads).

use crate::app::App;

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let Some(s) = app.sequence() else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("mixer"));
        area.add_space(16.0);
        area.heading("Audio");
        area.label("No sequence.");
        return;
    };

    // Read-only pass: every strip's state (the command writes go after).
    let tracks: Vec<(String, bool, bool, f64, f64)> = s.audio_tracks.iter().map(|t| (t.name.clone(), t.muted, t.solo, t.volume_db, t.pan)).collect();
    let master: f64 = s.master_volume_db;
    drop(s);

    ui.painter().rect_filled(body, 0.0, ui.visuals().panel_fill);
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("mixer"));
    area.add_space(12.0);
    area.heading("Audio Mixer");
    area.colored_label(
        egui::Color32::GRAY,
        "Faders, pan, mute and solo — through the engine's mixer.setStrip command (undoable). The audio clock's mixer graph reads exactly this state.",
    );
    area.separator();

    // The strips: one per audio track, horizontally.
    let strip_w = 190.0;
    let mut writes: Vec<(String, serde_json::Value)> = Vec::new();
    area.horizontal(|ui| {
        for (name, muted, solo, volume_db, pan) in &tracks {
            ui.vertical(|ui| {
                ui.set_min_width(strip_w);
                ui.heading(name);
                let mut mute = *muted;
                if ui.selectable_label(*muted, if *muted { "Mute ●" } else { "Mute" }).clicked() {
                    mute = !*muted;
                    writes.push((name.clone(), serde_json::json!({"strip": name, "muted": mute})));
                }
                let mut solo_on = *solo;
                if ui.selectable_label(*solo, if *solo { "Solo ●" } else { "Solo" }).clicked() {
                    solo_on = !*solo;
                    writes.push((name.clone(), serde_json::json!({"strip": name, "solo": solo_on})));
                }
                let mut vol = *volume_db;
                if ui.add(egui::Slider::new(&mut vol, -96.0..=12.0).vertical().text("dB")).changed() {
                    writes.push((name.clone(), serde_json::json!({"strip": name, "volumeDb": vol})));
                }
                ui.label(format!("{vol:+.1} dB"));
                let mut p = *pan;
                if ui.add(egui::Slider::new(&mut p, -100.0..=100.0).text("Pan")).changed() {
                    writes.push((name.clone(), serde_json::json!({"strip": name, "pan": p})));
                }
                ui.separator();
            });
        }
        // The Mix (master).
        ui.vertical(|ui| {
            ui.set_min_width(strip_w);
            ui.heading("Mix");
            ui.colored_label(egui::Color32::GRAY, "master");
            let mut vol = master;
            if ui.add(egui::Slider::new(&mut vol, -96.0..=12.0).vertical().text("dB")).changed() {
                writes.push(("Mix".to_string(), serde_json::json!({"strip": "Mix", "volumeDb": vol})));
            }
            ui.label(format!("{vol:+.1} dB"));
        });
    });

    for (_, params) in writes {
        if let Err(e) = app.execute("mixer.setStrip", params) {
            app.status = e;
        }
    }
}
