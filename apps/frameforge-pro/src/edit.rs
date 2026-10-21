//! The Edit workspace: the project panel (media items), the program monitor
//! (real frames through the engine) and the timeline (real clips, real
//! playhead). Laid out by rect — this egui generation has no panel helpers.

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, TrackKind};
use filmcraft_render::{RenderOptions, render_sequence};
use filmcraft_time::{TICKS_PER_SECOND, Tick, TimeRange};

use crate::app::{App, Monitor, SEQUENCE_H, SEQUENCE_RATE, SEQUENCE_W};
use crate::pixels;

const ROW_H: f32 = 34.0;
const RULER_H: f32 = 18.0;
const GENERATOR_SECONDS: i64 = 10;

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

/// One media item's import: mirror the generator in both the MediaRef and the
/// source's own info, the way the editing engine does.
fn add_generator(app: &mut App, generator: Generator) {
    let source = GeneratorSource::new(generator.clone(), SEQUENCE_W, SEQUENCE_H, SEQUENCE_RATE, Tick(GENERATOR_SECONDS * TICKS_PER_SECOND));
    let info = source.info().clone();
    let name = info.name.clone();
    app.doc.doc.project.add_item(
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
    );
    app.doc.mark_dirty();
    app.status = "Media added".to_string();
}

fn add_file(app: &mut App, path: std::path::PathBuf) {
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            app.status = format!("Could not read {}: {e}", path.display());
            return;
        }
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    match filmcraft_media::open_bytes(&name, bytes.into(), &filmcraft_codecs::openers()) {
        Ok(source) => {
            let info = source.info().clone();
            app.doc.doc.project.add_item(
                name.as_str(),
                Label::Iris,
                ItemKind::Media(MediaClip {
                    media: MediaRef::File { path: path.display().to_string() },
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
            app.doc.mark_dirty();
            app.status = format!("Imported {name}");
        }
        Err(e) => app.status = format!("{name}: {e}"),
    }
}

/// Place `media` at the end of the timeline, as the editing engine's place
/// command does: a video clip on V1 and an audio clip on A1, both real
/// `make_track_item` edits.
fn place_media(app: &mut App, media: ItemId) {
    let Some(seq_id) = app.sequence_id() else {
        app.status = "No sequence".to_string();
        return;
    };
    // Read what the media is, then drop the borrow before mutating.
    let Some((_, clip, _)) = app.doc.doc.project.resolve_media(media) else { return };
    let name = clip.info.name.clone();
    let has_video = clip.info.video.is_some();
    let has_audio = clip.info.has_audio();
    let duration = clip.info.duration;
    let rate = app.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);

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
    let start = v_end.max(a_end);
    let source = TimeRange::new(Tick::ZERO, duration);

    let mut placed = 0;
    if has_video
        && let Some(mut item) = app.doc.doc.project.make_track_item(media, TrackKind::Video, start, source, rate)
        && let Some(s) = app.doc.doc.project.sequence_mut(seq_id)
        && let Some(track) = s.video_tracks.first_mut()
    {
        item.name = name.clone();
        track.items.push(item);
        track.sort();
        placed += 1;
    }
    if has_audio
        && let Some(mut item) = app.doc.doc.project.make_track_item(media, TrackKind::Audio, start, source, rate)
        && let Some(s) = app.doc.doc.project.sequence_mut(seq_id)
        && let Some(track) = s.audio_tracks.first_mut()
    {
        item.name = name;
        track.items.push(item);
        track.sort();
        placed += 1;
    }
    if placed > 0 {
        app.doc.mark_dirty();
        app.status = format!("Placed {placed} clip(s)");
    } else {
        app.status = "Nothing to place: the media has no streams".to_string();
    }
}

fn remove_media(app: &mut App, media: ItemId) {
    if let Some(seq) = app.sequence_id()
        && let Some(s) = app.doc.doc.project.sequence_mut(seq)
    {
        for track in s.video_tracks.iter_mut().chain(s.audio_tracks.iter_mut()) {
            track.items.retain(|it| it.item != media);
            track.sort();
        }
    }
    app.doc.doc.compositions.remove(&media);
    app.doc.doc.project.root.remove_item(media);
    app.doc.mark_dirty();
    app.status = "Media removed".to_string();
}

fn project_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("project"));
    panel.add_space(8.0);
    panel.heading("Project");
    panel.horizontal(|ui| {
        if ui.button("+ Bars & Tone").clicked() {
            add_generator(app, Generator::BarsAndTone);
        }
    });
    ui.horizontal(|ui| {
        if ui.button("+ Tone").clicked() {
            add_generator(app, Generator::Tone { hz: 440.0, db: -6.0 });
        }
        if ui.button("+ File…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Media", &["mp4", "mov", "mkv", "webm", "wav", "mp3", "flac", "png", "jpg", "jpeg", "tiff", "bmp", "tga"])
                .pick_file()
            {
                add_file(app, path);
            }
        }
    });
    panel.separator();

    let mut ids = Vec::new();
    app.doc.doc.project.root.all_items(&mut ids);
    let mut clicks = Vec::new();
    egui::ScrollArea::vertical().show(&mut panel, |ui| {
        for id in ids {
            let Some(item) = app.doc.doc.project.items.get(&id) else { continue };
            let ItemKind::Media(clip) = &item.kind else { continue };
            let name = item.name.clone();
            let kind = match &clip.media {
                MediaRef::Generator(_) => "generator",
                MediaRef::File { .. } => "file",
            };
            let has_comp = app.doc.doc.compositions.contains_key(&id);
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
    panel.colored_label(egui::Color32::GRAY, "Double-click media to place it. Select a clip, then open the Effects workspace.");
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
        ui.add(egui::Slider::new(&mut app.zoom, 20.0..=400.0).text("px / s"));
        ui.separator();
        ui.colored_label(
            egui::Color32::GRAY,
            "Click: playhead · Click a clip: select · Del: remove · Trimming and drag edits arrive with the engine session (M6)",
        );
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

    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
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
    let needs = app.monitor.as_ref().map_or(true, |m| m.frame != frame || m.revision != app.doc.revision || m.seq != seq_id);
    if needs {
        let rendered = {
            let sources = app.doc.sources();
            render_sequence(&app.doc.doc.project, seq_id, app.playhead, RenderOptions::default(), sources)
        };
        let size = (rendered.w as u32, rendered.h as u32);
        let image = pixels::to_color_image(&rendered);
        if let Some(m) = app.monitor.as_mut().filter(|m| m.seq == seq_id) {
            m.texture.set(image, egui::TextureOptions::default());
        } else {
            let texture = ui.ctx().load_texture("program-monitor", image, egui::TextureOptions::default());
            app.monitor = Some(Monitor { texture, frame, revision: app.doc.revision, seq: seq_id, size });
        }
        if let Some(m) = app.monitor.as_mut() {
            m.frame = frame;
            m.revision = app.doc.revision;
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
        ui.colored_label(egui::Color32::GRAY, "silent preview — audio playback arrives with the engine session (M6)");
        ui.label(format!(
            "{} × {} · frame {}",
            app.monitor.as_ref().map(|m| m.size.0).unwrap_or(0),
            app.monitor.as_ref().map(|m| m.size.1).unwrap_or(0),
            frame + 1
        ));
    });
}
