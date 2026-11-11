//! Transition browser (T24).
//!
//! The catalog is the engine's own registry: `effects.list` with
//! `kind: "VideoTransition"` / `"AudioTransition"` returns every transition the
//! renderer can apply, with the folder the Effects panel groups it under. The
//! browser applies the chosen effect at the selected edit through the engine's
//! apply command, names the transition at that edit for selection / removal,
//! and keeps the length and alignment on editor state so the next apply uses
//! them without a dialog round trip.

use filmcraft_project::{ClipId, TransitionAlign};

use crate::app::App;
use crate::ui;

/// One row of the engine's transition catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    /// The engine's own folder path, e.g. `Video Transitions/Wipe`.
    pub path: String,
}

impl CatalogEntry {
    /// The leaf folder, as a browser heading shows it (`Wipe`).
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or(self.path.as_str(), |(_, folder)| folder)
    }
}

/// The transition already sitting at the selected clip's In edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedTransition {
    pub id: u64,
    pub name: String,
    pub frames: i64,
    pub align: TransitionAlign,
}

/// The open Transition browser. Editor state: not saved with the project and
/// not part of the undo history.
#[derive(Clone, Debug)]
pub struct TransitionBrowser {
    /// Show video transitions (true) or audio transitions (false).
    pub video: bool,
    pub filter: String,
    pub catalog: Vec<CatalogEntry>,
    /// The effect Apply would use, once a row is picked.
    pub effect: Option<String>,
    /// Why the catalog could not be read, when it could not.
    pub error: Option<String>,
}

impl TransitionBrowser {
    fn new(video: bool) -> Self {
        Self { video, filter: String::new(), catalog: Vec::new(), effect: None, error: None }
    }
}

/// Open the browser for the selected clip's kind (video when nothing is
/// selected), loading the engine's transition catalog.
pub fn open(app: &mut App) {
    let video = selected_is_video(app).unwrap_or(true);
    let mut browser = TransitionBrowser::new(video);
    refresh(app, &mut browser);
    app.transitions = Some(browser);
}

