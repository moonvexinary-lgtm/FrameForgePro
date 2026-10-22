//! The Edit workspace: the project panel (media items), the program monitor
//! (real frames through the engine) and the timeline (real clips, real
//! playhead, real engine commands). Laid out by rect — this egui generation
//! has no panel helpers.

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef};
use filmcraft_time::TICKS_PER_SECOND;
use filmcraft_time::Tick;

use crate::app::{App, Monitor, SEQUENCE_H, SEQUENCE_RATE, SEQUENCE_W, TimelineDrag};
use crate::pixels;

const ROW_H: f32 = 34.0;
const RULER_H: f32 = 18.0;
const GENERATOR_SECONDS: f64 = 10.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    let project_w = app.project_w.clamp(180.0, body.width() - 260.0);
    let timeline_h = app.timeline_h.clamp(120.0, body.height() - 150.0);

    let project = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + project_w, body.max.y));
    let rest = egui::Rect::from_min_max(egui::pos2(body.min.x + project_w, body.min.y), body.max);
    let timeline = egui::Rect::from_min_max(egui::pos2(rest.min.x, rest.max.y - timeline_h), rest.max);
    let monitor = egui::Rect::from_min_max(rest.min, egui::pos2(rest.max.x, timeline.min.y));

    project_panel(app, ui, project);
    crate::app::gutter_v(ui, body.min.x + project_w, body.min.y, body.max.y, &mut app.project_w);
    monitor_panel(app, ui, monitor);
    crate::app::gutter_h(ui, monitor.max.y, rest.min.x, rest.max.x, &mut app.timeline_h);
    timeline_panel(app, ui, timeline);
}

/// Generators the engine has commands for (undoable, with the registry's
/// validation and defaults).
fn add_generator(app: &mut App, command: &str) {
    match app.execute(command, serde_json::json!({"seconds": GENERATOR_SECONDS})) {
        Ok(_) => app.status = "Media added".to_string(),
        Err(e) => app.status = e,
    }
}

