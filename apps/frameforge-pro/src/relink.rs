//! The relink dialog: reconnecting media whose file moved or went offline.
//!
//! The editing engine owns all the work that can be wrong — the offline scan,
//! the candidate check (name, extension, fingerprint, duration), the folder
//! remap and the undoable edit — behind `media.findMissing` and `media.relink`.
//! This module owns only what a person sees and clicks, so a link made here is
//! the same link an agent would make through the command, with the same undo
//! step. Rows are addressed by stable ids so the UI tests can click them the
//! way the timeline tests already click the track headers.

use filmcraft_project::ItemId;

use crate::app::App;
use crate::ui;

/// One item whose file the last scan found missing, offline or proxy-missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingRow {
    pub item: ItemId,
    /// The project item's name.
    pub name: String,
    /// The file name the project recorded.
    pub file_name: String,
    /// The full path the project recorded (the file that is gone).
    pub path: String,
    /// `missing`, `offline` or `proxyMissing`, straight from the engine.
    pub status: String,
}

impl MissingRow {
    /// Parse one row of the engine's `media.findMissing` / `media.linkMedia`
    /// result. Unknown shapes are skipped rather than guessed at.
    pub(crate) fn from_json(value: &serde_json::Value) -> Option<MissingRow> {
        Some(MissingRow {
            item: ItemId(value.get("item")?.as_u64()?),
            name: value.get("name")?.as_str()?.to_string(),
            file_name: value.get("fileName")?.as_str()?.to_string(),
            path: value.get("path")?.as_str()?.to_string(),
            status: value.get("status")?.as_str()?.to_string(),
        })
    }

    fn status_color(&self) -> egui::Color32 {
        match self.status.as_str() {
            "missing" => ui::DANGER,
            _ => ui::WARN,
        }
    }
}

/// The floating Link Media dialog.
#[derive(Default)]
pub struct RelinkDialog {
    pub rows: Vec<MissingRow>,
    /// The index of the row the buttons act on.
    pub selected: Option<usize>,
    /// The file path typed or pasted for the selected row.
    pub path: String,
    /// Link even when the candidate fails the match checks.
    pub force: bool,
    /// The last confirmation or refusal, shown under the list.
    pub message: Option<String>,
}

impl RelinkDialog {
    /// Open over the app's current missing-media list.
    pub fn open(app: &App) -> RelinkDialog {
        let mut dialog = RelinkDialog { rows: app.offline_rows.clone(), ..RelinkDialog::default() };
        if !dialog.rows.is_empty() {
            dialog.select(0);
        }
        dialog
    }

    /// Select the row at `index` and seed the path field with its recorded
    /// location, so the field is never empty once a row is chosen.
    pub fn select(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index) {
            self.selected = Some(index);
            self.path = row.path.clone();
            self.message = None;
        }
    }

    /// The row the buttons act on.
    pub fn selected_row(&self) -> Option<&MissingRow> {
        self.selected.and_then(|index| self.rows.get(index))
    }

    /// Link the selected row to `self.path` through the engine's `media.relink`
    /// (undoable). The list is then rescanned so the dialog and the badge never
    /// disagree with the document.
    pub fn link(&mut self, app: &mut App, force: bool) {
        let Some(row) = self.selected_row().cloned() else {
            self.message = Some("Choose a missing file first".to_string());
            return;
        };
        let path = self.path.trim().to_string();
        if path.is_empty() {
            self.message = Some("Type or choose the file to link".to_string());
            return;
        }
        let params = serde_json::json!({"item": row.item.0, "path": path, "force": force});
        match app.execute("media.relink", params) {
            Ok(_) => {
                self.message = Some(format!("Linked {}", row.file_name));
                self.rescan(app);
            }
            Err(error) => self.message = Some(error),
        }
    }

    /// Re-read the app's missing list after the document changed. The message
    /// is left alone so a link that fixed one of several rows still reports it.
    fn rescan(&mut self, app: &mut App) {
        app.refresh_offline();
        self.rows = app.offline_rows.clone();
        let first = self.rows.first().map(|row| row.path.clone());
        match first {
            Some(path) => {
                self.selected = Some(0);
                self.path = path;
            }
            None => {
                self.selected = None;
                self.path.clear();
            }
        }
    }
}

