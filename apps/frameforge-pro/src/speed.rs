//! Speed, direction and freeze-frame dialog (T23).
//!
//! The dialog is session state: it starts from the selected clip's current
//! rate, and Apply runs the engine's own `clip.speedDuration` command so the
//! duration maths, the linked pair and the interpolation mode stay in one
//! undoable edit. Freeze runs `clip.frameHoldOptions`, which holds one source
//! frame on the clip and leaves its timeline length alone.

use filmcraft_project::TimeInterpolation;

use crate::app::App;
use crate::ui;

/// Which source frame a freeze holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldOn {
    In,
    Out,
    Playhead,
}

impl HoldOn {
    pub const ALL: [HoldOn; 3] = [HoldOn::In, HoldOn::Out, HoldOn::Playhead];

    /// The engine's `holdOn` value.
    pub fn id(self) -> &'static str {
        match self {
            HoldOn::In => "in",
            HoldOn::Out => "out",
            HoldOn::Playhead => "playhead",
        }
    }

    /// The dialog's label.
    pub fn name(self) -> &'static str {
        match self {
            HoldOn::In => "In point",
            HoldOn::Out => "Out point",
            HoldOn::Playhead => "Playhead",
        }
    }
}

/// The open Speed / Duration dialog. Never saved with the project.
#[derive(Clone, Debug)]
pub struct SpeedDialog {
    /// Playback rate as a percentage (100 = real time).
    pub percent: f64,
    pub reverse: bool,
    /// Push later clips by the length change.
    pub ripple: bool,
    pub interpolation: TimeInterpolation,
    pub hold_on: HoldOn,
}

/// Open the dialog for the selected clip, seeded from its current rate and
/// direction so Apply starts from what the timeline shows.
pub fn open(app: &mut App) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = "Select a clip to change its speed".to_string();
        return;
    };
    let Some((_, item)) = app.sequence().and_then(|seq| seq.find_item(clip)) else { return };
    app.speed = Some(SpeedDialog {
        percent: (item.speed.abs() * 100.0).clamp(1.0, 10_000.0),
        reverse: item.reverse,
        ripple: false,
        interpolation: item.time_interpolation,
        hold_on: HoldOn::In,
    });
}

/// Apply the dialog to the selected clip. One undoable engine edit.
pub fn apply(app: &mut App, dialog: &SpeedDialog) -> Result<(), String> {
    let _ = (dialog.percent, dialog.reverse, dialog.ripple, dialog.interpolation);
    let message = "Speed/Duration is not implemented".to_string();
    app.status = message.clone();
    Err(message)
}

/// Freeze the selected video clip on one source frame.
pub fn freeze(app: &mut App, hold_on: HoldOn) -> Result<(), String> {
    app.status = format!("Freeze frame ({}) is not implemented", hold_on.name());
    Err(app.status.clone())
}

/// Remove a frame hold from the selected clip.
pub fn clear_hold(app: &mut App) -> Result<(), String> {
    app.status = "Removing a frame hold is not implemented".to_string();
    Err(app.status.clone())
}

