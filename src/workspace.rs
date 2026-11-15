//! Open buffers and the editor groups (split panes) that show them. Each
//! group has its own tabs, and each tab its own cursor and scroll position,
//! so one file can be open in two panes at different places.
//!
//! Invariant: the focused tab's view state lives in its `Buffer` (where the
//! vim engine edits it); every other tab keeps its state in `Tab::view`.

use std::path::Path;

use crate::buffer::{Buffer, Pos};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewState {
    pub cursor: Pos,
    pub want_col: usize,
    pub scroll_y: f32,
    pub scroll_x: f32,
}

impl ViewState {
    fn take(buf: &Buffer) -> Self {
        Self { cursor: buf.cursor, want_col: buf.want_col, scroll_y: buf.scroll_y, scroll_x: buf.scroll_x }
    }

    /// Install into `buf`; `clamp` keeps the cursor inside the (possibly edited) text.
    fn put(self, buf: &mut Buffer, clamp: bool) {
        buf.cursor = self.cursor;
        buf.want_col = self.want_col;
        buf.scroll_y = self.scroll_y;
        buf.scroll_x = self.scroll_x;
        if clamp {
            buf.clamp_cursor(false);
        }
    }
}

pub struct Tab {
    pub buf: usize,
    pub view: ViewState,
}

#[derive(Default)]
pub struct Group {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

pub struct Workspace {
    pub buffers: Vec<Buffer>,
    /// Never empty; a lone group may have no tabs.
    pub groups: Vec<Group>,
    pub focus: usize,
}

impl Default for Workspace {
    fn default() -> Self {
        Self { buffers: Vec::new(), groups: vec![Group::default()], focus: 0 }
    }
}

impl Workspace {
    /// Buffer index of the focused tab.
    pub fn current(&self) -> Option<usize> {
        let g = &self.groups[self.focus];
        g.tabs.get(g.active).map(|t| t.buf)
    }

    pub fn current_buf(&self) -> Option<&Buffer> {
        self.current().map(|b| &self.buffers[b])
    }

    #[cfg(test)]
    pub fn current_buf_mut(&mut self) -> Option<&mut Buffer> {
        self.current().map(|b| &mut self.buffers[b])
    }

    pub fn find(&self, path: &Path) -> Option<usize> {
        self.buffers.iter().position(|b| b.path.as_deref() == Some(path))
    }

    /// Save the focused tab's live state into its `Tab`.
    fn store(&mut self) {
        let g = &mut self.groups[self.focus];
        if let Some(t) = g.tabs.get_mut(g.active) {
            t.view = ViewState::take(&self.buffers[t.buf]);
        }
    }

    /// Install the focused tab's state into its buffer.
    fn load(&mut self) {
        let g = &self.groups[self.focus];
        if let Some(t) = g.tabs.get(g.active) {
            t.view.put(&mut self.buffers[t.buf], true);
        }
    }

    /// Show buffer `b` in the focused group, reusing its tab there if it has one.
    pub fn show(&mut self, b: usize) {
        self.store();
        let g = &mut self.groups[self.focus];
        match g.tabs.iter().position(|t| t.buf == b) {
            Some(i) => g.active = i,
            None => {
                g.tabs.push(Tab { buf: b, view: ViewState::default() });
                g.active = g.tabs.len() - 1;
            }
        }
        self.load();
    }

    pub fn add_buffer(&mut self, buf: Buffer) -> usize {
        self.buffers.push(buf);
        let b = self.buffers.len() - 1;
        self.show(b);
        b
    }

    pub fn activate(&mut self, group: usize, tab: usize) {
        if group >= self.groups.len() || tab >= self.groups[group].tabs.len() {
            return;
        }
        self.store();
        self.focus = group;
        self.groups[group].active = tab;
        self.load();
    }

    pub fn focus_group(&mut self, group: usize) {
        if group < self.groups.len() && group != self.focus {
            self.store();
            self.focus = group;
            self.load();
        }
    }

