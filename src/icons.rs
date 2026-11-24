//! Small vector icons drawn with the painter, in the style of VS Code's codicons.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Painter, Rect, Sense, Shape, Stroke, Ui};

use crate::theme;

pub fn draw_chevron(p: &Painter, c: egui::Pos2, down: bool, color: Color32) {
    let s = 3.5;
    let pts = if down {
        vec![pos2(c.x - s, c.y - s / 2.0), pos2(c.x, c.y + s / 2.0), pos2(c.x + s, c.y - s / 2.0)]
    } else {
        vec![pos2(c.x - s / 2.0, c.y - s), pos2(c.x + s / 2.0, c.y), pos2(c.x - s / 2.0, c.y + s)]
    };
    p.add(Shape::line(pts, Stroke::new(1.3, color)));
}

pub fn draw_x(p: &Painter, c: egui::Pos2, s: f32, color: Color32) {
    let st = Stroke::new(1.2, color);
    p.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], st);
    p.line_segment([pos2(c.x + s, c.y - s), pos2(c.x - s, c.y + s)], st);
}

/// A small page with a folded corner.
pub fn draw_file_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let (w, h, f) = (9.0, 12.0, 3.0);
    let (l, t) = (c.x - w / 2.0, c.y - h / 2.0);
    let pts = vec![
        pos2(l, t),
        pos2(l + w - f, t),
        pos2(l + w, t + f),
        pos2(l + w, t + h),
        pos2(l, t + h),
    ];
    p.add(Shape::convex_polygon(pts, color.gamma_multiply(0.25), Stroke::new(1.2, color)));
}

pub fn draw_files_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.5, color);
    let back = Rect::from_min_size(pos2(c.x - 5.0, c.y - 11.0), vec2(13.0, 17.0));
    let front = Rect::from_min_size(pos2(c.x - 9.0, c.y - 6.0), vec2(13.0, 17.0));
    p.line_segment([back.left_top(), back.right_top()], st);
    p.line_segment([back.right_top(), back.right_bottom()], st);
    p.line_segment([back.left_top(), pos2(back.left(), front.top())], st);
    p.line_segment([back.right_bottom(), pos2(front.right(), back.bottom())], st);
    p.rect_filled(front, 0.0, theme::p().activity_bg);
    p.rect_stroke(front, 1.0, st, egui::StrokeKind::Middle);
}

pub fn draw_trash_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.2, color);
    p.line_segment([pos2(c.x - 6.0, c.y - 4.0), pos2(c.x + 6.0, c.y - 4.0)], st);
    p.line_segment([pos2(c.x - 2.0, c.y - 6.0), pos2(c.x + 2.0, c.y - 6.0)], st);
    let body = vec![pos2(c.x - 4.5, c.y - 4.0), pos2(c.x - 3.5, c.y + 6.0), pos2(c.x + 3.5, c.y + 6.0), pos2(c.x + 4.5, c.y - 4.0)];
    p.add(Shape::line(body, st));
}

pub fn draw_gear_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.6, color);
    p.circle_stroke(c, 3.5, st);
    p.circle_stroke(c, 8.0, st);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let dir = vec2(a.cos(), a.sin());
        p.line_segment([c + dir * 8.0, c + dir * 11.0], Stroke::new(3.0, color));
    }
}

