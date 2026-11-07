//! The Export workspace: real renders through the editing engine's export
//! pipeline — the same compositor the program monitor shows — with progress
//! and a saved-file report. Only formats with built-in encoders are offered;
//! hardware-only formats are not faked.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use filmcraft_export::{ExportError, ExportSettings, Format, Progress, export};
use filmcraft_time::{Tick, TimeRange};
use frameforge_bridge::{FrameForgeProject, ProjectSources};

use crate::app::App;

/// The formats the build can encode on its own (H.265 and AV1 need a
/// registered hardware encoder; they are not listed until that exists).
const FORMATS: &[(Format, &str, &str)] = &[
    (Format::H264, "H.264 (MP4)", "mp4"),
    (Format::ProRes, "Apple ProRes (QuickTime)", "mov"),
    (Format::DnxHr, "Avid DNxHR (QuickTime)", "mov"),
    (Format::Mjpeg, "Motion-JPEG (QuickTime)", "mov"),
    (Format::Wav, "Audio only (WAV)", "wav"),
];

/// A named combination of format and quality settings applied in one click.
/// After the user edits format, bitrate or audio by hand the row shows
/// "Custom" again, so it never claims settings it is not using.
struct Preset {
    name: &'static str,
    format: Format,
    bitrate_kbps: u32,
    include_audio: bool,
}

const PRESETS: &[Preset] = &[
    Preset { name: "H.264 · Web 1080p (20 Mbps)", format: Format::H264, bitrate_kbps: 20_000, include_audio: true },
    Preset { name: "H.264 · Small file (8 Mbps)", format: Format::H264, bitrate_kbps: 8_000, include_audio: true },
    Preset { name: "ProRes · Mastering", format: Format::ProRes, bitrate_kbps: 20_000, include_audio: true },
    Preset { name: "WAV · Audio only", format: Format::Wav, bitrate_kbps: 20_000, include_audio: true },
];

/// What the user asked to export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportRange {
    /// The whole sequence, whether or not In/Out marks exist.
    #[default]
    EntireSequence,
    /// The sequence's In/Out marks. The engine resolves "no marks" to the whole
    /// sequence, so this is never an empty export.
    InOut,
}

/// The engine's `ExportSettings.range`: an explicit whole-sequence range, or
/// `None` for "In/Out if set, else the whole sequence" (the engine's own
/// documented default).
fn range_setting(choice: ExportRange, duration: Tick) -> Option<TimeRange> {
    match choice {
        ExportRange::EntireSequence => Some(TimeRange::new(Tick::ZERO, duration)),
        ExportRange::InOut => None,
    }
}

/// Apply a preset's format, bitrate and audio choice. Returns false for an index
/// that does not exist so a stale selection cannot half-apply.
fn apply_preset(state: &mut ExportState, index: usize) -> bool {
    let Some(preset) = PRESETS.get(index) else { return false };
    let Some(format_index) = FORMATS.iter().position(|(format, _, _)| *format == preset.format) else { return false };
    state.format_index = format_index;
    state.bitrate_kbps = preset.bitrate_kbps;
    state.include_audio = preset.include_audio;
    state.preset_index = Some(index);
    true
}

pub struct RunningExport {
    pub progress: Arc<Progress>,
    pub done: Arc<Mutex<Option<Result<String, String>>>>,
    /// The final path, so a cancelled render can remove its partial file.
    pub path: PathBuf,
}

#[derive(Default)]
pub struct ExportState {
    pub format_index: usize,
    pub bitrate_kbps: u32,
    pub include_audio: bool,
    /// The preset row shows this index; `None` means the settings were edited
    /// by hand (or nothing has been applied yet).
    pub preset_index: Option<usize>,
    pub range: ExportRange,
    pub path: Option<PathBuf>,
    pub path_text: String,
    pub running: Option<RunningExport>,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Resolve the typed output path: trim it, add the selected format's
/// extension exactly once, and reject an empty path. The same function is
/// used by the overwrite prompt and the render, so the name the user confirms
/// is the name that is written.
fn output_path(text: &str, extension: &str) -> Option<String> {
    let path = text.trim();
    if path.is_empty() {
        return None;
    }
    if Path::new(path).extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension)) {
        Some(path.to_string())
    } else {
        Some(format!("{path}.{extension}"))
    }
}