/// Load (or reload) the catalog for the browser's current kind. A query: it
/// does not change the document, the history or the dirty flag.
pub fn refresh(app: &mut App, browser: &mut TransitionBrowser) {
    let kind = if browser.video { "VideoTransition" } else { "AudioTransition" };
    match app.session.execute("effects.list", serde_json::json!({"kind": kind})) {
        Ok(value) => {
            browser.catalog = value
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|e| {
                            Some(CatalogEntry {
                                id: e.get("id")?.as_str()?.to_string(),
                                name: e.get("name")?.as_str()?.to_string(),
                                path: e.get("path").and_then(|p| p.as_str()).unwrap_or_default().to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            browser.error = None;
        }
        Err(error) => {
            browser.catalog.clear();
            browser.error = Some(error.to_string());
        }
    }
    // The chosen row only survives a reload that still lists it: switching to
    // the audio tab must not leave a video effect armed for Apply.
    if browser.effect.as_ref().is_none_or(|id| !browser.catalog.iter().any(|e| &e.id == id)) {
        browser.effect = None;
    }
}

/// Switch between the video and audio catalogs.
pub fn set_kind(app: &mut App, browser: &mut TransitionBrowser, video: bool) {
    if browser.video != video {
        browser.video = video;
        browser.filter.clear();
        refresh(app, browser);
    }
}

/// The rows the filter leaves, in the engine's registry order.
pub fn visible(browser: &TransitionBrowser) -> Vec<CatalogEntry> {
    let needle = browser.filter.trim().to_lowercase();
    browser
        .catalog
        .iter()
        .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle) || e.path.to_lowercase().contains(&needle))
        .cloned()
        .collect()
}

/// Apply the chosen transition at the selected edit: the engine's apply
/// command (effect, length, at the clip's In edge), then the alignment through
/// `sequence.setTransition` because apply only takes a length.
pub fn apply(app: &mut App, browser: &TransitionBrowser) -> Result<(), String> {
    let Some((clip, _)) = app.selected_clip() else {
        return Err("Select a clip to apply a transition".to_string());
    };
    let Some(selected_video) = clip_is_video(app, clip) else {
        return Err("The selected clip is no longer in the sequence".to_string());
    };
    let target = if selected_video == browser.video { Some(clip) } else { linked_half(app, clip, browser.video) };
    let Some(target) = target else {
        return Err(if browser.video {
            "Select a video clip (or its linked sound) for a video transition".to_string()
        } else {
            "Select an audio clip (or its linked picture) for an audio transition".to_string()
        });
    };
    let Some(effect) = browser.effect.clone() else {
        return Err(format!("Choose a {} transition from the catalog first", if browser.video { "video" } else { "audio" }));
    };
    let name = effect_name(browser, &effect);
    let frames = app.transition_frames.clamp(1, 600);
    let command = if browser.video { "sequence.applyVideoTransition" } else { "sequence.applyAudioTransition" };
    let value = app.execute(command, serde_json::json!({"clip": target.0, "edge": "in", "frames": frames, "effect": effect}))?;
    let align = align_id(app.transition_align);
    if align != "center" {
        // Apply only takes a length; the alignment is one more engine edit.
        let id = value["transition"].as_u64().unwrap_or(0);
        if let Err(error) = app.execute("sequence.setTransition", serde_json::json!({"transition": id, "align": align})) {
            app.status = format!("{name} applied, but not aligned: {error}");
            return Err(error);
        }
    }
    app.status = format!("{name} at the selected edit ({frames} frames, {align} at the cut)");
    Ok(())
}

/// The transition at the selected clip's In edge, for the browser's heading
/// and the Select / Remove buttons.
pub fn at_selection(app: &App, video: bool) -> Option<AppliedTransition> {
    let (clip, _) = app.selected_clip()?;
    let q = app.sequence()?;
    let tracks = if video { &q.video_tracks } else { &q.audio_tracks };
    let target = if tracks.iter().any(|t| t.items.iter().any(|i| i.id == clip)) { clip } else { linked_half(app, clip, video)? };
    let rate = app.sequence_rate();
    tracks.iter().flat_map(|track| &track.transitions).find(|x| x.to == Some(target)).map(|x| AppliedTransition {
        id: x.id.0,
        name: x.effect.def().map_or_else(|| x.effect.effect.clone(), |d| d.name.to_string()),
        frames: rate.frame_at(x.duration).max(1),
        align: x.align,
    })
}

/// Make the transition at the selected edit the engine's selection, so
/// Clear Transition and the transition commands act on it.
pub fn select(app: &mut App, video: bool) -> Result<(), String> {
    let Some((clip, _)) = app.selected_clip() else {
        return Err("Select the incoming clip of a transition".to_string());
    };
    let Some(current) = at_selection(app, video) else {
        return Err("No transition at the selected clip's In edge".to_string());
    };
    app.session.execute("timeline.select", serde_json::json!({"clips": [clip.0], "transitions": [current.id]})).map_err(|error| error.to_string())?;
    app.status = format!("Selected {} (transition {})", current.name, current.id);
    Ok(())
}

/// Remove the transition at the selected clip's In edge (one engine edit).
pub fn remove(app: &mut App, video: bool) -> Result<(), String> {
    let Some(current) = at_selection(app, video) else {
        return Err("No transition at the selected clip's In edge".to_string());
    };
    app.execute("sequence.removeTransition", serde_json::json!({"transitions": [current.id]}))?;
    app.status = format!("Removed {}", current.name);
    Ok(())
}

/// The engine's `align` value for a transition alignment.
fn align_id(align: TransitionAlign) -> &'static str {
    match align {
        TransitionAlign::CenterAtCut => "center",
        TransitionAlign::StartAtCut => "start",
        TransitionAlign::EndAtCut => "end",
    }
}

/// Whether a clip is on a video track (true) or an audio track (false).
fn clip_is_video(app: &App, clip: ClipId) -> Option<bool> {
    let q = app.sequence()?;
    if q.video_tracks.iter().any(|t| t.items.iter().any(|i| i.id == clip)) {
        Some(true)
    } else if q.audio_tracks.iter().any(|t| t.items.iter().any(|i| i.id == clip)) {
        Some(false)
    } else {
        None
    }
}