/// Tone has no engine command yet: a real, undoable `Session::edit` closure —
/// the engine's own pattern for custom edits.
fn add_tone(app: &mut App) {
    let hz = 440.0_f32;
    let db = -6.0_f32;
    let result = app.session.edit("New Tone", |project, state| {
        let source = GeneratorSource::new(Generator::Tone { hz, db }, SEQUENCE_W, SEQUENCE_H, SEQUENCE_RATE, Tick(10 * TICKS_PER_SECOND));
        let info = source.info().clone();
        let name = info.name.clone();
        let id = project.add_item(
            name.as_str(),
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::Generator(Generator::Tone { hz, db }),
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
        );
        state.project_selection = vec![id];
        Ok(id)
    });
    match result {
        Ok(_) => {
            app.mark_dirty();
            app.status = "Media added".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

fn add_file(app: &mut App, path: std::path::PathBuf) {
    match app.execute("file.import", serde_json::json!({"paths": [path.display().to_string()]})) {
        Ok(_) => app.status = "Imported".to_string(),
        Err(e) => app.status = e,
    }
}

/// Place `media` at the end of the timeline through the engine's place
/// command (undoable, with its validation): a picture goes to V1 with its
/// sound on A1; audio-only media goes to A1 alone.
fn place_media(app: &mut App, media: ItemId) {
    let Some(seq_id) = app.sequence_id() else {
        app.status = "No sequence".to_string();
        return;
    };
    let Some((_, clip, _)) = app.session.project.resolve_media(media) else { return };
    let has_video = clip.info.video.is_some();
    let has_audio = clip.info.has_audio();
    let (mut v_end, mut a_end) = (Tick::ZERO, Tick::ZERO);
    if let Some(s) = app.sequence() {
        if let Some(t) = s.video_tracks.first() {
            for it in &t.items {
                v_end = v_end.max(it.start + it.duration);
            }
        }
        if let Some(t) = s.audio_tracks.first() {
            for it in &t.items {
                a_end = a_end.max(it.start + it.duration);
            }
        }
    }
    let at = v_end.max(a_end).0;
    let params = if has_video {
        serde_json::json!({"item": media.0, "track": "V1", "audioTrack": if has_audio { "A1" } else { "" }, "time": at, "sequence": seq_id.0})
    } else if has_audio {
        serde_json::json!({"item": media.0, "track": "A1", "time": at, "sequence": seq_id.0})
    } else {
        app.status = "Nothing to place: the media has no streams".to_string();
        return;
    };
    match app.execute("timeline.place", params) {
        Ok(_) => app.status = "Placed clip".to_string(),
        Err(e) => app.status = e,
    }
}

/// Remove `media` and its clips (undoable). Its composition stays keyed: an
/// undo restores the item under the same id, and the composition with it.
fn remove_media(app: &mut App, media: ItemId) {
    let result = app.session.edit("Remove Media", |project, _| {
        for seq in project.items.values_mut() {
            if let ItemKind::Sequence(s) = &mut seq.kind {
                let s = std::sync::Arc::make_mut(s);
                for track in s.video_tracks.iter_mut().chain(s.audio_tracks.iter_mut()) {
                    track.items.retain(|it| it.item != media);
                    track.sort();
                }
            }
        }
        project.root.remove_item(media);
        Ok(())
    });
    match result {
        Ok(()) => {
            app.mark_dirty();
            app.status = "Media removed".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

fn project_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("project"));
    panel.add_space(8.0);
    panel.heading("Project");
    panel.horizontal(|ui| {
        if ui.button("+ Bars & Tone").clicked() {
            add_generator(app, "file.newBarsAndTone");
        }
        if ui.button("+ Color Matte").clicked() {
            add_generator(app, "file.newColorMatte");
        }
    });
    panel.horizontal(|ui| {
        if ui.button("+ Tone").clicked() {
            add_tone(app);
        }
        if ui.button("+ File…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Media", &["mp4", "mov", "mkv", "webm", "wav", "mp3", "flac", "png", "jpg", "jpeg", "tiff", "bmp", "tga"])
                .pick_file()
        {
            add_file(app, path);
        }
    });
    panel.separator();

    let mut ids = Vec::new();
    app.session.project.root.all_items(&mut ids);
    let mut clicks = Vec::new();
    egui::ScrollArea::vertical().show(&mut panel, |ui| {
        for id in ids {
            let Some(item) = app.session.project.items.get(&id) else { continue };
            let ItemKind::Media(clip) = &item.kind else { continue };
            let name = item.name.clone();
            let kind = match &clip.media {
                MediaRef::Generator(_) => "generator",
                MediaRef::File { .. } => "file",
            };
            let has_comp = app.compositions.contains_key(&id);
            let label = format!("{name} [{kind}]{}", if has_comp { " ●" } else { "" });
            let response = ui.selectable_label(false, label).on_hover_text(if has_comp {
                "Has a compositing composition. Double-click: place on the timeline. Right-click: remove."
            } else {
                "Double-click: place on the timeline. Right-click: remove."
            });
            if response.double_clicked() {
                clicks.push((id, 0));
            } else if response.secondary_clicked() {
                clicks.push((id, 1));
            }
        }
    });
    for (id, which) in clicks {
        if which == 0 {
            place_media(app, id);
        } else {
            remove_media(app, id);
        }
    }
    panel.separator();
    panel.colored_label(egui::Color32::GRAY, "Double-click media to place it (undoable). Select a clip, then the Effects workspace.");
}

fn timeline_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut header =
        ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0))).id_salt("timeline-head"));
    header.add_space(4.0);
    header.horizontal(|ui| {
        ui.label("Timeline");
        ui.separator();
        if ui.button("Razor (all tracks)").clicked() {
            app.add_edit_all_tracks();
        }
        ui.separator();
        ui.add(egui::Slider::new(&mut app.zoom, 20.0..=400.0).text("px / s"));
        ui.separator();
        ui.colored_label(egui::Color32::GRAY, "Click: playhead · Drag clip: move · Drag edge: trim · Del: remove · Ctrl+Z: undo · Ctrl+K: add edit");
    });

    let Some(s) = app.sequence() else {
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 26.0), rect.max)));
        body.label("No sequence");
        return;
    };
    let rows = s.video_tracks.len() + s.audio_tracks.len();
    let strip = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 26.0), rect.max);
    let width = ((app.sequence_end().seconds() + 2.0) * app.zoom as f64).max(strip.width() as f64) as f32;
    let desired = egui::vec2(width, RULER_H + rows as f32 * ROW_H);
    let (alloc, response) = {
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(strip).id_salt("timeline-body"));
        body.allocate_exact_size(desired, egui::Sense::click_and_drag())
    };
    let painter = ui.painter_at(alloc);
    let to_x = |t: Tick| alloc.left() + t.seconds() as f32 * app.zoom;

    painter.rect_filled(alloc, 0.0, ui.visuals().extreme_bg_color);
    let seconds = (alloc.width() / app.zoom).ceil() as i64;
    for tick in 0..=seconds {
        let x = alloc.left() + tick as f32 * app.zoom;
        painter.line_segment(
            [egui::pos2(x, alloc.top()), egui::pos2(x, alloc.top() + RULER_H)],
            egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
        );
        if tick % 5 == 0 {
            painter.text(
                egui::pos2(x + 3.0, alloc.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{tick}s"),
                egui::FontId::monospace(10.0),
                ui.visuals().text_color(),
            );
        }
    }

    let mut hit_rects: Vec<(egui::Rect, filmcraft_project::ClipId)> = Vec::new();
    for (row, track) in s.video_tracks.iter().chain(&s.audio_tracks).enumerate() {
        let y = alloc.top() + RULER_H + row as f32 * ROW_H;
        let is_video = row < s.video_tracks.len();
        let row_rect = egui::Rect::from_min_max(egui::pos2(alloc.left(), y), egui::pos2(alloc.right(), y + ROW_H));
        painter.rect_filled(row_rect, 2.0, ui.visuals().faint_bg_color);
        let name = if is_video { format!("V{}", row + 1) } else { format!("A{}", row + 1 - s.video_tracks.len()) };
        painter.text(egui::pos2(alloc.left() + 6.0, y + ROW_H / 2.0), egui::Align2::LEFT_CENTER, name, egui::FontId::default(), ui.visuals().text_color());
        for item in &track.items {
            let clip_rect = egui::Rect::from_min_max(egui::pos2(to_x(item.start), y + 3.0), egui::pos2(to_x(item.start + item.duration), y + ROW_H - 3.0));
            let selected = app.selection == Some(item.id);
            let fill = if selected {
                ui.visuals().selection.bg_fill
            } else if is_video {
                egui::Color32::from_rgb(70, 105, 145)
            } else {
                egui::Color32::from_rgb(88, 120, 88)
            };
            painter.rect_filled(clip_rect, 3.0, fill);
            painter.rect_stroke(clip_rect, 3.0, egui::Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color), egui::StrokeKind::Inside);
            painter.text(
                clip_rect.left_top() + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                item.name.clone(),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
            hit_rects.push((clip_rect, item.id));
        }
    }

    let px = to_x(app.playhead);
    painter.line_segment([egui::pos2(px, alloc.top()), egui::pos2(px, alloc.bottom())], egui::Stroke::new(2.0, egui::Color32::LIGHT_RED));

    // Interaction: drag a clip to move it, its edges to trim (engine
    // commands on release — one undo step); click empty space to scrub.
    const EDGE: f32 = 8.0;
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // Read-only phase: what clip, what track, what gesture.
        let decision = hit_rects.iter().find(|(r, _)| r.contains(pos)).copied().and_then(|(rect, id)| {
            let (track, start) = app.sequence().and_then(|s| s.find_item(id).map(|(t, i)| (t, i.start)))?;
            let kind = if pos.x < rect.left() + EDGE {
                0
            } else if pos.x > rect.right() - EDGE {
                1
            } else {
                2
            };
            Some((id, track, start, kind))
        });
        match decision {
            Some((id, track, start, kind)) => {
                app.selection = Some(id);
                app.timeline_drag = match kind {
                    0 => Some(TimelineDrag::Trim { clip: id, edge: "in", grab_x: pos.x, last_x: pos.x }),
                    1 => Some(TimelineDrag::Trim { clip: id, edge: "out", grab_x: pos.x, last_x: pos.x }),
                    _ => Some(TimelineDrag::Move { clip: id, track, grab_x: pos.x, origin: start, last_x: pos.x }),
                };
            }
            None => app.timeline_drag = None,
        }
    }

    if response.dragged()
        && app.timeline_drag.is_some()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match &mut app.timeline_drag {
            Some(TimelineDrag::Move { last_x, .. }) => *last_x = pos.x,
            Some(TimelineDrag::Trim { last_x, .. }) => *last_x = pos.x,
            None => {}
        }
    }

    // Ghost preview of the drag (the document only changes on release).
    if let Some(drag) = &app.timeline_drag
        && let Some((rect, _)) = hit_rects.iter().find(|(_, id)| Some(*id) == drag_clip(drag)).copied()
    {
        let delta_x = match drag {
            TimelineDrag::Move { grab_x, last_x, .. } => last_x - grab_x,
            TimelineDrag::Trim { edge, grab_x, last_x, .. } => {
                if *edge == "in" {
                    (last_x - grab_x).min(rect.width() - 4.0)
                } else {
                    last_x - grab_x
                }
            }
        };
        let ghost = match drag {
            TimelineDrag::Move { .. } => rect.translate(egui::vec2(delta_x.max(-rect.left() + alloc.left()), 0.0)),
            TimelineDrag::Trim { edge, .. } => {
                if *edge == "in" {
                    egui::Rect::from_min_max(egui::pos2(rect.left() + delta_x.max(0.0).min(rect.width() - 4.0), rect.min.y), rect.max)
                } else {
                    egui::Rect::from_min_max(rect.min, egui::pos2((rect.right() + delta_x).max(rect.left() + 4.0), rect.max.y))
                }
            }
        };
        painter.rect_filled(ghost, 3.0, egui::Color32::from_rgba_unmultiplied(120, 170, 230, 90));
        painter.rect_stroke(ghost, 3.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 170, 230)), egui::StrokeKind::Inside);
    }

    if response.drag_stopped()
        && let Some(drag) = app.timeline_drag.take()
    {
        let zoom = app.zoom;
        match drag {
            TimelineDrag::Move { clip, track, grab_x, last_x, origin, .. } => {
                let delta = ((last_x - grab_x) / zoom) as f64 * TICKS_PER_SECOND as f64;
                let time = Tick((origin.0 as f64 + delta).max(0.0) as i64);
                let params = serde_json::json!({"moves": [{"clip": clip.0, "track": track.0, "time": time.0}], "insert": false, "linked": true});
                match app.execute("timeline.move", params) {
                    Ok(_) => app.status = "Moved clip".to_string(),
                    Err(e) => app.status = e,
                }
            }
            TimelineDrag::Trim { clip, edge, grab_x, last_x, .. } => {
                let delta = Tick((((last_x - grab_x) / zoom) as f64 * TICKS_PER_SECOND as f64) as i64);
                let params = serde_json::json!({"clip": clip.0, "edge": edge, "mode": "regular", "delta": delta.0});
                match app.execute("timeline.trim", params) {
                    Ok(_) => app.status = format!("Trimmed {edge} edge"),
                    Err(e) => app.status = e,
                }
            }
        }
    }

    // Scrubbing (click or drag on empty space — never while a clip drag runs).
    if (response.clicked() || response.dragged())
        && app.timeline_drag.is_none()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match hit_rects.iter().find(|(r, _)| r.contains(pos)).map(|(_, id)| *id) {
            Some(id) if response.clicked() => app.selection = Some(id),
            _ => {
                let secs = ((pos.x - alloc.left()) / app.zoom).max(0.0) as f64;
                app.playhead = Tick((secs * TICKS_PER_SECOND as f64) as i64);
                app.selection = None;
            }
        }
    }
}