    /// Move focus `delta` groups left/right, optionally wrapping around.
    pub fn focus_step(&mut self, delta: isize, wrap: bool) {
        let n = self.groups.len() as isize;
        let target = self.focus as isize + delta;
        let target = if wrap { target.rem_euclid(n) } else { target.clamp(0, n - 1) };
        self.focus_group(target as usize);
    }

    pub fn cycle_tab(&mut self, delta: isize) {
        let g = &self.groups[self.focus];
        let n = g.tabs.len() as isize;
        if n > 1 {
            let i = (g.active as isize + delta).rem_euclid(n) as usize;
            self.activate(self.focus, i);
        }
    }

    fn views_of(&self, b: usize) -> usize {
        self.groups.iter().flat_map(|g| &g.tabs).filter(|t| t.buf == b).count()
    }

    /// Close a tab. A buffer goes away with its last tab, which needs `force`
    /// if it has unsaved changes. An emptied group is removed unless it is the last.
    pub fn close_tab(&mut self, group: usize, tab: usize, force: bool) -> Result<(), String> {
        let Some(b) = self.groups.get(group).and_then(|g| g.tabs.get(tab)).map(|t| t.buf) else {
            return Ok(());
        };
        let last_view = self.views_of(b) == 1;
        if last_view && self.buffers[b].dirty && !force {
            return Err("E37: No write since last change (add ! to override)".into());
        }
        self.store();
        let g = &mut self.groups[group];
        g.tabs.remove(tab);
        if g.active > tab || g.active >= g.tabs.len() {
            g.active = g.active.saturating_sub(1);
        }
        if g.tabs.is_empty() && self.groups.len() > 1 {
            self.groups.remove(group);
            if self.focus > group || self.focus >= self.groups.len() {
                self.focus = self.focus.saturating_sub(1);
            }
        }
        if last_view {
            self.remove_buffer(b);
        }
        self.load();
        Ok(())
    }

    /// Remove the focused group if it has no tabs (and is not the only one).
    pub fn close_empty_group(&mut self) -> bool {
        if self.groups.len() > 1 && self.groups[self.focus].tabs.is_empty() {
            self.groups.remove(self.focus);
            self.focus = self.focus.min(self.groups.len() - 1);
            self.load();
            true
        } else {
            false
        }
    }

    fn remove_buffer(&mut self, b: usize) {
        self.buffers.remove(b);
        for t in self.groups.iter_mut().flat_map(|g| &mut g.tabs) {
            if t.buf > b {
                t.buf -= 1;
            }
        }
    }

    /// Split the focused group: a new group to its right showing the same tab.
    pub fn split(&mut self) {
        self.store();
        let copy = {
            let g = &self.groups[self.focus];
            g.tabs.get(g.active).map(|t| Tab { buf: t.buf, view: t.view })
        };
        let group = Group { tabs: copy.into_iter().collect(), active: 0 };
        self.groups.insert(self.focus + 1, group);
        self.focus += 1;
        self.load();
    }

    /// Close every group but the focused one.
    pub fn only(&mut self, force: bool) -> Result<(), String> {
        let keep: Vec<usize> = self.groups[self.focus].tabs.iter().map(|t| t.buf).collect();
        let dropped: Vec<usize> = (0..self.buffers.len()).filter(|b| !keep.contains(b)).collect();
        if !force {
            if let Some(&b) = dropped.iter().find(|&&b| self.buffers[b].dirty) {
                return Err(format!("E445: Other window contains changes: \"{}\"", self.buffers[b].name()));
            }
        }
        self.store();
        let g = self.groups.swap_remove(self.focus);
        self.groups = vec![g];
        self.focus = 0;
        for &b in dropped.iter().rev() {
            self.remove_buffer(b);
        }
        self.load();
        Ok(())
    }

