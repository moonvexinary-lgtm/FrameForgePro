#![allow(dead_code)] // The design kit is intentionally complete; workspaces adopt pieces incrementally.

//! The FrameForge Pro design system: the palette, type scale and the small
//! painted widgets the TSX mock-up uses (panel headers, tabs, cards and the
//! monitor overlays). Keeping them here means every workspace speaks the same
//! visual language without a second UI toolkit.

use egui::{Align2, Color32, FontId, Id, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2, pos2, vec2};

pub const ACCENT: Color32 = Color32::from_rgb(0x35, 0xc2, 0xb5);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1b, 0x4a, 0x46);
pub const ACCENT_HI: Color32 = Color32::from_rgb(0x62, 0xdc, 0xcf);
pub const ACCENT_SEL: Color32 = Color32::from_rgb(0x24, 0x40, 0x3d);
pub const BG_APP: Color32 = Color32::from_rgb(0x15, 0x15, 0x15);
pub const BG_CONTROL: Color32 = Color32::from_rgb(0x2e, 0x2e, 0x2e);
pub const BG_HEADER: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const BG_INSET: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const LINE_DARK: Color32 = Color32::from_rgb(0x0f, 0x0f, 0x0f);
pub const LINE_LIGHT: Color32 = Color32::from_rgb(0x3b, 0x3b, 0x3b);
pub const TEXT_HI: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);
pub const TEXT_MID: Color32 = Color32::from_rgb(0xb3, 0xb3, 0xb3);
pub const TEXT_LO: Color32 = Color32::from_rgb(0x7c, 0x7c, 0x7c);
pub const TEXT_OFF: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
pub const DANGER: Color32 = Color32::from_rgb(0xe5, 0x53, 0x4b);
pub const WARN: Color32 = Color32::from_rgb(0xe2, 0xa3, 0x36);
pub const CLIP_VIDEO: Color32 = Color32::from_rgb(0x8b, 0x79, 0xd1);
pub const CLIP_VIDEO_HI: Color32 = Color32::from_rgb(0xa8, 0x98, 0xe6);
pub const CLIP_AUDIO: Color32 = Color32::from_rgb(0x3f, 0x9b, 0x62);
pub const CLIP_AUDIO_HI: Color32 = Color32::from_rgb(0x5c, 0xc2, 0x83);
pub const CLIP_TITLE: Color32 = Color32::from_rgb(0xd3, 0x6b, 0xc8);
pub const CLIP_ADJUST: Color32 = Color32::from_rgb(0xc5, 0x8f, 0xd9);

/// Install the mock-up's palette and the compact type/spacing scale. Called
/// once, before the first frame, from the eframe creation callback.
pub fn install(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = BG_INSET;
    visuals.faint_bg_color = BG_APP;
    visuals.override_text_color = Some(TEXT_HI);
    visuals.hyperlink_color = ACCENT_HI;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = DANGER;

    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE_LIGHT);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MID);

    visuals.widgets.inactive.bg_fill = BG_CONTROL;
    visuals.widgets.inactive.weak_bg_fill = BG_CONTROL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE_LIGHT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_MID);

    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_HI);

    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT_HI);

    visuals.widgets.open.bg_fill = BG_HOVER;
    visuals.widgets.open.weak_bg_fill = BG_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_HI);

    visuals.selection.bg_fill = ACCENT_SEL;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = vec2(6.0, 4.0);
    style.spacing.button_padding = vec2(8.0, 3.0);
    style.spacing.interact_size = vec2(34.0, 22.0);
    style.spacing.slider_width = 120.0;
    style.spacing.combo_width = 108.0;
    style.spacing.text_edit_width = 160.0;
    style.spacing.indent = 12.0;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(16.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// A panel background plus a child UI clipped to the same rectangle.
pub fn panel(ui: &mut Ui, rect: Rect, id: &str) -> Ui {
    ui.painter().rect_filled(rect, 0.0, BG_PANEL);
    ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(id))
}

/// The 24 px title bar the mock-up puts on every panel. The accent rule is
/// under the title, not the whole panel, so adjacent headers stay quiet.
pub fn panel_header(ui: &mut Ui, title: &str) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, BG_HEADER);
    let galley = painter.layout_no_wrap(title.to_string(), FontId::proportional(11.0), TEXT_HI);
    let at = pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x;
    painter.galley(at, galley, TEXT_HI);
    painter.rect_filled(Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - 2.0), vec2(width, 2.0)), 0.0, ACCENT);
    rect
}