/// The clip a drag operates on.
fn drag_clip(drag: &TimelineDrag) -> Option<filmcraft_project::ClipId> {
    match drag {
        TimelineDrag::Move { clip, .. } => Some(*clip),
        TimelineDrag::Trim { clip, .. } => Some(*clip),
    }
}

fn monitor_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    let Some(seq_id) = app.sequence_id() else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor"));
        area.vertical_centered(|ui| ui.label("No sequence. New project creates one; import media on the left and double-click it to place a clip."));
        return;
    };
    let rate = app.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);
    let frame = rate.frame_at(app.playhead);

    // Render only when the shown frame, the document revision or the sequence
    // changed — through the same compositor export uses.
    let needs = app.monitor.as_ref().is_none_or(|m| m.frame != frame || m.revision != app.revision || m.seq != seq_id);
    if needs {
        let playhead = app.playhead;
        let rendered = app
            .with_sources(|project, sources| filmcraft_render::render_sequence(project, seq_id, playhead, filmcraft_render::RenderOptions::default(), sources))
            .unwrap_or_else(|| filmcraft_render::Image::new(SEQUENCE_W as usize, SEQUENCE_H as usize));
        let size = (rendered.w as u32, rendered.h as u32);
        let image = pixels::to_color_image(&rendered);
        if let Some(m) = app.monitor.as_mut().filter(|m| m.seq == seq_id) {
            m.texture.set(image, egui::TextureOptions::default());
        } else {
            let texture = ui.ctx().load_texture("program-monitor", image, egui::TextureOptions::default());
            app.monitor = Some(Monitor { texture, frame, revision: app.revision, seq: seq_id, size });
        }
        if let Some(m) = app.monitor.as_mut() {
            m.frame = frame;
            m.revision = app.revision;
        }
    }

    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor"));
    if let Some(m) = &app.monitor {
        let avail = area.available_rect_before_wrap();
        let scale = (avail.width() / m.size.0 as f32).min((avail.height() - 24.0) / m.size.1 as f32);
        let shown = egui::vec2(m.size.0 as f32 * scale, m.size.1 as f32 * scale);
        let at = egui::pos2(avail.left() + (avail.width() - shown.x) / 2.0, avail.top() + (avail.height() - shown.y) / 2.0);
        let dest = egui::Rect::from_min_size(at, shown);
        ui.painter().image(m.texture.id(), dest, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
    area.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.colored_label(egui::Color32::GRAY, if app.wall_start.is_some() && app.playing { "wall clock (no audio device)" } else { "audio clock" });
        ui.label(format!(
            "{} × {} · frame {}",
            app.monitor.as_ref().map(|m| m.size.0).unwrap_or(0),
            app.monitor.as_ref().map(|m| m.size.1).unwrap_or(0),
            frame + 1
        ));
    });
}