/// An activity-bar button: a rounded hover background, and a pill-shaped
/// accent marker on the left when active.
pub fn activity_item(
    ui: &Ui,
    r: Rect,
    id: &str,
    active: bool,
    p: &theme::Palette,
    draw: fn(&Painter, egui::Pos2, Color32),
) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(r.shrink2(vec2(6.0, 4.0)), CornerRadius::same(8), p.list_hover);
    }
    if active {
        let marker = Rect::from_center_size(pos2(r.left() + 1.5, r.center().y), vec2(3.0, 22.0));
        painter.rect_filled(marker, CornerRadius::same(2), p.accent);
    }
    let color = if active || resp.hovered() { p.activity_fg } else { p.icon_inactive };
    draw(painter, r.center(), color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A folder, open or closed, in a warm folder color.
pub fn draw_folder_icon(p: &Painter, c: egui::Pos2, open: bool) {
    let color = Color32::from_rgb(0xdc, 0xb6, 0x7a);
    let (w, h) = (14.0, 10.5);
    let body = Rect::from_center_size(c + vec2(0.0, 0.5), vec2(w, h));
    // The tab on top.
    let tab = Rect::from_min_size(body.min - vec2(0.0, 2.0), vec2(6.0, 3.0));
    p.rect_filled(tab, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, color);
    if open {
        p.rect_filled(body, CornerRadius::same(2), color.gamma_multiply(0.55));
        let lid = vec![
            pos2(body.left() + 2.5, body.top() + 3.0),
            pos2(body.right() + 1.0, body.top() + 3.0),
            pos2(body.right() - 1.5, body.bottom()),
            pos2(body.left(), body.bottom()),
        ];
        p.add(Shape::convex_polygon(lid, color, Stroke::NONE));
    } else {
        p.rect_filled(body, CornerRadius::same(2), color);
    }
}

/// A circular arrow: an open ring with an arrowhead at its upper-right end.
pub fn draw_refresh_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let r = 5.5;
    // From just right of the top, counterclockwise most of the way round.
    let (start, sweep) = (-1.15_f32, -4.9_f32);
    let points: Vec<egui::Pos2> = (0..=24)
        .map(|i| {
            let a = start + sweep * i as f32 / 24.0;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    p.add(Shape::line(points, Stroke::new(1.3, color)));
    // The head points along the ring at the start, clockwise.
    let tip = pos2(c.x + r * start.cos(), c.y + r * start.sin());
    let along = vec2(-start.sin(), start.cos());
    let out = vec2(start.cos(), start.sin());
    let head = tip + along * 1.5;
    p.add(Shape::convex_polygon(
        vec![head, head - along * 4.0 + out * 2.6, head - along * 4.0 - out * 2.6],
        color,
        Stroke::NONE,
    ));
}

/// Two stacked squares, the front one with a minus: collapse all.
pub fn draw_collapse_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.1, color);
    // On the pixel grid, so the thin lines stay crisp.
    let front = Rect::from_min_size(pos2((c.x - 6.0).round() + 0.5, (c.y - 3.0).round() + 0.5), vec2(9.0, 9.0));
    p.rect_stroke(front, CornerRadius::same(2), st, egui::StrokeKind::Middle);
    // The square behind shows as an L above and to the right.
    let back = vec![
        pos2(front.left() + 3.0, front.top() - 3.0),
        pos2(front.right() + 3.0, front.top() - 3.0),
        pos2(front.right() + 3.0, front.bottom() - 3.0),
    ];
    p.add(Shape::line(back, st));
    p.line_segment([pos2(front.left() + 2.5, front.center().y), pos2(front.right() - 2.5, front.center().y)], st);
}

pub fn draw_split_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let r = Rect::from_center_size(c, vec2(14.0, 12.0));
    p.rect_stroke(r, 1.5, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    p.line_segment([pos2(c.x, r.top()), pos2(c.x, r.bottom())], Stroke::new(1.2, color));
}

/// A page with a narrow strip on its right: the minimap.
pub fn draw_minimap_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let r = Rect::from_center_size(c, vec2(14.0, 12.0));
    p.rect_stroke(r, 1.5, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    for (i, w) in [6.0, 4.0, 5.0].iter().enumerate() {
        let y = r.top() + 3.5 + i as f32 * 2.5;
        p.line_segment([pos2(r.left() + 2.5, y), pos2(r.left() + 2.5 + w, y)], Stroke::new(1.0, color));
    }
    p.line_segment([pos2(r.right() - 4.0, r.top()), pos2(r.right() - 4.0, r.bottom())], Stroke::new(1.0, color));
}

