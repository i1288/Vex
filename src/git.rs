//! Git integration: file status for the explorer, the branch for the status
//! bar, and which lines changed since the last commit for the editor gutter.
//! `git` runs on a background thread; nothing here blocks the window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileStatus {
    /// Ordered by how much it matters, so a folder shows its most important child.
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflict,
}

impl FileStatus {
    pub fn letter(self) -> char {
        match self {
            FileStatus::Modified => 'M',
            FileStatus::Added => 'A',
            FileStatus::Untracked => 'U',
            FileStatus::Deleted => 'D',
            FileStatus::Renamed => 'R',
            FileStatus::Conflict => '!',
        }
    }
}

/// A changed stretch of a file, in the buffer's current line numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineChange {
    Added { start: usize, end: usize },
    Modified { start: usize, end: usize },
    /// Lines removed just above `at`.
    Deleted { at: usize },
}

#[derive(Default)]
pub struct Repo {
    pub branch: String,
    /// The commit checked out; when it changes, the gutter diffs start over.
    pub head: String,
    pub ahead: usize,
    pub behind: usize,
    /// Status of each changed file, by absolute path.
    pub files: HashMap<PathBuf, FileStatus>,
    /// Each changed file's folders, with the most important status inside.
    pub folders: HashMap<PathBuf, FileStatus>,
}

enum Job {
    Status(PathBuf),
    Diff { id: u64, version: u64, path: PathBuf, lines: Vec<String> },
    /// Drop cached HEAD contents (after a commit, say).
    Forget,
}

pub enum Done {
    Status(Option<Repo>),
    Diff { id: u64, version: u64, changes: Vec<LineChange> },
}

pub struct Git {
    jobs: Sender<Job>,
    results: Receiver<Done>,
}

impl Git {
    pub fn new(notify: impl Fn() + Send + 'static) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("vex-git".into())
            .spawn(move || {
                // HEAD's version of each file (None: not in HEAD), so typing
                // only re-diffs instead of running git again.
                let mut head: HashMap<PathBuf, Option<Vec<String>>> = HashMap::new();
                let mut last_head: Option<String> = None;
                while let Ok(first) = job_rx.recv() {
                    let mut batch = vec![first];
                    batch.extend(job_rx.try_iter());
                    // Keep only the newest diff per buffer and one status.
                    let mut diffs: HashMap<u64, (u64, PathBuf, Vec<String>)> = HashMap::new();
                    let mut status = None;
                    for job in batch {
                        match job {
                            Job::Status(root) => status = Some(root),
                            Job::Diff { id, version, path, lines } => {
                                diffs.insert(id, (version, path, lines));
                            }
                            Job::Forget => head.clear(),
                        }
                    }
                    if let Some(root) = status {
                        let repo = read_status(&root);
                        // A new commit (or checkout) changes what HEAD holds.
                        let commit = repo.as_ref().map(|r| r.head.clone());
                        if commit != last_head {
                            head.clear();
                            last_head = commit;
                        }
                        if tx.send(Done::Status(repo)).is_err() {
                            return;
                        }
                    }
                    for (id, (version, path, lines)) in diffs {
                        let base = head.entry(path.clone()).or_insert_with(|| head_lines(&path));
                        let changes = match base {
                            Some(base) => line_changes(base, &lines),
                            None => Vec::new(),
                        };
                        if tx.send(Done::Diff { id, version, changes }).is_err() {
                            return;
                        }
                    }
                    notify();
                }
            })
            .expect("can start the git thread");
        Self { jobs, results }
    }

    pub fn refresh_status(&self, root: &Path) {
        let _ = self.jobs.send(Job::Status(root.to_path_buf()));
    }

    pub fn forget_head(&self) {
        let _ = self.jobs.send(Job::Forget);
    }

    pub fn diff(&self, id: u64, version: u64, path: &Path, lines: Vec<String>) {
        let _ = self.jobs.send(Job::Diff { id, version, path: path.to_path_buf(), lines });
    }

