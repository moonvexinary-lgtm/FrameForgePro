//! The Export workspace: real renders through the editing engine's export
//! pipeline — the same compositor the program monitor shows — with progress
//! and a saved-file report. Only formats with built-in encoders are offered;
//! hardware-only formats are not faked.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use filmcraft_export::{ExportSettings, Format, Progress, export};
use filmcraft_project::ItemId;
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
    pub seq: ItemId,
    pub done: Arc<Mutex<Option<Result<String, String>>>>,
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
    if let Some(running) = &app.export.running {
        let fraction = running.progress.fraction();
        let rect = area.available_rect_before_wrap();
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, 14.0));
        area.painter().rect_filled(bar, 3.0, ui.visuals().selection.bg_fill);
        area.label(format!("Rendering… {:.0}%", fraction * 100.0));
        area.ctx().request_repaint();
        finished = running.done.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    if let Some(result) = finished {
        match result {
            Ok(report) => {
                app.export.result = Some(report);
                app.export.error = None;
                app.status = "Export finished".to_string();
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
    let mut path = app.export.path_text.trim().to_string();
    if path.is_empty() {
        return;
    }
    if !path.to_lowercase().ends_with(extension) {
        path = format!("{path}.{extension}");
        app.export.path_text = path;
    }

    let doc: FrameForgeProject = app.doc.doc.clone();
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
                .map_err(|e| e.to_string()),
            Err(e) => Err(format!("{e}")),
        };
        *worker_done.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });

    app.export.running = Some(RunningExport { progress, seq, done });
    app.export.result = None;
    app.export.error = None;
    app.status = format!("Exporting to {path}…");
}
