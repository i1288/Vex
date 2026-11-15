//! File tree for the sidebar and the file list for quick open.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const IGNORED_DIRS: [&str; 6] = [".git", "target", "node_modules", ".venv", "__pycache__", ".cache"];
const MAX_FILES: usize = 20_000;

#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

pub struct Row {
    pub entry: Entry,
    pub depth: usize,
    pub expanded: bool,
}

pub struct Explorer {
    pub root: PathBuf,
    expanded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, Vec<Entry>>,
}

impl Explorer {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            expanded: HashSet::new(),
            cache: HashMap::new(),
        }
    }

    pub fn root_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn refresh(&mut self) {
        self.cache.clear();
    }

    pub fn toggle(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
        }
    }

    /// Expand every folder between the root and `path` so it shows in the tree.
    pub fn reveal(&mut self, path: &Path) {
        let mut p = path.parent();
        while let Some(dir) = p {
            if !dir.starts_with(&self.root) || dir == self.root {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            p = dir.parent();
        }
    }

    fn children(&mut self, dir: &Path) -> &[Entry] {
        self.cache.entry(dir.to_path_buf()).or_insert_with(|| read_dir_sorted(dir))
    }

    /// The tree flattened into the rows currently visible.
    pub fn rows(&mut self) -> Vec<Row> {
        let mut out = Vec::new();
        let root = self.root.clone();
        self.collect(&root, 0, &mut out);
        out
    }

    fn collect(&mut self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let entries = self.children(dir).to_vec();
        for entry in entries {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            let path = entry.path.clone();
            out.push(Row { entry, depth, expanded });
            if expanded {
                self.collect(&path, depth + 1, out);
            }
        }
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<Entry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir() || e.path().is_dir();
            Some(Entry { path: e.path(), name, is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries
}

/// All files under `root` (skipping build and VCS folders), as paths relative to it.
pub fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in read_dir_sorted(&dir) {
            if out.len() >= MAX_FILES {
                return out;
            }
            if entry.is_dir {
                if !IGNORED_DIRS.contains(&entry.name.as_str()) {
                    stack.push(entry.path);
                }
            } else if let Ok(rel) = entry.path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// Fuzzy subsequence score of `query` against `candidate`; higher is better.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.to_lowercase().chars().collect();
    let name_start = candidate.rfind('/').map_or(0, |i| candidate[..=i].chars().count());
    let mut score = 0;
    let mut ci = 0;
    let mut prev_match: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = (ci..cand.len()).find(|&i| cand[i] == q)?;
        score += 1;
        if prev_match == Some(found.wrapping_sub(1)) {
            score += 5; // consecutive
        }
        if found >= name_start {
            score += 3; // inside the file name
        }
        if found == name_start || found == 0 || matches!(cand.get(found.wrapping_sub(1)), Some('/' | '_' | '-' | '.')) {
            score += 4; // word start
        }
        prev_match = Some(found);
        ci = found + 1;
    }
    Some(score * 100 - cand.len() as i32)
}

#[cfg(test)]
mod tests {
    use super::fuzzy_score;

    #[test]
    fn prefers_file_name_matches() {
        let a = fuzzy_score("main", "src/main.rs").unwrap();
        let b = fuzzy_score("main", "docs/maintenance/notes.md").unwrap();
        assert!(a > b);
        assert!(fuzzy_score("xyz", "src/main.rs").is_none());
    }
}