/// Draw the dialog when one is open. Called every frame from `App::ui_frame`,
/// so the window floats over whatever workspace is showing.
pub fn show(app: &mut App, ui: &egui::Ui) {
    let Some(mut dialog) = app.relink.take() else { return };
    let mut closed = false;
    egui::Window::new("Relink Media")
        .id(egui::Id::new("ff-relink-window"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            closed = dialog_ui(&mut dialog, app, ui);
        });
    if !closed {
        app.relink = Some(dialog);
    }
}

/// The dialog's body. Returns true when the user asked to close it.
fn dialog_ui(dialog: &mut RelinkDialog, app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut closed = false;
    ui.set_min_width(470.0);
    ui.set_max_width(560.0);
    ui.label(ui::hint("These files moved or are offline. Pick one, point it at the file, and Link."));

    // A click is recorded here and applied after the loop: `dialog.select`
    // needs `&mut dialog` and the rows are borrowed by the loop itself.
    let mut clicked_row = None;
    let selected = dialog.selected;
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        if dialog.rows.is_empty() {
            ui.label(ui::hint("Nothing is missing: every file the project names is where it should be."));
        }
        for (index, row) in dialog.rows.iter().enumerate() {
            let is_selected = selected == Some(index);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
            let response =
                ui.interact(rect, egui::Id::new(("ff-relink-row", index)), egui::Sense::click()).on_hover_text(format!("{}\n{}", row.name, row.path));
            if response.hovered() {
                ui.painter().rect_filled(rect, 3.0, ui::BG_HOVER);
            } else if is_selected {
                ui.painter().rect_filled(rect, 3.0, ui::ACCENT_SEL);
            }
            if is_selected {
                ui::selected_bar(ui.painter(), rect);
            }
            let clipped = ui.painter_at(egui::Rect::from_min_max(egui::pos2(rect.left() + 10.0, rect.top()), egui::pos2(rect.right() - 78.0, rect.bottom())));
            clipped.text(
                egui::pos2(rect.left() + 10.0, rect.center().y - 6.0),
                egui::Align2::LEFT_CENTER,
                &row.file_name,
                egui::FontId::proportional(11.5),
                ui::TEXT_HI,
            );
            clipped.text(
                egui::pos2(rect.left() + 10.0, rect.center().y + 7.0),
                egui::Align2::LEFT_CENTER,
                &row.path,
                egui::FontId::monospace(9.0),
                ui::TEXT_LO,
            );
            ui.painter().text(
                egui::pos2(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &row.status,
                egui::FontId::monospace(9.5),
                row.status_color(),
            );
            if response.clicked() {
                clicked_row = Some(index);
            }
        }
    });
    if let Some(index) = clicked_row {
        dialog.select(index);
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(ui::meta("File"));
        ui.add(egui::TextEdit::singleline(&mut dialog.path).id_salt("ff-relink-path").desired_width((ui.available_width() - 96.0).max(120.0)));
        if chip(ui, "ff-relink-browse", "Browse…", true)
            && let Some(path) = rfd::FileDialog::new().pick_file()
        {
            dialog.path = path.display().to_string();
        }
    });
    ui.horizontal(|ui| {
        if chip(ui, "ff-relink-link", "Link selected", dialog.selected.is_some()) {
            dialog.link(app, dialog.force);
        }
        ui.checkbox(&mut dialog.force, "Force").on_hover_text("Link even when the file name, size or duration does not match the clip");
        ui.label(ui::hint("Link also searches the chosen file's folder for the others"));
    });

    if let Some(message) = &dialog.message {
        ui.add_space(4.0);
        let color = if message.starts_with("Linked") { ui::ACCENT_HI } else { ui::DANGER };
        ui.label(egui::RichText::new(message).size(10.5).color(color));
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(ui::hint(&format!("{} file(s) still missing or offline", dialog.rows.len())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if chip(ui, "ff-relink-close", "Close", true) {
                closed = true;
            }
        });
    });
    closed
}

/// A flat painted button with a stable id, so a scripted click finds it the
/// same way a person does (the timeline registers its controls the same way).
fn chip(ui: &mut egui::Ui, id: &'static str, label: &str, enabled: bool) -> bool {
    let width = 22.0 + label.chars().count() as f32 * 6.5;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let response = ui.interact(rect, egui::Id::new(id), sense);
    let hovered = enabled && response.hovered();
    let (fill, text) = if !enabled {
        (ui::BG_INSET, ui::TEXT_OFF)
    } else if hovered {
        (ui::ACCENT, ui::BG_APP)
    } else {
        (ui::BG_CONTROL, ui::TEXT_HI)
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.0), text);
    enabled && response.clicked()
}
