//! Drawing and mouse handling for one editor view: a buffer shown in a
//! rectangle, with optional soft wrapping.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Response, Sense, Stroke, Ui};

use crate::buffer::{tab_size, Buffer, Pos};
use crate::settings::Settings;
use crate::syntax;
use crate::theme;
use crate::vim::{self, Mode, Vim};

const SCROLLBAR_W: f32 = 14.0;
const MINIMAP_W: f32 = 100.0;
/// Height of one line in the minimap, and the font size of its tiny text.
const MM_ROW: f32 = 2.0;
const MM_FONT: f32 = 2.2;

/// How a line is cut into display rows.
pub struct LineLayout {
    /// Screen column (tabs expanded) before each char, plus one past the end.
    pub vcols: Vec<usize>,
    /// Char index where each display row starts; the first is always 0.
    pub starts: Vec<usize>,
    /// Columns continuation rows are shifted right by (wrapped lines keep their indent).
    pub cont_indent: usize,
}

impl LineLayout {
    /// Lay out line `l`, wrapping at `cols` screen columns if given. Rows break
    /// after whitespace when possible.
    pub fn new(buf: &Buffer, l: usize, cols: Option<usize>) -> Self {
        let line = &buf.lines[l];
        let ts = tab_size();
        let mut vcols = Vec::with_capacity(line.len() + 1);
        let mut v = 0;
        for c in line {
            vcols.push(v);
            v += if *c == '\t' { ts - v % ts } else { 1 };
        }
        vcols.push(v);
        let mut starts = vec![0];
        let mut cont_indent = 0;
        if let Some(cols) = cols.filter(|&c| v > c) {
            let indent_chars = line.iter().take_while(|c| c.is_whitespace()).count();
            cont_indent = if vcols[indent_chars] * 2 < cols { vcols[indent_chars] } else { 0 };
            let mut s = 0;
            let mut brk: Option<usize> = None;
            for i in 0..line.len() {
                let limit = if starts.len() == 1 { cols } else { cols - cont_indent };
                if vcols[i + 1] - vcols[s] > limit && i > s {
                    let ns = brk.filter(|&b| b > s && b <= i).unwrap_or(i);
                    starts.push(ns);
                    s = ns;
                    brk = None;
                }
                if line[i].is_whitespace() && i >= indent_chars {
                    brk = Some(i + 1);
                }
            }
        }
        Self { vcols, starts, cont_indent }
    }

    pub fn rows(&self) -> usize {
        self.starts.len()
    }

    /// Display row holding char `col`.
    pub fn row_of(&self, col: usize) -> usize {
        self.starts.partition_point(|&s| s <= col).saturating_sub(1)
    }

    /// Char range `[start, end)` of row `r`.
    pub fn range(&self, r: usize) -> (usize, usize) {
        let end = self.starts.get(r + 1).copied().unwrap_or(self.vcols.len() - 1);
        (self.starts[r], end)
    }

    /// Screen column of char `col` within its row (past-the-end cols extend right).
    pub fn x_col(&self, r: usize, col: usize) -> usize {
        let base = self.vcols[self.starts[r]];
        let indent = if r > 0 { self.cont_indent } else { 0 };
        let len = self.vcols.len() - 1;
        let v = if col <= len { self.vcols[col] } else { self.vcols[len] + (col - len) };
        indent + v - base
    }

    /// Char under screen column `x` of row `r`.
    pub fn col_at(&self, r: usize, x: usize) -> usize {
        let (s, e) = self.range(r);
        let last_row = r + 1 == self.rows();
        let target = self.vcols[s] + x.saturating_sub(if r > 0 { self.cont_indent } else { 0 });
        let col = (s..e).find(|&c| self.vcols[c + 1] > target).unwrap_or(e);
        if last_row { col } else { col.min(e.saturating_sub(1)).max(s) }
    }
}

pub struct View<'a> {
    pub buf: &'a mut Buffer,
    pub vim: &'a mut Vim,
    pub settings: &'a Settings,
    /// Whether this view has keyboard focus (shows mode, selection, cursor style).
    pub focused: bool,
    /// Scroll so the cursor is visible.
    pub follow_cursor: bool,
    pub drag_anchor: &'a mut Option<Pos>,
}

