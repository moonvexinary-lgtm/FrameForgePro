//! The Export workspace: real renders through the editing engine's export
//! pipeline — the same compositor the program monitor shows — with progress
//! and a saved-file report. Only formats with built-in encoders are offered;
//! hardware-only formats are not faked.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use filmcraft_export::{ExportError, ExportSettings, Format, Progress, export};
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
    ui.painter().rect_filled(body, 0.0, ui.visuals().panel_fill);
    let margin = 40.0;
    let inner = egui::Rect::from_min_max(body.min + egui::vec2(margin, margin), body.max - egui::vec2(margin, margin));
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt("export"));
    area.heading("Export");
    area.separator();

    // Format: only what this build can really encode.
    let selected_label = FORMATS.get(app.export.format_index).map(|(_, label, _)| *label).unwrap_or("H.264 (MP4)");
    egui::ComboBox::from_id_salt("format").selected_text(selected_label).show_ui(&mut area, |ui| {
        for (index, (_, label, _)) in FORMATS.iter().enumerate() {
            ui.selectable_value(&mut app.export.format_index, index, *label);
        }
    });
    area.label("Formats needing a hardware encoder (H.265, AV1) are not offered until one is registered.");

    area.separator();
    area.horizontal(|ui| {
        ui.label("Video bitrate");
        ui.add(egui::DragValue::new(&mut app.export.bitrate_kbps).range(500..=100_000).suffix(" kbps"));
    });
    area.checkbox(&mut app.export.include_audio, "Include audio");

    area.separator();
    area.horizontal(|ui| {
        ui.label("Output file");
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

    area.separator();
    let can_export = !app.export.path_text.is_empty() && app.export.running.is_none() && app.sequence_id().is_some();
    if area.add_enabled(can_export, egui::Button::new("Export")).clicked() {
        start(app);
    }

    // Poll the running export (draw first, then take the finished result).
    let mut finished = None;
    let mut finished_path = None;
    let mut cancel_clicked = false;
    if let Some(running) = &app.export.running {
        let fraction = running.progress.fraction();
        let rect = area.available_rect_before_wrap();
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, 14.0));
        area.painter().rect_filled(bar, 3.0, ui.visuals().selection.bg_fill);
        area.horizontal(|ui| {
            ui.label(format!("Rendering… {:.0}%", fraction * 100.0));
            if running.progress.cancel.load(Ordering::Relaxed) {
                ui.label("Cancelling…");
            } else if ui.button("Cancel").clicked() {
                running.progress.cancel.store(true, Ordering::Relaxed);
                cancel_clicked = true;
            }
        });
        area.ctx().request_repaint();
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
        area.separator();
        area.label(egui::RichText::new("Done").strong());
        area.label(result);
    }
    if let Some(error) = &app.export.error {
        area.separator();
        area.colored_label(egui::Color32::LIGHT_RED, error);
    }

    area.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.colored_label(egui::Color32::GRAY, "The export runs the same CPU reference renderer the program monitor shows — one picture, no surprises.");
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
    let settings = ExportSettings {
        format,
        path: path.clone(),
        range: None,
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

    use super::{CANCELLED_BY_USER, export_error_message, output_path};

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
}
