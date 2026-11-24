//! Remember the open files per project folder, and bring them back next time:
//! the panes, their tabs, and where the cursor was in each.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::buffer::Pos;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Session {
    pub focus: usize,
    #[serde(default)]
    pub groups: Vec<GroupState>,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct GroupState {
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<TabState>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct TabState {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    #[serde(default)]
    pub scroll: f32,
}

impl TabState {
    pub fn pos(&self) -> Pos {
        Pos::new(self.line, self.col)
    }
}

/// Where the session for `root` is kept: one small file per folder, named by
/// a hash of its path.
fn file_for(root: &Path) -> Option<PathBuf> {
    // FNV-1a: stable across Rust versions, unlike the std hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    Some(crate::settings::config_dir()?.join("sessions").join(format!("{h:016x}.toml")))
}

pub fn load(root: &Path) -> Option<Session> {
    let text = std::fs::read_to_string(file_for(root)?).ok()?;
    toml::from_str(&text).ok()
}

pub fn save(root: &Path, session: &Session) -> std::io::Result<()> {
    let Some(path) = file_for(root) else { return Ok(()) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(session).map_err(std::io::Error::other)?;
    std::fs::write(path, format!("# Open files in {}, for reopening the folder.\n{text}", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let s = Session {
            focus: 1,
            groups: vec![
                GroupState { active: 0, tabs: vec![TabState { path: "/a/b.rs".into(), line: 3, col: 4, scroll: 20.0 }] },
                GroupState { active: 1, tabs: vec![] },
            ],
        };
        let text = toml::to_string(&s).unwrap();
        assert_eq!(toml::from_str::<Session>(&text).unwrap(), s);
    }
}