fn selected_is_video(app: &App) -> Option<bool> {
    let (clip, _) = app.selected_clip()?;
    clip_is_video(app, clip)
}

/// The other half of the selected clip's linked pair on the wanted kind, so a
/// video transition applies to the picture when its linked sound is selected
/// (and the other way round), as the Dissolve shortcut already does.
fn linked_half(app: &App, clip: ClipId, video: bool) -> Option<ClipId> {
    let q = app.sequence()?;
    let link = q.find_item(clip)?.1.link?;
    let tracks = if video { &q.video_tracks } else { &q.audio_tracks };
    tracks.iter().flat_map(|t| &t.items).find(|i| i.id != clip && i.link == Some(link)).map(|i| i.id)
}

/// The name the catalog gives an effect id, or the id itself.
fn effect_name(browser: &TransitionBrowser, id: &str) -> String {
    browser.catalog.iter().find(|e| e.id == id).map_or_else(|| id.to_string(), |e| e.name.clone())
}

/// Draw the browser when one is open. Called every frame from `App::ui_frame`,
/// so it floats over the timeline whose edit it changes.
pub fn show(app: &mut App, ui: &egui::Ui) {
    let Some(mut browser) = app.transitions.take() else { return };
    let mut close = false;
    egui::Window::new("Transitions")
        .id(egui::Id::new("ff-transition-window"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 84.0))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            // Pin the width: an auto-sized anchored window would otherwise take
            // the whole screen on its first layout pass.
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                let label = if browser.video { "Video transitions" } else { "Audio transitions" };
                ui.label(egui::RichText::new(label).size(11.0).strong().color(ui::TEXT_HI));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if action(ui, "ff-transition-close", "Close", false).clicked() {
                        close = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                if action(ui, "ff-transition-video", "Video", browser.video).clicked() {
                    set_kind(app, &mut browser, true);
                }
                if action(ui, "ff-transition-audio", "Audio", !browser.video).clicked() {
                    set_kind(app, &mut browser, false);
                }
                ui.add(egui::TextEdit::singleline(&mut browser.filter).desired_width(150.0).hint_text("Filter transitions"));
            });
            if let Some(error) = browser.error.clone() {
                ui.label(egui::RichText::new(error).size(10.0).color(ui::TEXT_LO));
            }
            let rows = visible(&browser);
            if rows.is_empty() {
                ui.label(ui::hint("No transition matches this filter."));
            } else {
                egui::ScrollArea::vertical().id_salt("transition-catalog").max_height(260.0).auto_shrink([false, false]).show(ui, |ui| {
                    for entry in &rows {
                        let width = ui.available_width();
                        let (row, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
                        let selected = browser.effect.as_deref() == Some(entry.id.as_str());
                        let hit = ui.interact(row, egui::Id::new(format!("ff-transition-effect-{}", entry.id)), egui::Sense::click());
                        let fill = if hit.hovered() {
                            ui::BG_HOVER
                        } else if selected {
                            ui::ACCENT_SEL
                        } else {
                            ui::BG_INSET
                        };
                        ui.painter().rect_filled(row, 3.0, fill);
                        if selected {
                            ui::selected_bar(ui.painter(), row);
                        }
                        ui.painter().text(
                            egui::pos2(row.left() + 8.0, row.center().y),
                            egui::Align2::LEFT_CENTER,
                            &entry.name,
                            egui::FontId::proportional(11.0),
                            ui::TEXT_HI,
                        );
                        ui.painter().text(
                            egui::pos2(row.right() - 6.0, row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            entry.folder(),
                            egui::FontId::proportional(9.5),
                            ui::TEXT_OFF,
                        );
                        if hit.clicked() {
                            browser.effect = Some(entry.id.clone());
                        }
                    }
                });
            }

            ui.separator();
            let applied = at_selection(app, browser.video);
            match &applied {
                Some(current) => {
                    ui.label(ui::hint(&format!("At the selected edit: {} - {} frames, {} at the cut", current.name, current.frames, align_id(current.align))));
                }
                None => {
                    ui.label(ui::hint("No transition at the selected clip's In edge yet."));
                }
            }
            ui.horizontal(|ui| {
                if action(ui, "ff-transition-select", "Select", false).clicked() && applied.is_some() {
                    let _ = select(app, browser.video);
                }
                if action(ui, "ff-transition-remove", "Remove", false).clicked() && applied.is_some() {
                    let _ = remove(app, browser.video);
                }
                ui.label(egui::RichText::new("Length").size(10.5).color(ui::TEXT_LO));
                ui.add(egui::DragValue::new(&mut app.transition_frames).range(1..=600).suffix(" fr"));
                let align_label = match app.transition_align {
                    TransitionAlign::CenterAtCut => "Center",
                    TransitionAlign::StartAtCut => "Start",
                    TransitionAlign::EndAtCut => "End",
                };
                egui::ComboBox::from_id_salt("ff-transition-align").selected_text(align_label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.transition_align, TransitionAlign::CenterAtCut, "Center at cut");
                    ui.selectable_value(&mut app.transition_align, TransitionAlign::StartAtCut, "Start at cut");
                    ui.selectable_value(&mut app.transition_align, TransitionAlign::EndAtCut, "End at cut");
                });
            });
            ui.horizontal(|ui| {
                let chosen = browser.effect.as_ref().map_or_else(|| "Choose a transition".to_string(), |id| effect_name(&browser, id));
                ui.label(ui::hint(&chosen));
                if action(ui, "ff-transition-apply", "Apply", true).clicked()
                    && let Err(error) = apply(app, &browser)
                {
                    app.status = error;
                }
            });
        });
    if !close {
        app.transitions = Some(browser);
    }
}