impl View<'_> {
    pub fn show(self, ui: &mut Ui, rect: Rect, id: egui::Id) -> Response {
        let View { buf, vim, settings: st, focused, follow_cursor, drag_anchor } = self;
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let p = theme::p();
        painter.rect_filled(rect, 0.0, p.editor_bg);

        let ctx = ui.ctx().clone();
        let font = FontId::monospace(st.font_size);
        let char_w = ctx.fonts_mut(|f| f.glyph_width(&font, 'M'));
        let line_h = (st.font_size * 1.4).round();
        let mode = if focused { vim.mode } else { Mode::Normal };

        let digits = buf.line_count().to_string().len().max(3);
        let show_numbers = st.line_numbers || st.relative_line_numbers;
        let gutter_w = if show_numbers { 18.0 + digits as f32 * char_w + 26.0 } else { 14.0 };
        // The minimap sits between the text and the scrollbar when there is room.
        let minimap = (st.minimap && rect.width() > 460.0).then(|| {
            Rect::from_min_max(pos2(rect.right() - SCROLLBAR_W - MINIMAP_W, rect.top()), pos2(rect.right() - SCROLLBAR_W, rect.bottom()))
        });
        let text_right = minimap.map_or(rect.right(), |m| m.left());
        let text_rect = Rect::from_min_max(pos2(rect.left() + gutter_w, rect.top()), pos2(text_right, rect.bottom()));
        let text_x0 = text_rect.left() + 4.0;
        if focused {
            vim.page_lines = ((rect.height() / line_h) as usize).max(1);
        }

        let wrap_cols = st
            .word_wrap
            .then(|| (((text_rect.width() - SCROLLBAR_W - 8.0) / char_w) as usize).max(8));
        // First display row of each line (plus the total at the end).
        let mut row_of_line = Vec::with_capacity(buf.line_count() + 1);
        let mut rows = 0;
        for l in 0..buf.line_count() {
            row_of_line.push(rows);
            rows += match wrap_cols {
                // Only lines that could be wider than the view need laying out.
                Some(c) if buf.line_len(l) * tab_size() > c => LineLayout::new(buf, l, Some(c)).rows(),
                _ => 1,
            };
        }
        row_of_line.push(rows);
        let line_of_row = |r: usize| row_of_line.partition_point(|&x| x <= r).saturating_sub(1);

        // Scroll: wheel, then keep the cursor in view after keyboard input.
        let max_scroll = rows.saturating_sub(1) as f32 * line_h;
        if ui.rect_contains_pointer(rect) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            buf.scroll_y -= d.y;
            buf.scroll_x -= d.x;
        }
        let cur = buf.cursor;
        let cur_layout = LineLayout::new(buf, cur.line, wrap_cols);
        let cur_row_in_line = cur_layout.row_of(cur.col);
        if follow_cursor {
            let so = (st.scroll_off as f32 * line_h).min(rect.height() / 3.0);
            let cy = (row_of_line[cur.line] + cur_row_in_line) as f32 * line_h;
            if cy - so < buf.scroll_y {
                buf.scroll_y = cy - so;
            } else if cy + line_h + so > buf.scroll_y + rect.height() {
                buf.scroll_y = cy + line_h + so - rect.height();
            }
            if wrap_cols.is_none() {
                let cx = cur_layout.x_col(0, cur.col) as f32 * char_w;
                let text_w = text_rect.width() - 20.0;
                if cx < buf.scroll_x {
                    buf.scroll_x = (cx - text_w / 3.0).max(0.0);
                } else if cx + char_w * 2.0 > buf.scroll_x + text_w {
                    buf.scroll_x = cx + char_w * 2.0 - text_w + text_w / 3.0;
                }
            }
        }
        buf.scroll_y = buf.scroll_y.clamp(0.0, max_scroll);
        buf.scroll_x = if wrap_cols.is_some() { 0.0 } else { buf.scroll_x.max(0.0) };
        let (scroll_x, scroll_y) = (buf.scroll_x, buf.scroll_y);

        // Mouse: click places the cursor, drag selects in visual mode.
        if let Some(ptr) = resp.interact_pointer_pos() {
            let r = (((ptr.y - rect.top() + scroll_y) / line_h).max(0.0) as usize).min(rows.saturating_sub(1));
            let l = line_of_row(r);
            let lay = LineLayout::new(buf, l, wrap_cols);
            let x = ((ptr.x - text_x0 + scroll_x) / char_w).round().max(0.0) as usize;
            let pos = Pos::new(l, lay.col_at((r - row_of_line[l]).min(lay.rows() - 1), x));
            if resp.drag_started() {
                *drag_anchor = Some(pos);
            } else if resp.dragged() {
                if let Some(a) = drag_anchor.filter(|a| *a != pos) {
                    if !matches!(vim.mode, Mode::Visual | Mode::VisualLine) {
                        vim.mode = Mode::Visual;
                        vim.visual_anchor = a;
                    }
                    buf.cursor = pos;
                    buf.clamp_cursor(false);
                }
            }
            if resp.clicked() || resp.drag_started() {
                if vim.mode != Mode::Insert {
                    vim.mode = Mode::Normal;
                }
                buf.cursor = pos;
                buf.clamp_cursor(vim.mode == Mode::Insert);
                buf.want_col = buf.visual_col(pos.line, buf.cursor.col);
            }
        }
        if resp.drag_stopped() {
            *drag_anchor = None;
        }

        let first_row = (scroll_y / line_h) as usize;
        let last_row = ((scroll_y + rect.height()) / line_h).ceil() as usize;
        let text_painter = painter.with_clip_rect(text_rect);
        let selection = if focused { vim.selection(buf) } else { None };
        let search = vim.highlight_regex();
        let lang = syntax::lang_for(&buf.extension());
        let first_line = line_of_row(first_row);

        // Block-comment state at the first visible line.
        let mut in_block = false;
        if let Some(lang) = lang {
            for l in 0..first_line {
                syntax::highlight(lang, &buf.lines[l], &mut in_block);
            }
        }

        let row_y = |r: usize| rect.top() + r as f32 * line_h - scroll_y;
        let cur_line_number = buf.cursor.line;
        let mut l = first_line;
        while l < buf.line_count() && row_of_line[l] <= last_row {
            let line = &buf.lines[l];
            let len = line.len();
            let lay = LineLayout::new(buf, l, wrap_cols);
            let spans = match lang {
                Some(lang) => syntax::highlight(lang, line, &mut in_block),
                None => vec![(0, len, syntax::Tok::Plain)],
            };
            let matches = search.as_ref().map(|re| vim::search_matches(line, re)).unwrap_or_default();
            let y0 = row_y(row_of_line[l]);

            if l == cur_line_number && selection.is_none() {
                let block = Rect::from_min_size(pos2(text_rect.left(), y0), vec2(text_rect.width(), line_h * lay.rows() as f32));
                text_painter.rect_stroke(block.shrink(1.0), 0.0, Stroke::new(2.0, p.current_line), egui::StrokeKind::Inside);
            }
            let indent = if st.indent_guides { indent_width(buf, l) } else { 0 };

            for r in 0..lay.rows() {
                let y = y0 + r as f32 * line_h;
                let (s, e) = lay.range(r);
                let last = r + 1 == lay.rows();
                let x_of = |c: usize| text_x0 + lay.x_col(r, c) as f32 * char_w - scroll_x;

                if let Some((a, b)) = selection.filter(|(a, b)| (a.line..=b.line).contains(&l)) {
                    let (x0, x1) = if mode == Mode::VisualLine {
                        (text_rect.left(), text_rect.right())
                    } else {
                        let sa = if l == a.line { a.col } else { 0 };
                        let sb = if l == b.line { b.col + 1 } else { len + 1 };
                        let (lo, hi) = (sa.max(s), sb.min(if last { len + 1 } else { e }));
                        if lo >= hi { (0.0, 0.0) } else { (x_of(lo), x_of(hi)) }
                    };
                    if x1 > x0 {
                        text_painter.rect_filled(Rect::from_x_y_ranges(x0..=x1, y..=y + line_h), 0.0, p.selection);
                    }
                }
                for &(ms, me) in &matches {
                    let (lo, hi) = (ms.max(s), me.min(e));
                    if lo < hi || (ms == me && ms >= s && ms < e) {
                        let r = Rect::from_x_y_ranges(x_of(lo)..=x_of(hi), y + 1.0..=y + line_h - 1.0);
                        text_painter.rect_filled(r, 2.0, p.search_match);
                        text_painter.rect_stroke(r, 2.0, Stroke::new(1.0, p.search_border), egui::StrokeKind::Inside);
                    }
                }
                for k in 0..indent / tab_size() {
                    let gx = (text_x0 + (k * tab_size()) as f32 * char_w - scroll_x).round() + 0.5;
                    text_painter.line_segment([pos2(gx, y), pos2(gx, y + line_h)], Stroke::new(1.0, p.indent_guide));
                }

                let mut job = LayoutJob::default();
                for &(ss, se, tok) in &spans {
                    let (lo, hi) = (ss.max(s), se.min(e));
                    if lo >= hi {
                        continue;
                    }
                    let mut text = String::new();
                    for c in lo..hi {
                        if line[c] == '\t' {
                            text.extend(std::iter::repeat_n(' ', lay.vcols[c + 1] - lay.vcols[c]));
                        } else {
                            text.push(line[c]);
                        }
                    }
                    job.append(&text, 0.0, TextFormat::simple(font.clone(), tok.color()));
                }
                let g = ctx.fonts_mut(|f| f.layout_job(job));
                let gy = y + (line_h - g.size().y) / 2.0;
                text_painter.galley(pos2(x_of(s), gy), g, p.text);
                if r > 0 {
                    // Wrap marker in the gutter, like a soft line continuation.
                    painter.text(
                        pos2(text_rect.left() - 6.0, y + line_h / 2.0),
                        Align2::RIGHT_CENTER,
                        "↪",
                        FontId::proportional(st.font_size * 0.8),
                        p.line_number.gamma_multiply(0.6),
                    );
                }
            }

            // Line number: absolute, or distance from the cursor line.
            if show_numbers {
                let (num, color) = if l == cur_line_number {
                    (if st.line_numbers { l + 1 } else { 0 }, p.line_number_active)
                } else if st.relative_line_numbers {
                    (l.abs_diff(cur_line_number), p.line_number)
                } else {
                    (l + 1, p.line_number)
                };
                painter.text(
                    pos2(rect.left() + 18.0 + digits as f32 * char_w, y0 + line_h / 2.0),
                    Align2::RIGHT_CENTER,
                    num.to_string(),
                    font.clone(),
                    color,
                );
            }
            l += 1;
        }

        // Cursor.
        let c = buf.cursor;
        let y = row_y(row_of_line[c.line] + cur_row_in_line);
        if y + line_h >= rect.top() && y <= rect.bottom() {
            let line = &buf.lines[c.line];
            let ch = line.get(c.col).copied();
            let cells = cur_layout.vcols.get(c.col + 1).map_or(1, |n| n - cur_layout.vcols[c.col]).max(1);
            let x = text_x0 + cur_layout.x_col(cur_row_in_line, c.col) as f32 * char_w - scroll_x;
            let cell = Rect::from_min_size(pos2(x, y), vec2(char_w * cells as f32, line_h));
            match mode {
                _ if !focused => {
                    text_painter.rect_stroke(cell, 0.0, Stroke::new(1.0, p.cursor.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                }
                Mode::Insert => {
                    text_painter.rect_filled(Rect::from_min_size(pos2(x - 1.0, y), vec2(2.0, line_h)), 0.0, p.cursor);
                }
                Mode::Command | Mode::Search => {
                    text_painter.rect_stroke(cell, 0.0, Stroke::new(1.0, p.cursor), egui::StrokeKind::Inside);
                }
                _ => {
                    text_painter.rect_filled(cell, 0.0, p.cursor);
                    if let Some(ch) = ch.filter(|c| !c.is_whitespace()) {
                        text_painter.text(pos2(x, y + line_h / 2.0), Align2::LEFT_CENTER, ch, font.clone(), p.editor_bg);
                    }
                }
            }
        }

        if let Some(mm) = minimap {
            let visible = (first_line, line_of_row(last_row.min(rows.saturating_sub(1))));
            draw_minimap(ui, &painter, mm, id, buf, lang, visible, line_h, max_scroll, rows, &row_of_line);
        }

        // Vertical scrollbar with a cursor marker, like VS Code's overview ruler.
        let total = max_scroll + rect.height();
        if total > rect.height() + 1.0 {
            let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR_W, rect.top()), rect.max);
            let thumb_h = (rect.height() / total * rect.height()).max(24.0);
            let range = (rect.height() - thumb_h).max(1.0);
            let sresp = ui.interact(track, id.with("vscroll"), Sense::click_and_drag());
            if sresp.dragged() {
                buf.scroll_y = (buf.scroll_y + sresp.drag_delta().y * max_scroll / range).clamp(0.0, max_scroll);
            }
            let ty = rect.top() + buf.scroll_y / max_scroll.max(1.0) * range;
            let thumb = Rect::from_min_size(pos2(track.left(), ty), vec2(SCROLLBAR_W, thumb_h));
            let alpha = if sresp.dragged() || sresp.hovered() { 0x90 } else { 0x50 };
            let sb = p.scrollbar;
            painter.rect_filled(thumb, 0.0, Color32::from_rgba_unmultiplied(sb.r(), sb.g(), sb.b(), alpha));
            let cur_row = row_of_line[c.line] + cur_row_in_line;
            let my = rect.top() + cur_row as f32 / rows.max(1) as f32 * rect.height();
            painter.rect_filled(Rect::from_min_size(pos2(track.left(), my), vec2(SCROLLBAR_W, 2.0)), 0.0, p.cursor);
        }
        resp
    }
}