/// A panel tab strip. Returns the index clicked this frame. `selected` is the
/// currently open tab; the selected tab gets the same 2 px accent rule.
pub fn tabs(ui: &mut Ui, id: &'static str, labels: &[&str], selected: usize) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, BG_HEADER);
    let mut clicked = None;
    let mut x = rect.left() + 4.0;
    for (index, label) in labels.iter().enumerate() {
        let color = if index == selected { TEXT_HI } else { TEXT_MID };
        let galley = ui.painter().layout_no_wrap((*label).to_string(), FontId::proportional(11.0), color);
        let tab = Rect::from_min_size(pos2(x, rect.top()), vec2(galley.size().x + 18.0, rect.height()));
        let response = ui.interact(tab, Id::new((id, index)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(tab, 0.0, BG_HOVER);
        }
        let at = pos2(tab.left() + 9.0, tab.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, color);
        if index == selected {
            ui.painter().rect_filled(Rect::from_min_size(pos2(tab.left() + 9.0, rect.bottom() - 2.0), vec2(tab.width() - 18.0, 2.0)), 0.0, ACCENT);
        }
        if response.clicked() {
            clicked = Some(index);
        }
        x += tab.width();
    }
    clicked
}

/// A small uppercase section label, used inside the inspector and browser.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(7.0);
    ui.label(RichText::new(title).size(10.0).strong().color(TEXT_LO));
}

/// A key/value row with the value in the high-contrast text colour.
pub fn kv(ui: &mut Ui, key: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(11.0).color(TEXT_LO));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value.into()).size(11.0).color(TEXT_MID));
        });
    });
}

/// A full-width quiet button, the default action shape in the mock-up.
pub fn full_button(ui: &mut Ui, label: &str) -> Response {
    let width = ui.available_width();
    ui.add_sized([width, 24.0], egui::Button::new(RichText::new(label).size(12.0)).fill(BG_CONTROL))
}

/// A full-width accent button for the primary action in a panel.
pub fn accent_button(ui: &mut Ui, label: &str) -> Response {
    let width = ui.available_width();
    ui.add_sized([width, 24.0], egui::Button::new(RichText::new(label).size(12.0).strong().color(BG_APP)).fill(ACCENT))
}

/// A small coloured status dot and its label.
pub fn status(ui: &mut Ui, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, color);
        ui.label(RichText::new(text).size(10.5).color(TEXT_LO));
    });
}

/// Fit `size` inside `rect` without stretching, centred (letterboxed).
pub fn fit_rect(rect: Rect, size: (u32, u32)) -> Rect {
    let scale = (rect.width() / size.0.max(1) as f32).min(rect.height() / size.1.max(1) as f32);
    let shown = vec2(size.0 as f32 * scale, size.1 as f32 * scale);
    Rect::from_center_size(rect.center(), shown)
}

/// Draw a texture letterboxed inside `rect`, on the inset background.
pub fn image(ui: &Ui, rect: Rect, texture: egui::TextureId, size: (u32, u32)) {
    ui.painter().rect_filled(rect, 0.0, BG_INSET);
    let dest = fit_rect(rect, size);
    ui.painter().image(texture, dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// Title/action-safe guides and a thirds grid over the program picture.
pub fn guides(painter: &egui::Painter, rect: Rect, safe: bool, grid: bool) {
    if safe {
        let title = rect.shrink2(rect.size() * 0.1);
        let action = rect.shrink2(rect.size() * 0.05);
        painter.rect_stroke(title, 0.0, Stroke::new(1.0, Color32::from_white_alpha(90)), StrokeKind::Inside);
        painter.rect_stroke(action, 0.0, Stroke::new(1.0, Color32::from_white_alpha(45)), StrokeKind::Inside);
    }
    if grid {
        for i in 1..3 {
            let x = rect.left() + rect.width() * i as f32 / 3.0;
            let y = rect.top() + rect.height() * i as f32 / 3.0;
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(28)));
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, Color32::from_white_alpha(28)));
        }
    }
}

/// A framed badge, e.g. "GPU" or "Rec.709". Keeps the status bar readable.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let font = FontId::proportional(10.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = vec2(galley.size().x + 12.0, 16.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 8.0, BG_CONTROL);
    ui.painter().galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

/// A thin 1 px rule that matches the panel borders.
pub fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, LINE_LIGHT);
}

/// The mock-up's compact text style for secondary explanatory copy.
pub fn hint(text: &str) -> RichText {
    RichText::new(text).size(10.5).color(TEXT_LO)
}

/// A tiny label with the accent colour, for values that are live.
pub fn live(text: &str) -> RichText {
    RichText::new(text).size(11.0).strong().color(ACCENT_HI)
}

/// Draw a left-aligned title, a subtitle and a right-aligned status line; the
/// header used by the program monitor and the new workspaces.
pub fn title_line(ui: &mut Ui, title: &str, subtitle: &str, right: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(12.0).strong().color(TEXT_HI));
        if !subtitle.is_empty() {
            ui.label(RichText::new(subtitle).size(10.5).color(TEXT_LO));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !right.is_empty() {
                ui.label(RichText::new(right).size(10.0).color(TEXT_LO));
            }
        });
    });
}

/// A standard panel body margin. The mock-up uses 8–12 px, not egui's default.
pub fn inset(ui: &mut Ui) {
    ui.add_space(8.0);
    ui.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);
}

/// An empty-state block with a single accent action, used by unbuilt panels.
pub fn empty_state(ui: &mut Ui, title: &str, body: &str, action: &str) -> bool {
    ui.add_space(18.0);
    ui.label(RichText::new(title).size(14.0).strong().color(TEXT_HI));
    ui.add_space(4.0);
    ui.label(RichText::new(body).size(11.0).color(TEXT_LO));
    ui.add_space(10.0);
    accent_button(ui, action).clicked()
}