    pub fn results(&self) -> impl Iterator<Item = Done> + '_ {
        self.results.try_iter()
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    let out = cmd.output().ok()?;
    out.status.success().then_some(out.stdout)
}

fn read_status(root: &Path) -> Option<Repo> {
    let top = git(root, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
    let top = top.canonicalize().unwrap_or(top);
    let out = git(root, &["status", "--porcelain=v1", "-z", "--branch", "--untracked-files=all"])?;
    let mut repo = parse_status(&top, &out);
    repo.head = git(root, &["rev-parse", "HEAD"]).map(|h| String::from_utf8_lossy(&h).trim().to_string()).unwrap_or_default();
    Some(repo)
}

fn parse_status(top: &Path, out: &[u8]) -> Repo {
    let mut repo = Repo::default();
    let mut entries = out.split(|&b| b == 0).map(|e| String::from_utf8_lossy(e).into_owned());
    while let Some(entry) = entries.next() {
        if let Some(head) = entry.strip_prefix("## ") {
            // "main...origin/main [ahead 1, behind 2]", "No commits yet on main", "HEAD (no branch)"
            let (name, rest) = head.split_once("...").map_or((head, ""), |(n, r)| (n, r));
            let name = name.strip_prefix("No commits yet on ").unwrap_or(name);
            repo.branch = name.split(' ').next().unwrap_or("").to_string();
            let count = |key: &str| {
                rest.split_once(key).and_then(|(_, r)| r.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).unwrap_or(0)
            };
            repo.ahead = count("ahead ");
            repo.behind = count("behind ");
            continue;
        }
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = entry.split_at(3);
        let mut chars = xy.chars();
        let (x, y) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));
        let status = match (x, y) {
            ('?', '?') => FileStatus::Untracked,
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => FileStatus::Conflict,
            ('R', _) => {
                entries.next(); // the old name
                FileStatus::Renamed
            }
            ('A', _) => FileStatus::Added,
            (_, 'D') | ('D', _) => FileStatus::Deleted,
            _ => FileStatus::Modified,
        };
        let abs = top.join(path);
        let mut dir = abs.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(top) && *d != top) {
            let e = repo.folders.entry(d.to_path_buf()).or_insert(status);
            *e = (*e).min(status);
            dir = d.parent();
        }
        repo.files.insert(abs, status);
    }
    repo
}

/// The file's lines as of the last commit, or None if it isn't committed.
fn head_lines(path: &Path) -> Option<Vec<String>> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_string_lossy();
    let out = git(dir, &["show", &format!("HEAD:./{name}")])?;
    Some(String::from_utf8_lossy(&out).lines().map(|l| l.trim_end_matches('\r').to_string()).collect())
}

/// What changed from `old` to `new`, as gutter marks.
pub fn line_changes(old: &[String], new: &[String]) -> Vec<LineChange> {
    // Trim the common start and end; the middle is usually small.
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let (a, b) = (&old[prefix..old.len() - suffix], &new[prefix..new.len() - suffix]);
    let mut out = Vec::new();
    match diff_hunks(a, b) {
        Some(hunks) => {
            for (del, (ins_start, ins_end)) in hunks {
                let (s, e) = (prefix + ins_start, prefix + ins_end);
                out.push(match (del > 0, e > s) {
                    (true, true) => LineChange::Modified { start: s, end: e },
                    (false, true) => LineChange::Added { start: s, end: e },
                    _ => LineChange::Deleted { at: s },
                });
            }
        }
        // Too different to diff cheaply: the whole middle counts as changed.
        None if !b.is_empty() => out.push(LineChange::Modified { start: prefix, end: prefix + b.len() }),
        None if !a.is_empty() => out.push(LineChange::Deleted { at: prefix }),
        None => {}
    }
    out
}