    /// Run `f` on group `g`'s active buffer with that tab's view state
    /// installed; `f` also learns whether the group is focused.
    pub fn with_view<R>(&mut self, group: usize, f: impl FnOnce(&mut Buffer, bool) -> R) -> Option<R> {
        let focused = group == self.focus;
        let g = self.groups.get_mut(group)?;
        let t = g.tabs.get_mut(g.active)?;
        let buf = &mut self.buffers[t.buf];
        if focused {
            return Some(f(buf, true));
        }
        let live = ViewState::take(buf);
        t.view.put(buf, true);
        let r = f(buf, false);
        t.view = ViewState::take(buf);
        live.put(buf, false);
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_with(texts: &[&str]) -> Workspace {
        let mut ws = Workspace::default();
        for (i, t) in texts.iter().enumerate() {
            let mut b = Buffer::from_str(t);
            b.path = Some(format!("/f{i}").into());
            ws.add_buffer(b);
        }
        ws
    }

    #[test]
    fn split_keeps_independent_cursors() {
        let mut ws = ws_with(&["a\nb\nc\nd"]);
        ws.current_buf_mut().unwrap().cursor = Pos::new(3, 0);
        ws.split();
        assert_eq!(ws.groups.len(), 2);
        assert_eq!(ws.focus, 1);
        // The new pane starts where the old one was, then moves on its own.
        assert_eq!(ws.current_buf().unwrap().cursor, Pos::new(3, 0));
        ws.current_buf_mut().unwrap().cursor = Pos::new(1, 0);
        ws.focus_group(0);
        assert_eq!(ws.current_buf().unwrap().cursor, Pos::new(3, 0));
        ws.focus_group(1);
        assert_eq!(ws.current_buf().unwrap().cursor, Pos::new(1, 0));
    }

    #[test]
    fn with_view_renders_other_pane_without_disturbing_focus() {
        let mut ws = ws_with(&["abc\ndef"]);
        ws.split();
        ws.current_buf_mut().unwrap().cursor = Pos::new(1, 3); // insert-mode position past the end
        ws.groups[0].tabs[0].view.cursor = Pos::new(0, 1);
        let seen = ws.with_view(0, |b, focused| (b.cursor, focused)).unwrap();
        assert_eq!(seen, (Pos::new(0, 1), false));
        assert_eq!(ws.current_buf().unwrap().cursor, Pos::new(1, 3), "focused state restored unclamped");
    }

    #[test]
    fn closing_last_view_of_dirty_buffer_needs_force() {
        let mut ws = ws_with(&["x"]);
        ws.buffers[0].dirty = true;
        ws.split();
        // Another view still shows it, so this close is fine.
        ws.close_tab(1, 0, false).unwrap();
        assert_eq!(ws.groups.len(), 1, "emptied group is removed");
        assert!(ws.close_tab(0, 0, false).is_err());
        ws.close_tab(0, 0, true).unwrap();
        assert!(ws.buffers.is_empty());
        assert_eq!(ws.groups.len(), 1);
    }

    #[test]
    fn removing_a_buffer_reindexes_tabs() {
        let mut ws = ws_with(&["a", "b", "c"]);
        ws.close_tab(0, 0, false).unwrap();
        let names: Vec<_> = ws.groups[0].tabs.iter().map(|t| ws.buffers[t.buf].text()).collect();
        assert_eq!(names, ["b", "c"]);
    }

    #[test]
    fn only_refuses_to_drop_unsaved_buffers() {
        let mut ws = ws_with(&["a"]);
        ws.split();
        let b = Buffer::from_str("new");
        ws.add_buffer(b);
        ws.buffers[1].dirty = true;
        ws.focus_group(0);
        assert!(ws.only(false).is_err());
        ws.only(true).unwrap();
        assert_eq!(ws.groups.len(), 1);
        assert_eq!(ws.buffers.len(), 1);
    }

    #[test]
    fn focus_step_clamps_or_wraps() {
        let mut ws = ws_with(&["a"]);
        ws.split();
        ws.split();
        ws.focus_step(1, false);
        assert_eq!(ws.focus, 2);
        ws.focus_step(1, true);
        assert_eq!(ws.focus, 0);
        ws.focus_step(-1, false);
        assert_eq!(ws.focus, 0);
    }
}