/// The cancellation sentinel crosses the worker's string-only result channel.
const CANCELLED_BY_USER: &str = "Export cancelled by the user";

fn export_error_message(error: ExportError) -> String {
    match error {
        ExportError::Cancelled => CANCELLED_BY_USER.to_string(),
        other => other.to_string(),
    }
}

/// Ask before replacing a file the user already has. The export path was
/// resolved with [`output_path`] first, so this is the exact final name.
fn confirm_overwrite(path: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title("Replace existing file?")
        .set_description(format!("{path}\n\nalready exists. Replace it?"))
        .set_buttons(rfd::MessageButtons::YesNo)
        .set_level(rfd::MessageLevel::Warning)
        .show()
        == rfd::MessageDialogResult::Yes
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    ui.painter().rect_filled(body, 0.0, crate::ui::BG_APP);
    let mut area = crate::ui::panel(ui, body, "export");
    crate::ui::panel_header(&mut area, "Export");
    crate::ui::inset(&mut area);
    crate::ui::title_line(&mut area, "Deliver", "The same CPU reference renderer the program monitor shows", "");

    // Keep the controls in a readable column rather than stretching across a
    // wide window.
    let card_w = (body.width() - 48.0).clamp(320.0, 560.0);
    area.horizontal(|ui| {
        ui.add_space(((ui.available_width() - card_w) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(card_w);

            crate::ui::section(ui, "PRESET");
            let preset_label = app.export.preset_index.and_then(|index| PRESETS.get(index)).map(|preset| preset.name).unwrap_or("Custom");
            let mut chosen = app.export.preset_index;
            egui::ComboBox::from_id_salt("preset").selected_text(preset_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, "Custom");
                for (index, preset) in PRESETS.iter().enumerate() {
                    ui.selectable_value(&mut chosen, Some(index), preset.name);
                }
            });
            if chosen != app.export.preset_index {
                app.export.preset_index = chosen;
                if let Some(index) = chosen {
                    apply_preset(&mut app.export, index);
                }
            }

            crate::ui::section(ui, "FORMAT");
            let selected_label = FORMATS.get(app.export.format_index).map(|(_, label, _)| *label).unwrap_or("H.264 (MP4)");
            let previous_format = app.export.format_index;
            egui::ComboBox::from_id_salt("format").selected_text(selected_label).show_ui(ui, |ui| {
                for (index, (_, label, _)) in FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut app.export.format_index, index, *label);
                }
            });
            if app.export.format_index != previous_format {
                app.export.preset_index = None;
            }
            ui.label(crate::ui::hint("Formats needing a hardware encoder (H.265, AV1) are not offered until one is registered."));

            crate::ui::section(ui, "VIDEO");
            ui.horizontal(|ui| {
                ui.label(crate::ui::meta("Bitrate"));
                if ui.add(egui::DragValue::new(&mut app.export.bitrate_kbps).range(500..=100_000).suffix(" kbps")).changed() {
                    app.export.preset_index = None;
                }
            });
            if ui.checkbox(&mut app.export.include_audio, "Include audio").changed() {
                app.export.preset_index = None;
            }

            crate::ui::section(ui, "RANGE");
            let has_marks = app.sequence().is_some_and(|seq| seq.mark_in.is_some() || seq.mark_out.is_some());
            if !has_marks && app.export.range == ExportRange::InOut {
                app.export.range = ExportRange::EntireSequence;
            }
            ui.radio_value(&mut app.export.range, ExportRange::EntireSequence, "Entire sequence");
            ui.add_enabled(has_marks, egui::RadioButton::new(app.export.range == ExportRange::InOut, "In / Out marks"))
                .on_disabled_hover_text("Set an In and an Out on the timeline first");

            crate::ui::section(ui, "OUTPUT");
            ui.horizontal(|ui| {
                let mut text = app.export.path_text.clone();
                let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 90.0));
                if response.changed() {
                    app.export.path_text = text;
                }
                if ui.button("Browse…").clicked() {
                    let extension = FORMATS.get(app.export.format_index).map(|(_, _, e)| *e).unwrap_or("mp4");
                    if let Some(path) = rfd::FileDialog::new().add_filter("Output", &[extension]).set_file_name(format!("export.{extension}")).save_file() {
                        app.export.path_text = path.display().to_string();
                        app.export.path = Some(path);
                    }
                }
            });

            crate::ui::big_gap(ui);
            let can_export = !app.export.path_text.is_empty() && app.export.running.is_none() && app.sequence_id().is_some();
            if can_export {
                if crate::ui::accent_button(ui, "Start Export").clicked() {
                    start(app);
                }
            } else {
                ui.add_enabled(false, egui::Button::new(egui::RichText::new("Start Export").size(12.0)).min_size(egui::vec2(ui.available_width(), 24.0)));
            }

            // Poll the running export (draw first, then take the finished result).
            let mut finished = None;
            let mut finished_path = None;
            let mut cancel_clicked = false;
            if let Some(running) = &app.export.running {
                crate::ui::section(ui, "RENDER");
                let fraction = running.progress.fraction();
                let (track, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), egui::Sense::hover());
                ui.painter().rect_filled(track, 2.0, crate::ui::BG_INSET);
                let filled = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, track.height()));
                ui.painter().rect_filled(filled, 2.0, crate::ui::ACCENT);
                ui.horizontal(|ui| {
                    ui.label(crate::ui::hint(&format!("Rendering… {:.0}%", fraction * 100.0)));
                    if running.progress.cancel.load(Ordering::Relaxed) {
                        ui.label(crate::ui::hint("Cancelling…"));
                    } else if ui.button("Cancel").clicked() {
                        running.progress.cancel.store(true, Ordering::Relaxed);
                        cancel_clicked = true;
                    }
                });
                ui.ctx().request_repaint();
                finished_path = Some(running.path.clone());
                finished = running.done.lock().unwrap_or_else(|e| e.into_inner()).take();
            }
            if cancel_clicked {
                app.status = "Cancelling export…".to_string();
            }
            if let Some(result) = finished {
                match result {
                    Ok(report) => {
                        app.export.result = Some(report);
                        app.export.error = None;
                        app.status = "Export finished".to_string();
                    }
                    Err(e) if e == CANCELLED_BY_USER => {
                        // FilmCraft writes straight to the target; a cancelled render
                        // leaves a truncated file, so remove it (overwrite was already
                        // confirmed before the render started).
                        if let Some(path) = &finished_path {
                            let _ = std::fs::remove_file(path);
                        }
                        app.export.result = None;
                        app.export.error = None;
                        app.status = "Export cancelled; the partial file was removed".to_string();
                    }
                    Err(e) => {
                        app.export.error = Some(e);
                        app.status = "Export failed".to_string();
                    }
                }
                app.export.running = None;
            }

            if let Some(result) = &app.export.result {
                crate::ui::spaced_rule(ui);
                ui.label(egui::RichText::new("Done").size(11.0).strong().color(crate::ui::ACCENT_HI));
                ui.label(egui::RichText::new(result).size(10.5).color(crate::ui::TEXT_MID));
            }
            if let Some(error) = &app.export.error {
                crate::ui::spaced_rule(ui);
                ui.label(egui::RichText::new(error).size(10.5).color(crate::ui::DANGER));
            }

            crate::ui::footer_hint(ui, "The export runs the same CPU reference renderer the program monitor shows — one picture, no surprises.");
        });
    });
}