/// Draw the dialog when one is open. Called every frame from `App::ui_frame`,
/// so it floats over whatever workspace is showing.
pub fn show(app: &mut App, ui: &egui::Ui) {
    let Some(mut dialog) = app.speed.take() else { return };
    let mut close = false;
    egui::Window::new("Speed / Duration")
        .id(egui::Id::new("ff-speed-window"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.set_min_width(340.0);
            let name = clip_name(app);
            ui.label(ui::hint(&format!("{name} - the clip changes at its start; a linked pair changes together.")));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Speed").size(11.0).color(ui::TEXT_MID));
                ui.add(egui::DragValue::new(&mut dialog.percent).range(1.0..=10_000.0).speed(5.0).suffix(" %"));
            });
            ui.horizontal(|ui| {
                for preset in [25.0, 50.0, 100.0, 200.0, 400.0] {
                    let label = format!("{}%", preset as i64);
                    if action(ui, &format!("ff-speed-preset-{}", preset as i64), &label, false).clicked() {
                        dialog.percent = preset;
                    }
                }
            });
            ui.checkbox(&mut dialog.reverse, "Reverse");
            ui.checkbox(&mut dialog.ripple, "Ripple (shift the clips after it)");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Time interpolation").size(10.5).color(ui::TEXT_LO));
                egui::ComboBox::from_id_salt("ff-speed-interpolation").selected_text(dialog.interpolation.label()).show_ui(ui, |ui| {
                    for mode in TimeInterpolation::ALL {
                        ui.selectable_value(&mut dialog.interpolation, mode, mode.label());
                    }
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if action(ui, "ff-speed-apply", "Apply", true).clicked() {
                    close = apply(app, &dialog).is_ok();
                }
                if action(ui, "ff-speed-close", "Close", false).clicked() {
                    close = true;
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Freeze on").size(10.5).color(ui::TEXT_LO));
                egui::ComboBox::from_id_salt("ff-speed-hold-on").selected_text(dialog.hold_on.name()).show_ui(ui, |ui| {
                    for mode in HoldOn::ALL {
                        ui.selectable_value(&mut dialog.hold_on, mode, mode.name());
                    }
                });
                if action(ui, "ff-speed-freeze", "Freeze Frame", false).clicked() {
                    close = freeze(app, dialog.hold_on).is_ok();
                }
                if action(ui, "ff-speed-unhold", "Remove Hold", false).clicked() {
                    close = clear_hold(app).is_ok();
                }
            });
        });
    if !close {
        app.speed = Some(dialog);
    }
}

/// The selected clip's name, for the dialog's header line.
fn clip_name(app: &App) -> String {
    app.sequence().and_then(|seq| app.selected_clip().and_then(|(clip, _)| seq.find_item(clip).map(|(_, item)| item.name.clone()))).unwrap_or_default()
}

/// A small clickable rect with an explicit id. egui's automatic button ids are
/// not stable to look up, and the headless UI tests click dialogs by id.
fn action(ui: &mut egui::Ui, id: &str, label: &str, accent: bool) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), ui::TEXT_HI).size().x + 20.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    let fill = if accent {
        if response.hovered() { ui::ACCENT } else { ui::ACCENT_DIM }
    } else if response.hovered() {
        ui::BG_HOVER
    } else {
        ui::BG_INSET
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, ui::TEXT_HI);
    response
}