/// The minimap: every line drawn as thin colored strokes, with a slider over
/// the lines in view. Click to jump, drag the slider to scroll.
#[allow(clippy::too_many_arguments)]
fn draw_minimap(
    ui: &mut Ui,
    painter: &egui::Painter,
    mm: Rect,
    id: egui::Id,
    buf: &mut Buffer,
    lang: Option<&syntax::Lang>,
    visible: (usize, usize),
    line_h: f32,
    max_scroll: f32,
    rows: usize,
    row_of_line: &[usize],
) {
    let p = theme::p();
    let total = buf.line_count();
    let capacity = (mm.height() / MM_ROW) as usize;
    let shown = visible.1 + 1 - visible.0;
    // When the file is taller than the minimap, the minimap scrolls along with the editor.
    let first = if total <= capacity {
        0
    } else {
        let frac = if max_scroll > 0.0 { buf.scroll_y / max_scroll } else { 0.0 };
        ((total - capacity) as f32 * frac).round() as usize
    };

    let resp = ui.interact(mm, id.with("minimap"), Sense::click_and_drag());
    let slider = Rect::from_min_size(
        pos2(mm.left(), mm.top() + visible.0.saturating_sub(first) as f32 * MM_ROW),
        vec2(mm.width(), (shown as f32 * MM_ROW).max(6.0)),
    );
    if resp.dragged() {
        // Dragging the slider by its own travel moves through the whole file.
        let travel = (mm.height().min(total as f32 * MM_ROW) - slider.height()).max(1.0);
        buf.scroll_y = (buf.scroll_y + resp.drag_delta().y / travel * max_scroll).clamp(0.0, max_scroll);
        ui.ctx().request_repaint();
    } else if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let line = (first + ((pos.y - mm.top()) / MM_ROW) as usize).min(total - 1);
            let page_rows = (mm.height() / line_h) as usize;
            let target = row_of_line[line].saturating_sub(page_rows / 2).min(rows.saturating_sub(1));
            buf.scroll_y = (target as f32 * line_h).clamp(0.0, max_scroll);
            ui.ctx().request_repaint();
        }
    }

    let mm_painter = painter.with_clip_rect(mm);
    let mut in_block = false;
    if let Some(lang) = lang {
        for l in 0..first {
            syntax::highlight(lang, &buf.lines[l], &mut in_block);
        }
    }
    let font = FontId::monospace(MM_FONT);
    let max_cols = ((mm.width() - 8.0) / ui.ctx().fonts_mut(|f| f.glyph_width(&font, 'M'))) as usize;
    for (i, l) in (first..total.min(first + capacity)).enumerate() {
        let line = &buf.lines[l];
        let y = mm.top() + i as f32 * MM_ROW;
        if l == buf.cursor.line {
            mm_painter.rect_filled(Rect::from_min_size(pos2(mm.left(), y), vec2(mm.width(), MM_ROW)), 0.0, p.cursor.gamma_multiply(0.25));
        }
        if line.iter().all(|c| c.is_whitespace()) {
            if let Some(lang) = lang {
                syntax::highlight(lang, line, &mut in_block);
            }
            continue;
        }
        let spans = match lang {
            Some(lang) => syntax::highlight(lang, line, &mut in_block),
            None => vec![(0, line.len(), syntax::Tok::Plain)],
        };
        // Real glyphs in a tiny monospace font: reads as miniature code.
        let mut job = LayoutJob::default();
        let mut col = 0;
        for (s, e, tok) in spans {
            let mut text = String::new();
            for &c in &line[s..e] {
                if col >= max_cols {
                    break;
                }
                let w = if c == '\t' { tab_size() - col % tab_size() } else { 1 };
                if c == '\t' {
                    text.extend(std::iter::repeat_n(' ', w));
                } else {
                    text.push(c);
                }
                col += w;
            }
            job.append(&text, 0.0, TextFormat::simple(font.clone(), tok.color().gamma_multiply(0.9)));
        }
        let g = ui.ctx().fonts_mut(|f| f.layout_job(job));
        mm_painter.galley(pos2(mm.left() + 6.0, y + MM_ROW / 2.0 - g.size().y / 2.0), g, p.text);
    }

    // Like VS Code, the slider only shows while the pointer is over the minimap.
    if resp.hovered() || resp.dragged() {
        let alpha = if resp.dragged() { 0.16 } else { 0.10 };
        let shade = if p.dark { Color32::WHITE } else { Color32::BLACK };
        mm_painter.rect_filled(slider, 0.0, shade.gamma_multiply(alpha));
    }
}