fn start(app: &mut App) {
    let Some(seq) = app.sequence_id() else { return };
    let Some(entry) = FORMATS.get(app.export.format_index) else { return };
    let extension = entry.2;
    let format = entry.0;
    let Some(path) = output_path(&app.export.path_text, extension) else {
        app.status = "Choose an output file first".to_string();
        return;
    };
    app.export.path_text = path.clone();
    if Path::new(path.as_str()).exists() && !confirm_overwrite(&path) {
        app.status = "Export cancelled: the existing file was kept".to_string();
        return;
    }

    let doc: FrameForgeProject = app.document();
    let range = range_setting(app.export.range, app.sequence().map(|seq| seq.duration()).unwrap_or_default());
    let settings = ExportSettings {
        format,
        path: path.clone(),
        range,
        include_audio: app.export.include_audio,
        bitrate_kbps: if app.export.bitrate_kbps == 0 { 20_000 } else { app.export.bitrate_kbps },
        ..ExportSettings::default()
    };
    let progress = Arc::<Progress>::default();
    let done: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));

    let worker_progress = Arc::clone(&progress);
    let worker_done = Arc::clone(&done);
    std::thread::spawn(move || {
        let result = match ProjectSources::build(&doc) {
            Ok(sources) => export(&Arc::new(doc.project.clone()), seq, &settings, &sources, &worker_progress)
                .map(|report| format!("{}\n{} frames, {:.2} s, {:.2} MB", report.path, report.frames, report.seconds, report.bytes as f64 / 1_000_000.0))
                .map_err(export_error_message),
            Err(e) => Err(format!("{e}")),
        };
        *worker_done.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });

    let running_path = PathBuf::from(path.clone());
    app.export.running = Some(RunningExport { progress, done, path: running_path });
    app.export.result = None;
    app.export.error = None;
    app.status = format!("Exporting to {path}…");
}