/// The accent underline used under the active workspace tab. Kept separate so
/// the top bar and the panel tabs can share the same visual rule.
pub fn underline(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom() - 2.0), vec2(rect.width(), 2.0)), 0.0, ACCENT);
}

/// A standard, non-wrapping tab label measurement for callers that lay out
/// their own tabs (the workspace strip does).
pub fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(size), TEXT_HI).size().x
}

/// Align a small right-hand value inside a row.
pub fn right_label(ui: &mut Ui, text: impl Into<String>) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(text.into()).size(10.5).color(TEXT_LO));
    });
}

/// A 2 px teal selection bar for cards and list rows.
pub fn selected_bar(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(Rect::from_min_size(rect.left_top(), vec2(2.0, rect.height())), 0.0, ACCENT);
}

/// The standard header height used by the shell.
pub const HEADER_H: f32 = 24.0;

/// The standard row height used by lists and browsers.
pub const ROW_H: f32 = 28.0;

/// The standard panel padding.
pub const PAD: f32 = 8.0;

/// The window background, painted before the panels so gaps show the app colour.
pub fn background(ui: &Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, BG_APP);
}

/// A small `×`/`−` style square button used by the layer rows.
pub fn mini_button(ui: &mut Ui, text: &str, tooltip: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).size(10.0).color(TEXT_MID)).small()).on_hover_text(tooltip)
}

/// A horizontal split line with 8 px breathing room.
pub fn spaced_rule(ui: &mut Ui) {
    ui.add_space(6.0);
    rule(ui);
    ui.add_space(6.0);
}

/// A helper for the status bar's right-aligned text.
pub fn right_text(ui: &mut Ui, text: &str, color: Color32) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).size(10.0).color(color));
    });
}

/// The label style used by the mock-up's tiny metadata rows.
pub fn meta(text: &str) -> RichText {
    RichText::new(text).size(10.0).color(TEXT_LO)
}

/// A compact `▶`/`❚❚` transport label. The glyphs are deliberately text so
/// they inherit the same font and colour as the rest of the shell.
pub fn play_label(playing: bool) -> &'static str {
    if playing { "❚❚" } else { "▶" }
}

/// A safe area used by the empty states.
pub fn body_rect(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min + vec2(PAD, PAD), rect.max - vec2(PAD, PAD))
}

/// A title for a floating box, matching the panel headers.
pub fn floating_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(TEXT_HI));
    ui.add_space(2.0);
}

/// A note that never pretends a feature is available.
pub fn unavailable(ui: &mut Ui, what: &str, why: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(what).size(11.0).strong().color(WARN));
    ui.label(RichText::new(why).size(10.5).color(TEXT_LO));
}

/// The app's standard horizontal padding.
pub fn left_pad(ui: &mut Ui) {
    ui.add_space(PAD);
}

/// The app's standard vertical padding.
pub fn top_pad(ui: &mut Ui) {
    ui.add_space(PAD);
}

/// The app's standard small gap.
pub fn gap(ui: &mut Ui) {
    ui.add_space(4.0);
}

/// The app's standard larger gap.
pub fn big_gap(ui: &mut Ui) {
    ui.add_space(10.0);
}

/// The app's standard footer note.
pub fn footer_hint(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(10.0).color(TEXT_OFF));
}

/// The colour for a clip row, keyed by its kind.
pub fn clip_color(video: bool, selected: bool) -> Color32 {
    match (video, selected) {
        (true, true) => CLIP_VIDEO_HI,
        (true, false) => CLIP_VIDEO,
        (false, true) => CLIP_AUDIO_HI,
        (false, false) => CLIP_AUDIO,
    }
}

/// A common rounded card fill.
pub fn card(ui: &mut Ui, rect: Rect, selected: bool) {
    ui.painter().rect_filled(rect, 3.0, if selected { ACCENT_SEL } else { BG_INSET });
    ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, if selected { ACCENT } else { LINE_LIGHT }), StrokeKind::Inside);
}

/// A tiny accent marker for "live" values.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}

/// The label used for the currently selected workspace.
pub fn tab_label(text: &str, selected: bool) -> RichText {
    RichText::new(text).size(11.0).color(if selected { TEXT_HI } else { TEXT_MID })
}

/// The standard corner radius for cards.
pub const RADIUS: f32 = 3.0;

/// The standard line height for the mock-up's compact text.
pub const LINE_H: f32 = 14.0;

/// A compact left-aligned label without wrapping.
pub fn nowrap(ui: &mut Ui, text: impl Into<String>, color: Color32, size: f32) {
    let text = text.into();
    let galley = ui.painter().layout_no_wrap(text, FontId::proportional(size), color);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
}

/// A standard icon-like glyph in a square.
pub fn glyph(ui: &mut Ui, text: &str, tooltip: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
    let response = ui.interact(rect, Id::new(("glyph", text, rect.left() as i32, rect.top() as i32)), Sense::click());
    let color = if response.hovered() { ACCENT_HI } else { TEXT_MID };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), color);
    response.on_hover_text(tooltip)
}
