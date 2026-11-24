//! Find in files: the Search view in the sidebar. The search itself runs on a
//! background thread and streams results back file by file, so typing in the
//! box never waits for it; starting a new search cancels the old one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui};
use regex::{Regex, RegexBuilder};

use crate::explorer;
use crate::icons::*;
use crate::theme;

/// Files larger than this are skipped.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Stop after this many matches; the count then ends in "+".
const MAX_MATCHES: usize = 5_000;
/// Characters of context shown before a match.
const CONTEXT: usize = 28;
/// Height of a result row.
const ROW_H: f32 = 22.0;
/// Wait this long after typing stops before searching.
const DEBOUNCE: f64 = 0.18;

#[derive(Clone, Default, PartialEq)]
pub struct Query {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl Query {
    /// The compiled pattern, or why it is not valid.
    pub fn build(&self) -> Result<Regex, String> {
        let body = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        let body = if self.whole_word { format!(r"\b(?:{body})\b") } else { body };
        RegexBuilder::new(&body)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| e.to_string().lines().last().unwrap_or("invalid pattern").trim().to_string())
    }

    /// The same search as a vim pattern, to highlight the matches in the editor.
    pub fn vim_pattern(&self) -> String {
        let body = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        let body = if self.whole_word { format!(r"\b(?:{body})\b") } else { body };
        if self.case_sensitive { body } else { format!("(?i){body}") }
    }
}

#[derive(Clone)]
pub struct Match {
    pub line: usize,
    /// Char column where the match starts.
    pub col: usize,
    /// The line around the match, and where the match sits in it (chars).
    pub preview: String,
    pub hl: (usize, usize),
}

pub struct FileResult {
    /// Relative to the search root.
    pub path: PathBuf,
    pub matches: Vec<Match>,
    pub collapsed: bool,
}

enum Msg {
    File(u64, FileResult),
    Done(u64, bool),
}

/// Where a clicked result points.
pub struct Jump {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
}

pub struct SearchView {
    pub query: Query,
    /// The query the results are for.
    searched: Option<Query>,
    pub results: Vec<FileResult>,
    error: Option<String>,
    running: bool,
    truncated: bool,
    generation: Arc<AtomicU64>,
    jobs: Sender<(u64, PathBuf, Regex)>,
    rx: Receiver<Msg>,
    /// When the query last changed; the search starts after a short pause.
    edited_at: Option<f64>,
    /// Ask the text box to take keyboard focus next frame.
    pub focus_requested: bool,
    /// Whether the text box has keyboard focus (keys then skip the editor).
    pub has_focus: bool,
}