/// Myers diff of two line lists: for each change, how many old lines went
/// and which new lines (a half-open range) came. None if they differ in
/// more than a few thousand places.
fn diff_hunks(a: &[String], b: &[String]) -> Option<Vec<(usize, (usize, usize))>> {
    const MAX_D: usize = 2000;
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let offset = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = false;
    for d in 0..=max.min(MAX_D) as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) { v[idx + 1] } else { v[idx - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                found = true;
                break;
            }
            k += 2;
        }
        if found {
            break;
        }
    }
    if !found {
        return None;
    }
    // Walk back through the trace, listing every step: same, delete, insert.
    #[derive(Clone, Copy, PartialEq)]
    enum Op {
        Same,
        Delete,
        Insert,
    }
    let (mut x, mut y) = (n, m);
    let mut ops = Vec::new();
    for d in (0..trace.len()).rev() {
        let v = &trace[d];
        let d = d as isize;
        let k = x - y;
        let idx = |k: isize| (k + offset) as usize;
        let prev_k = if k == -d || (k != d && v[idx(k - 1)] < v[idx(k + 1)]) { k + 1 } else { k - 1 };
        let prev_x = if d == 0 { 0 } else { v[idx(prev_k)] };
        let prev_y = if d == 0 { 0 } else { prev_x - prev_k };
        while x > prev_x && y > prev_y {
            ops.push(Op::Same);
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            ops.push(if x == prev_x { Op::Insert } else { Op::Delete });
        }
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    // Group runs of deletes and inserts into hunks, placed by line in `b`.
    let mut hunks: Vec<(usize, (usize, usize))> = Vec::new();
    let mut bi = 0;
    let mut open = false;
    for op in ops {
        match op {
            Op::Same => {
                bi += 1;
                open = false;
            }
            Op::Delete | Op::Insert => {
                if !open {
                    hunks.push((0, (bi, bi)));
                    open = true;
                }
                let h = hunks.last_mut().expect("pushed above");
                if op == Op::Delete {
                    h.0 += 1;
                } else {
                    bi += 1;
                    h.1 .1 = bi;
                }
            }
        }
    }
    Some(hunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn classifies_changes() {
        let old = lines("a\nb\nc\nd\ne");
        assert_eq!(line_changes(&old, &old), vec![]);
        assert_eq!(line_changes(&old, &lines("a\nb\nX\nc\nd\ne")), vec![LineChange::Added { start: 2, end: 3 }]);
        assert_eq!(line_changes(&old, &lines("a\nB\nc\nd\ne")), vec![LineChange::Modified { start: 1, end: 2 }]);
        assert_eq!(line_changes(&old, &lines("a\nb\nd\ne")), vec![LineChange::Deleted { at: 2 }]);
        assert_eq!(
            line_changes(&old, &lines("X\na\nb\nC\nd")),
            vec![LineChange::Added { start: 0, end: 1 }, LineChange::Modified { start: 3, end: 4 }, LineChange::Deleted { at: 5 }]
        );
        assert_eq!(line_changes(&[], &lines("a\nb")), vec![LineChange::Added { start: 0, end: 2 }]);
    }

    #[test]
    fn parses_porcelain_status() {
        let top = Path::new("/r");
        let out = b"## main...origin/main [ahead 2, behind 1]\0 M src/app.rs\0?? new.txt\0A  src/x/y.rs\0R  b.rs\0a.rs\0";
        let repo = parse_status(top, out);
        assert_eq!(repo.branch, "main");
        assert_eq!((repo.ahead, repo.behind), (2, 1));
        assert_eq!(repo.files[Path::new("/r/src/app.rs")], FileStatus::Modified);
        assert_eq!(repo.files[Path::new("/r/new.txt")], FileStatus::Untracked);
        assert_eq!(repo.files[Path::new("/r/src/x/y.rs")], FileStatus::Added);
        assert_eq!(repo.files[Path::new("/r/b.rs")], FileStatus::Renamed);
        assert_eq!(repo.folders[Path::new("/r/src")], FileStatus::Modified, "the most important child wins");
        assert_eq!(repo.folders[Path::new("/r/src/x")], FileStatus::Added);
        assert!(!repo.files.contains_key(Path::new("/r/a.rs")));
        assert_eq!(parse_status(top, b"## No commits yet on main\0").branch, "main");
    }
}