#[cfg(test)]
mod tests {
    use filmcraft_export::ExportError;
    use filmcraft_time::Tick;

    use super::{CANCELLED_BY_USER, ExportRange, ExportState, FORMATS, PRESETS, apply_preset, export_error_message, output_path, range_setting};

    #[test]
    fn output_path_adds_the_selected_extension_once() {
        assert_eq!(output_path("C:\\clips\\shot", "mp4").as_deref(), Some("C:\\clips\\shot.mp4"));
        assert_eq!(output_path("C:\\clips\\shot.MP4", "mp4").as_deref(), Some("C:\\clips\\shot.MP4"));
        assert_eq!(output_path("  C:\\clips\\shot  ", "mov").as_deref(), Some("C:\\clips\\shot.mov"));
        assert_eq!(output_path("C:\\clips\\shot.notmp4", "mp4").as_deref(), Some("C:\\clips\\shot.notmp4.mp4"));
        assert_eq!(output_path("   ", "mp4"), None);
    }

    #[test]
    fn a_cancelled_export_is_reported_as_a_cancellation() {
        assert_eq!(export_error_message(ExportError::Cancelled), CANCELLED_BY_USER);
    }

    #[test]
    fn every_preset_applies_an_offered_format_and_sets_all_three_fields() {
        let mut state = ExportState::default();
        for (index, preset) in PRESETS.iter().enumerate() {
            assert!(apply_preset(&mut state, index), "preset {index} applies");
            assert_eq!(FORMATS[state.format_index].0, preset.format, "preset {index} selects its own format");
            assert_eq!(state.bitrate_kbps, preset.bitrate_kbps);
            assert_eq!(state.include_audio, preset.include_audio);
            assert_eq!(state.preset_index, Some(index));
        }
    }

    #[test]
    fn an_unknown_preset_is_refused_without_changing_the_settings() {
        let mut state = ExportState { format_index: 2, bitrate_kbps: 12_000, preset_index: Some(1), ..ExportState::default() };

        assert!(!apply_preset(&mut state, PRESETS.len()), "an index past the table is refused");

        assert_eq!(state.format_index, 2);
        assert_eq!(state.bitrate_kbps, 12_000);
        assert_eq!(state.preset_index, Some(1));
    }

    #[test]
    fn a_whole_sequence_range_is_explicit_and_in_out_defers_to_the_marks() {
        let duration = Tick(48_000);
        let whole = range_setting(ExportRange::EntireSequence, duration).expect("the whole sequence is an explicit range");
        assert_eq!(whole.start, Tick::ZERO);
        assert_eq!(whole.end(), duration);

        assert_eq!(range_setting(ExportRange::InOut, duration), None, "In/Out is the engine's own resolution");
    }
}