impl SearchView {
    pub fn new(notify: impl Fn() + Send + 'static) -> Self {
        let generation = Arc::new(AtomicU64::new(0));
        let (jobs, job_rx) = mpsc::channel::<(u64, PathBuf, Regex)>();
        let (tx, rx) = mpsc::channel();
        let current = generation.clone();
        std::thread::Builder::new()
            .name("vex-search".into())
            .spawn(move || {
                while let Ok(mut job) = job_rx.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = job_rx.try_recv() {
                        job = newer;
                    }
                    let (generation_id, root, re) = job;
                    let truncated = run(&root, &re, &|| current.load(Ordering::Relaxed) != generation_id, &mut |file| {
                        let _ = tx.send(Msg::File(generation_id, file));
                        notify();
                    });
                    let _ = tx.send(Msg::Done(generation_id, truncated));
                    notify();
                }
            })
            .expect("can start the search thread");
        Self {
            query: Query::default(),
            searched: None,
            results: Vec::new(),
            error: None,
            running: false,
            truncated: false,
            generation,
            jobs,
            rx,
            edited_at: None,
            focus_requested: false,
            has_focus: false,
        }
    }

    /// Search now (results replace the current ones as they arrive).
    pub fn start(&mut self, root: &Path) {
        self.edited_at = None;
        let generation_id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.results.clear();
        self.truncated = false;
        self.searched = Some(self.query.clone());
        if self.query.text.is_empty() {
            self.error = None;
            self.running = false;
            return;
        }
        match self.query.build() {
            Ok(re) => {
                self.error = None;
                self.running = true;
                let _ = self.jobs.send((generation_id, root.to_path_buf(), re));
            }
            Err(e) => {
                self.error = Some(e);
                self.running = false;
            }
        }
    }

    pub fn match_count(&self) -> usize {
        self.results.iter().map(|f| f.matches.len()).sum()
    }

    fn receive(&mut self) {
        let current = self.generation.load(Ordering::Relaxed);
        for msg in self.rx.try_iter() {
            match msg {
                Msg::File(generation_id, file) if generation_id == current => self.results.push(file),
                Msg::Done(generation_id, truncated) if generation_id == current => {
                    self.running = false;
                    self.truncated = truncated;
                }
                _ => {}
            }
        }
    }

    /// Draw the view; returns the result the user clicked.
    pub fn show(&mut self, ui: &mut Ui, root: &Path) -> Option<Jump> {
        self.receive();
        let p = theme::p();
        let now = ui.input(|i| i.time);
        if let Some(t) = self.edited_at {
            if now - t >= DEBOUNCE {
                self.start(root);
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(DEBOUNCE));
            }
        }

        let w = ui.available_width();
        let header = sidebar_header(ui, "SEARCH");
        let all_collapsed = !self.results.is_empty() && self.results.iter().all(|f| f.collapsed);
        let buttons: [(&str, &str, fn(&egui::Painter, egui::Pos2, Color32)); 2] = [
            ("search-refresh", "Search Again", draw_refresh_icon),
            ("search-collapse", if all_collapsed { "Expand All" } else { "Collapse All" }, draw_collapse_icon),
        ];
        for (i, (id, tip, draw)) in buttons.into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(header.right() - 22.0 - i as f32 * 26.0, header.center().y), vec2(22.0, 22.0));
            if header_button(ui, r, id, tip, draw) {
                if i == 0 {
                    self.start(root);
                } else {
                    for f in &mut self.results {
                        f.collapsed = !all_collapsed;
                    }
                }
            }
        }

        // The query box, with the three toggles inside its right end.
        let (row, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
        let field = Rect::from_min_max(pos2(row.left() + 12.0, row.top() + 2.0), pos2(row.right() - 12.0, row.bottom() - 2.0));
        let toggles_w = 3.0 * 22.0 + 6.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field));
        let edit = egui::TextEdit::singleline(&mut self.query.text)
            .id_salt("search-query")
            .hint_text("Search")
            .font(FontId::proportional(13.0))
            .margin(egui::Margin { left: 8, right: toggles_w as i8, top: 5, bottom: 5 })
            .desired_width(field.width());
        let resp = child.add(edit);
        // Focus moves on the next frame; count the box as focused until then,
        // so the keys typed meanwhile do not reach the editor.
        let requested = std::mem::take(&mut self.focus_requested);
        if requested {
            resp.request_focus();
        }
        self.has_focus = resp.has_focus() || requested;
        if resp.changed() {
            self.edited_at = Some(now);
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.start(root);
            resp.request_focus();
        }
        let flags: [(&str, &str, bool); 3] = [
            ("Aa", "Match Case", self.query.case_sensitive),
            ("ab", "Match Whole Word", self.query.whole_word),
            (".*", "Use Regular Expression", self.query.regex),
        ];
        let mut toggled = None;
        for (i, (label, tip, on)) in flags.into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(field.right() - 14.0 - (2 - i) as f32 * 22.0, field.center().y), vec2(20.0, 20.0));
            let t = ui.interact(r, ui.scope_id().with(("search-flag", i)), Sense::click());
            if on {
                ui.painter().rect(r, CornerRadius::same(4), p.focus_border.gamma_multiply(0.25), Stroke::new(1.0, p.focus_border), egui::StrokeKind::Inside);
            } else if t.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(4), p.list_hover);
            }
            let color = if on { p.text_bright } else { p.text_muted };
            let font = if i == 1 { FontId::proportional(11.5) } else { FontId::monospace(11.5) };
            let text = ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, color);
            if i == 1 {
                // "ab" with a bar under it, like VS Code's whole-word icon.
                let y = text.bottom() + 0.5;
                ui.painter().line_segment([pos2(text.left() - 1.0, y), pos2(text.right() + 1.0, y)], Stroke::new(1.0, color));
            }
            if t.on_hover_text(tip).clicked() {
                toggled = Some(i);
            }
        }
        if let Some(i) = toggled {
            match i {
                0 => self.query.case_sensitive = !self.query.case_sensitive,
                1 => self.query.whole_word = !self.query.whole_word,
                _ => self.query.regex = !self.query.regex,
            }
            self.start(root);
            self.focus_requested = true;
        }

        // Summary line.
        let (row, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        let summary = if let Some(e) = &self.error {
            Some((e.clone(), p.error))
        } else if self.searched.as_ref().is_some_and(|q| !q.text.is_empty()) {
            let n = self.match_count();
            let files = self.results.len();
            let text = if n == 0 && self.running {
                "Searching…".to_string()
            } else if n == 0 {
                "No results found".to_string()
            } else {
                format!(
                    "{n}{} result{} in {files} file{}{}",
                    if self.truncated { "+" } else { "" },
                    if n == 1 { "" } else { "s" },
                    if files == 1 { "" } else { "s" },
                    if self.running { "…" } else { "" },
                )
            };
            Some((text, p.text_muted))
        } else {
            None
        };
        if let Some((text, color)) = summary {
            let galley = ui.painter().layout(text, FontId::proportional(12.0), color, row.width() - 26.0);
            ui.painter().galley(pos2(row.left() + 14.0, row.center().y - galley.size().y / 2.0), galley, color);
        }

        // One flat list of rows (file headers and their matches), so only the
        // rows in view are laid out, however many results there are.
        let rows: Vec<(usize, Option<usize>)> = self
            .results
            .iter()
            .enumerate()
            .flat_map(|(fi, f)| {
                let matches = if f.collapsed { 0 } else { f.matches.len() };
                std::iter::once((fi, None)).chain((0..matches).map(move |mi| (fi, Some(mi))))
            })
            .collect();
        let mut jump = None;
        let mut toggle = None;
        let highlight = p.search_match;
        egui::ScrollArea::vertical().id_salt("search-results").auto_shrink(false).show_rows(ui, ROW_H, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &(fi, mi) in &rows[range] {
                let file = &self.results[fi];
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                let painter = ui.painter();
                if resp.hovered() {
                    painter.rect_filled(r.shrink2(vec2(6.0, 0.5)), CornerRadius::same(5), p.list_hover);
                }
                let Some(mi) = mi else {
                    let name = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    draw_chevron(painter, pos2(r.left() + 17.0, r.center().y), !file.collapsed, p.text_muted);
                    draw_file_icon(painter, pos2(r.left() + 33.0, r.center().y), theme::file_color(&name));
                    let nr = painter.text(pos2(r.left() + 45.0, r.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(13.0), p.text_bright);
                    // The count, as a pill at the right end.
                    let count = painter.layout_no_wrap(file.matches.len().to_string(), FontId::proportional(11.0), p.text_bright);
                    let pill = Rect::from_center_size(
                        pos2(r.right() - 18.0 - count.size().x / 2.0, r.center().y),
                        vec2((count.size().x + 10.0).max(18.0), 16.0),
                    );
                    if let Some(dir) = file.path.parent().filter(|d| !d.as_os_str().is_empty()) {
                        let clip = Rect::from_min_max(pos2(nr.right() + 6.0, r.top()), pos2(pill.left() - 8.0, r.bottom()));
                        if clip.width() > 20.0 {
                            let g = painter.layout_no_wrap(dir.to_string_lossy().into_owned(), FontId::proportional(12.0), p.text_muted);
                            painter.with_clip_rect(clip).galley(pos2(clip.left(), r.center().y - g.size().y / 2.0), g, p.text_muted);
                        }
                    }
                    painter.rect_filled(pill, CornerRadius::same(8), p.input_bg);
                    painter.galley(pill.center() - count.size() / 2.0, count, p.text_bright);
                    if resp.clicked() {
                        toggle = Some(fi);
                    }
                    continue;
                };
                let m = &file.matches[mi];
                let x = r.left() + 45.0;
                let font = FontId::proportional(13.0);
                let chars: Vec<char> = m.preview.chars().collect();
                let part = |a: usize, b: usize| chars[a.min(chars.len())..b.min(chars.len())].iter().collect::<String>();
                // Drop context from the front until the match sits in the first
                // third of the row, so it is never cut off on the right.
                let room = (r.right() - 10.0 - x) * 0.35;
                let mut from = 0;
                while from < m.hl.0 && painter.layout_no_wrap(part(from, m.hl.0), font.clone(), p.text_ui).size().x > room {
                    from += ((m.hl.0 - from) / 4).max(1);
                }
                let lead = if from > 0 && !part(from, m.hl.0).is_empty() { "…" } else { "" };
                let fmt = |color: Color32, bg: Color32| egui::TextFormat { font_id: font.clone(), color, background: bg, ..Default::default() };
                let mut job = egui::text::LayoutJob::default();
                job.append(&format!("{lead}{}", part(from, m.hl.0).trim_start_matches('…')), 0.0, fmt(p.text_ui, Color32::TRANSPARENT));
                job.append(&part(m.hl.0, m.hl.1), 0.0, fmt(p.text_bright, highlight));
                job.append(&part(m.hl.1, chars.len()), 0.0, fmt(p.text_ui, Color32::TRANSPARENT));
                let g = ui.ctx().fonts_mut(|f| f.layout_job(job));
                let clip = Rect::from_min_max(pos2(x, r.top()), pos2(r.right() - 10.0, r.bottom()));
                painter.with_clip_rect(clip).galley(pos2(x, r.center().y - g.size().y / 2.0), g, p.text_ui);
                if resp.on_hover_text_at_pointer(format!("{}:{}", file.path.display(), m.line + 1)).clicked() {
                    jump = Some(Jump { path: file.path.clone(), line: m.line, col: m.col });
                }
            }
        });
        if let Some(fi) = toggle {
            self.results[fi].collapsed = !self.results[fi].collapsed;
        }
        jump
    }
}