pub fn draw_search_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.6, color);
    p.circle_stroke(pos2(c.x + 2.0, c.y - 2.0), 7.0, st);
    p.line_segment([pos2(c.x - 3.0, c.y + 3.0), pos2(c.x - 9.0, c.y + 9.0)], Stroke::new(2.2, color));
}


/// The title row at the top of a sidebar view ("EXPLORER", "SEARCH").
pub fn sidebar_header(ui: &mut Ui, title: &str) -> Rect {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 35.0), Sense::hover());
    ui.painter().text(r.left_center() + vec2(20.0, 0.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.0), theme::p().text_ui);
    r
}

/// A small icon button for a header; returns whether it was clicked.
pub fn header_button(ui: &Ui, r: Rect, id: &str, tip: &str, draw: fn(&Painter, egui::Pos2, Color32)) -> bool {
    let p = theme::p();
    let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), p.list_hover);
    }
    draw(ui.painter(), r.center(), if resp.hovered() { p.text_bright } else { p.text_muted });
    resp.on_hover_text(tip).clicked()
}

/// A floating box at the top of the window (Go to File, the command palette).
pub fn overlay_frame(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut Ui)) {
    let width = (ctx.content_rect().width() * 0.6).clamp(320.0, 620.0);
    egui::Area::new(egui::Id::unique(id))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 8.0))
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(theme::p().widget_bg)
                .stroke(Stroke::new(1.0, theme::p().border))
                .corner_radius(8)
                .inner_margin(6)
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(140) })
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui);
                });
        });
}

/// A key combination as keycaps ("Ctrl+Shift+P" → [Ctrl] [Shift] [P]),
/// right-aligned to `right`. Returns the left edge.
pub fn keycaps_right(p: &Painter, right: f32, y: f32, label: &str, color: Color32) -> f32 {
    let pal = theme::p();
    let caps: Vec<&str> = if label.len() > 1 && label.contains('+') && !label.ends_with('+') { label.split('+').collect() } else { vec![label] };
    let mut x = right;
    for cap in caps.iter().rev() {
        let g = p.layout_no_wrap(cap.to_string(), FontId::proportional(11.0), color);
        let chip = Rect::from_min_max(pos2(x - g.size().x - 10.0, y - 9.0), pos2(x, y + 9.0));
        p.rect(chip, CornerRadius::same(4), pal.input_bg, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
        p.galley(chip.center() - g.size() / 2.0, g, color);
        x = chip.left() - 3.0;
    }
    x
}

/// `text` with the chars at `hits` drawn brighter, for fuzzy-match results.
pub fn highlighted_job(text: &str, hits: &[usize], font: FontId, normal: Color32, bright: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (i, c) in text.chars().enumerate() {
        let hit = hits.contains(&i);
        let format = egui::TextFormat {
            font_id: font.clone(),
            color: if hit { bright } else { normal },
            underline: if hit { Stroke::new(1.0, bright.gamma_multiply(0.6)) } else { Stroke::NONE },
            ..Default::default()
        };
        job.append(&c.to_string(), 0.0, format);
    }
    job
}

/// A git branch: a line between two rings, with a third ring branching off.
pub fn draw_branch_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.1, color);
    let r = 1.8;
    let (top, bottom, side) = (pos2(c.x - 3.0, c.y - 4.5), pos2(c.x - 3.0, c.y + 4.5), pos2(c.x + 3.0, c.y - 4.5));
    for ring in [top, bottom, side] {
        p.circle_stroke(ring, r, st);
    }
    p.line_segment([top + vec2(0.0, r), bottom - vec2(0.0, r)], st);
    let bend = pos2(side.x, c.y - 0.5);
    p.add(Shape::line(vec![side + vec2(0.0, r), bend, pos2(top.x + 0.5, c.y + 2.5)], st));
}