/// Visual width of a line's indentation; blank lines borrow it from neighbors
/// so indent guides run through them.
fn indent_width(buf: &Buffer, l: usize) -> usize {
    let measure = |l: usize| -> Option<usize> {
        let line = &buf.lines[l];
        let n = line.iter().take_while(|c| c.is_whitespace()).count();
        (n < line.len()).then(|| buf.visual_col(l, n))
    };
    if let Some(w) = measure(l) {
        return w;
    }
    let prev = (l.saturating_sub(50)..l).rev().find_map(measure).unwrap_or(0);
    let next = (l + 1..(l + 50).min(buf.line_count())).find_map(measure).unwrap_or(0);
    prev.min(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(text: &str, cols: usize) -> (Buffer, LineLayout) {
        let b = Buffer::from_str(text);
        let lay = LineLayout::new(&b, 0, Some(cols));
        (b, lay)
    }

    #[test]
    fn short_lines_do_not_wrap() {
        let (_, lay) = layout("hello", 10);
        assert_eq!(lay.starts, vec![0]);
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let (_, lay) = layout("the quick brown fox jumps", 10);
        // "the quick " | "brown fox " | "jumps"
        assert_eq!(lay.starts, vec![0, 10, 20]);
        assert_eq!(lay.range(1), (10, 20));
        assert_eq!(lay.row_of(12), 1);
        assert_eq!(lay.row_of(24), 2);
    }

    #[test]
    fn long_words_break_mid_word() {
        let (_, lay) = layout("abcdefghijklmnop", 5);
        assert_eq!(lay.starts, vec![0, 5, 10, 15]);
    }

    #[test]
    fn continuation_rows_keep_indent() {
        let (_, lay) = layout("    aaaa bbbb cccc dddd", 12);
        assert_eq!(lay.cont_indent, 4);
        // Continuation rows have 12 - 4 columns available.
        assert_eq!(lay.starts, vec![0, 9, 14, 19]);
        // First char of a continuation row is drawn after the indent.
        assert_eq!(lay.x_col(1, 9), 4);
        assert_eq!(lay.col_at(1, 4), 9);
        assert_eq!(lay.col_at(1, 0), 9);
    }

    #[test]
    fn col_at_stays_in_row() {
        let (_, lay) = layout("the quick brown fox", 10);
        assert_eq!(lay.col_at(0, 50), 9);
        assert_eq!(lay.col_at(1, 50), 19); // last row may sit past the end
    }
}