/// Search every file under `root`; calls `found` for each file with matches.
/// Returns whether it stopped early at `MAX_MATCHES`.
fn run(root: &Path, re: &Regex, cancelled: &dyn Fn() -> bool, found: &mut dyn FnMut(FileResult)) -> bool {
    let mut total = 0;
    for rel in explorer::all_files(root) {
        if cancelled() {
            return false;
        }
        let path = root.join(&rel);
        if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            continue; // binary
        }
        let text = String::from_utf8_lossy(&bytes);
        let matches = search_text(&text, re, MAX_MATCHES - total);
        if matches.is_empty() {
            continue;
        }
        total += matches.len();
        found(FileResult { path: rel, matches, collapsed: false });
        if total >= MAX_MATCHES {
            return true;
        }
    }
    false
}

/// Matches of `re` in `text`, one entry per match, at most `limit`.
pub fn search_text(text: &str, re: &Regex, limit: usize) -> Vec<Match> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        for m in re.find_iter(line) {
            if m.start() == m.end() {
                continue; // empty matches (like `^`) are not useful here
            }
            if out.len() >= limit {
                return out;
            }
            let col = line[..m.start()].chars().count();
            let len = line[m.start()..m.end()].chars().count();
            // Preview: some context before the match, leading spaces dropped.
            let chars: Vec<char> = line.chars().collect();
            let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
            let from = col.saturating_sub(CONTEXT).max(indent.min(col));
            let mut preview: String = if from > indent { "…".into() } else { String::new() };
            let lead = preview.chars().count();
            preview.extend(chars[from..chars.len().min(col + len + 120)].iter().map(|&c| if c == '\t' { ' ' } else { c }));
            let start = lead + col - from;
            out.push(Match { line: line_no, col, preview, hl: (start, start + len) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Query {
        Query { text: text.into(), ..Default::default() }
    }

    #[test]
    fn finds_matches_with_columns_and_preview() {
        let re = q("needle").build().unwrap();
        let m = search_text("a\n    let x = Needle;\nneedle needle\n", &re, 100);
        assert_eq!(m.len(), 3, "case-insensitive by default");
        assert_eq!((m[0].line, m[0].col), (1, 12));
        assert_eq!(m[0].preview, "let x = Needle;");
        assert_eq!(m[0].hl, (8, 14));
        assert_eq!((m[2].line, m[2].col), (2, 7));
    }

    #[test]
    fn options_change_what_matches() {
        let text = "Foo foo food\n";
        let case = Query { case_sensitive: true, ..q("foo") }.build().unwrap();
        assert_eq!(search_text(text, &case, 100).len(), 2);
        let word = Query { whole_word: true, ..q("foo") }.build().unwrap();
        assert_eq!(search_text(text, &word, 100).len(), 2);
        let literal = q("f.o").build().unwrap();
        assert_eq!(search_text(text, &literal, 100).len(), 0, "not a regex unless asked");
        let re = Query { regex: true, ..q("f.o") }.build().unwrap();
        assert_eq!(search_text(text, &re, 100).len(), 3);
        assert!(Query { regex: true, ..q("(") }.build().is_err());
    }

    #[test]
    fn long_lines_get_trimmed_context() {
        let line = format!("{}needle", "x".repeat(100));
        let m = search_text(&line, &q("needle").build().unwrap(), 10);
        assert!(m[0].preview.starts_with('…'));
        assert_eq!(m[0].preview.chars().count(), 1 + CONTEXT + 6);
        let (a, b) = m[0].hl;
        assert_eq!(m[0].preview.chars().skip(a).take(b - a).collect::<String>(), "needle");
    }
}