#[cfg(test)]
mod tests {
    use super::{HoldOn, SpeedDialog, apply, clear_hold, freeze, open};
    use crate::app::App;
    use filmcraft_media::generators::GeneratorSource;
    use filmcraft_media::{Generator, MediaSource};
    use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};
    use serde_json::json;

    /// A fresh app with one sequence and a 10 s Bars & Tone pair placed at 0 on
    /// V1/A1. The picture's clip is selected, so the dialog starts from it.
    fn seeded() -> (App, filmcraft_project::ClipId) {
        let mut app = App::new();
        app.execute("file.newSequence", json!({"name": "Main", "width": 1280, "height": 720, "fps": 24, "video": 2, "audio": 2})).expect("create the sequence");
        let generator = Generator::BarsAndTone;
        let source = GeneratorSource::new(generator.clone(), 1280, 720, FrameRate::FPS_24, Tick(10 * TICKS_PER_SECOND));
        let info = source.info().clone();
        let name = info.name.clone();
        let item: ItemId = app
            .edit_project("Bars & Tone", |project, _| {
                Ok(project.add_item(
                    name.as_str(),
                    Label::Iris,
                    ItemKind::Media(MediaClip {
                        media: MediaRef::Generator(generator),
                        info,
                        interpret: Default::default(),
                        mark_in: None,
                        mark_out: None,
                        markers: Vec::new(),
                        offline: false,
                        proxy: None,
                        identity: None,
                    }),
                    None,
                ))
            })
            .expect("add the tone item");
        crate::edit::place_at_playhead(&mut app, item, false);
        let clip = app.sequence().expect("the sequence").video_tracks[0].items[0].id;
        app.selection = Some(clip);
        (app, clip)
    }

    fn span(app: &App) -> (Tick, Tick) {
        let seq = app.sequence().expect("the sequence");
        let item = &seq.video_tracks[0].items[0];
        (item.start, item.duration)
    }

    #[test]
    fn speed_dialog_doubles_the_rate_and_halves_the_duration_as_one_undo() {
        let (mut app, clip) = seeded();
        open(&mut app);
        let mut dialog = app.speed.clone().expect("the dialog opened for the selected clip");
        assert_eq!(dialog.percent, 100.0, "the dialog starts from the clip's own rate");

        dialog.percent = 200.0;
        apply(&mut app, &dialog).expect("the engine accepts 200%");

        let (_, item) = app.sequence().unwrap().find_item(clip).unwrap();
        assert_eq!(item.duration, Tick(5 * TICKS_PER_SECOND), "200% halves the timeline length");
        assert_eq!(item.speed, 2.0);
        assert_eq!(app.status, "Speed 200%");
        assert_eq!(app.sequence().unwrap().audio_tracks[0].items[0].duration, Tick(5 * TICKS_PER_SECOND), "the linked sound follows");

        app.undo();
        let (_, item) = app.sequence().unwrap().find_item(clip).unwrap();
        assert_eq!(item.duration, Tick(10 * TICKS_PER_SECOND), "one undo restores the length");
        assert_eq!(item.speed, 1.0);
    }

    #[test]
    fn speed_dialog_slows_down_reverses_and_ripples_the_clips_after_it() {
        let (mut app, first) = seeded();
        app.playhead = Tick(10 * TICKS_PER_SECOND);
        let item = app.sequence().unwrap().video_tracks[0].items[0].item;
        crate::edit::place_at_playhead(&mut app, item, false);
        app.playhead = Tick::ZERO;
        app.selection = Some(first);
        let second = app.sequence().unwrap().video_tracks[0].items[1].id;
        assert_eq!(app.sequence().unwrap().video_tracks[0].items[1].start, Tick(10 * TICKS_PER_SECOND), "the second pair starts at 10 s");

        open(&mut app);
        let mut dialog: SpeedDialog = app.speed.clone().expect("the dialog opened");
        dialog.percent = 50.0;
        dialog.reverse = true;
        dialog.ripple = true;
        apply(&mut app, &dialog).expect("the engine accepts 50% reversed with ripple");

        let seq = app.sequence().unwrap();
        let (_, one) = seq.find_item(first).unwrap();
        assert_eq!(one.duration, Tick(20 * TICKS_PER_SECOND), "50% doubles the length");
        assert!(one.reverse, "the picture plays backwards");
        assert_eq!(seq.find_item(second).unwrap().1.start, Tick(20 * TICKS_PER_SECOND), "ripple pushed the next pair past the longer clip");
        assert_eq!(app.status, "Speed 50% reversed");
    }

    #[test]
    fn freeze_frame_holds_a_source_frame_and_can_be_removed() {
        let (mut app, clip) = seeded();
        app.playhead = Tick(4 * TICKS_PER_SECOND);
        app.selection = Some(clip);

        freeze(&mut app, HoldOn::In).expect("a freeze at the in point");
        let (_, item) = app.sequence().unwrap().find_item(clip).unwrap();
        assert_eq!(item.frame_hold, Some(item.source_in), "the clip holds its first source frame");
        assert_eq!(item.duration, Tick(10 * TICKS_PER_SECOND), "a freeze keeps the timeline length");
        assert_eq!(app.status, "Freeze frame (In point)");

        app.undo();
        assert_eq!(app.sequence().unwrap().find_item(clip).unwrap().1.frame_hold, None, "one undo removes the hold");

        freeze(&mut app, HoldOn::Playhead).expect("a freeze at the playhead");
        let (_, item) = app.sequence().unwrap().find_item(clip).unwrap();
        assert_eq!(item.frame_hold, Some(item.source_in + Tick(4 * TICKS_PER_SECOND)), "the playhead's frame is held");

        clear_hold(&mut app).expect("the hold can be removed");
        assert_eq!(app.sequence().unwrap().find_item(clip).unwrap().1.frame_hold, None);
        assert_eq!(app.status, "Frame hold removed");
    }
}