/// A small clickable rect with an explicit id, so the headless UI tests can
/// click the window's controls: egui's auto ids are not addressable.
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
    let color = if accent { ui::TEXT_HI } else { ui::TEXT_MID };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
    response
}

#[cfg(test)]
mod tests {
    use super::{apply, at_selection, open, remove, select, set_kind, visible};
    use crate::app::App;
    use filmcraft_project::{ClipId, ItemId, TransitionAlign};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};
    use serde_json::json;

    /// Place exactly `src_in..src_out` of `item` at timeline `at` through the
    /// Source monitor, leaving the media's unused head/tail as handles.
    fn place_source_range(app: &mut App, item: ItemId, at: Tick, src_in: Tick, src_out: Tick) {
        app.playhead = at;
        crate::edit::open_source(app, item);
        crate::edit::source_set_playhead(app, src_in);
        crate::edit::source_mark(app, true);
        crate::edit::source_set_playhead(app, src_out - FrameRate::FPS_24.frame_duration());
        crate::edit::source_mark(app, false);
        crate::edit::source_edit(app, false);
    }

    /// Two adjacent 2 s clips with handles on both sides: the color-matte pair
    /// for video transitions, the Bars & Tone pair (picture and sound) for the
    /// audio variants.
    fn two_clip_app(video: bool) -> (App, Vec<ClipId>) {
        let mut s = filmcraft_engine::Session::default();
        s.execute("file.newProject", json!({"name": "Transitions"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let new = |s: &mut filmcraft_engine::Session, color: Option<&str>| {
            let v = match color {
                Some(color) => s.execute("file.newColorMatte", json!({"color": color, "seconds": 4.0})).unwrap(),
                None => s.execute("file.newBarsAndTone", json!({"seconds": 4.0})).unwrap(),
            };
            ItemId(v["item"].as_u64().unwrap())
        };
        let a = new(&mut s, video.then_some("#ff0000"));
        let b = new(&mut s, video.then_some("#0000ff"));
        let mut app = App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        place_source_range(&mut app, a, Tick::ZERO, Tick::ZERO, two_seconds);
        place_source_range(&mut app, b, two_seconds, Tick(TICKS_PER_SECOND), Tick(3 * TICKS_PER_SECOND));
        let clips = app.session.active_sequence().unwrap().video_tracks[0].items.iter().map(|i| i.id).collect();
        (app, clips)
    }

    #[test]
    fn the_catalog_is_the_engines_own_transition_registry() {
        let (mut app, _) = two_clip_app(true);
        open(&mut app);
        let browser = app.transitions.clone().expect("the browser opens");
        let wipe = browser.catalog.iter().find(|e| e.id == "linear_wipe").expect("the engine lists Linear Wipe");
        assert_eq!(wipe.path, "Video Transitions/Wipe", "the browser shows the engine's folder");
        assert!(browser.catalog.iter().any(|e| e.id == "cross_dissolve"), "the Dissolve folder is there");
        assert!(!browser.catalog.iter().any(|e| e.id == "constant_power"), "the video tab lists video transitions only");

        let mut audio = browser.clone();
        set_kind(&mut app, &mut audio, false);
        assert!(audio.catalog.iter().any(|e| e.id == "constant_power"), "the audio tab lists Constant Power");
        assert!(audio.catalog.iter().all(|e| e.path.starts_with("Audio Transitions")), "audio rows come from the audio registry");

        audio.filter = "gain".to_string();
        let rows = visible(&audio);
        assert_eq!(rows.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["constant_gain"], "the filter narrows by name");
    }

    #[test]
    fn applying_a_wipe_uses_the_chosen_effect_alignment_and_length() {
        let (mut app, clips) = two_clip_app(true);
        let cut = app.session.active_sequence().unwrap().video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap().start;
        app.selection = Some(clips[1]);
        open(&mut app);
        let mut browser = app.transitions.clone().expect("the browser opens");
        browser.effect = Some("linear_wipe".to_string());
        app.transition_frames = 10;
        app.transition_align = TransitionAlign::StartAtCut;

        apply(&mut app, &browser).expect("the browser applies the chosen transition");

        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.video_tracks[0].transitions.len(), 1, "one transition at the edit");
        let tr = &q.video_tracks[0].transitions[0];
        assert_eq!(tr.effect.effect, "linear_wipe", "the chosen effect, not the default dissolve");
        assert_eq!(tr.duration, FrameRate::FPS_24.tick_of(10), "the browser's length");
        assert_eq!(tr.align, TransitionAlign::StartAtCut, "the browser's alignment");
        assert_eq!(tr.start, cut, "Start at Cut keeps the transition's start on the cut");
        assert!(app.status.contains("Linear Wipe") && app.status.contains("10 frames"), "status says what landed: {}", app.status);
    }

    #[test]
    fn an_audio_transition_variant_applies_to_the_linked_sound() {
        let (mut app, clips) = two_clip_app(false);
        app.selection = Some(clips[1]);
        open(&mut app);
        let mut browser = app.transitions.clone().expect("the browser opens");
        set_kind(&mut app, &mut browser, false);
        browser.effect = Some("constant_gain".to_string());
        app.transition_frames = 8;

        apply(&mut app, &browser).expect("the audio variant applies");

        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.audio_tracks[0].transitions.len(), 1, "the linked sound takes the transition");
        assert_eq!(q.audio_tracks[0].transitions[0].effect.effect, "constant_gain", "the chosen variant, not the default crossfade");
        assert!(q.video_tracks[0].transitions.is_empty(), "the picture is left alone");
    }

    #[test]
    fn the_browser_selects_and_removes_the_transition_at_the_selected_edit() {
        let (mut app, clips) = two_clip_app(true);
        app.selection = Some(clips[1]);
        open(&mut app);
        let mut browser = app.transitions.clone().expect("the browser opens");
        browser.effect = Some("cross_dissolve".to_string());
        apply(&mut app, &browser).expect("apply");

        let current = at_selection(&app, true).expect("the incoming clip's transition");
        assert_eq!(current.name, "Cross Dissolve");
        select(&mut app, true).expect("the transition becomes the engine's selection");
        let selected = app.session.state.transition_selection.clone();
        assert_eq!(selected.len(), 1, "one transition selected");
        assert_eq!(selected[0].0, current.id);

        remove(&mut app, true).expect("the selected transition is removed");
        assert!(app.session.active_sequence().unwrap().video_tracks[0].transitions.is_empty());
        app.undo();
        assert_eq!(app.session.active_sequence().unwrap().video_tracks[0].transitions.len(), 1, "one undo restores it");
    }
}
