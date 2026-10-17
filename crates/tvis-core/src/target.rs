//! What the user watches, and how that resolves to processes.

use std::collections::{HashSet, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::frame::{Frame, Pid};

/// A watched subject. Always includes the whole descendant tree.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Target {
    /// Every process with this executable name.
    Name(String),
    /// One process by id.
    Pid(Pid),
}

/// Stable, printable key of a [`Target`].
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct TargetId(String);

impl TargetId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TargetId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Case-insensitive name comparison that ignores a Windows `.exe` suffix.
fn normalize(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").map(str::to_owned).unwrap_or(lower)
}

impl Target {
    pub fn id(&self) -> TargetId {
        match self {
            Self::Name(n) => TargetId(format!("name:{}", normalize(n))),
            Self::Pid(p) => TargetId(format!("pid:{p}")),
        }
    }

    /// Parses user input: digits mean a pid, anything else a name.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }
        Some(match input.parse::<Pid>() {
            Ok(pid) => Self::Pid(pid),
            Err(_) => Self::Name(input.to_owned()),
        })
    }

    /// Resolves the target to the pids of its root processes plus every
    /// descendant, roots first (breadth first).
    pub fn resolve(&self, frame: &Frame) -> Vec<Pid> {
        let roots: Vec<Pid> = match self {
            Self::Pid(pid) => frame.process(*pid).map(|p| p.pid).into_iter().collect(),
            Self::Name(name) => {
                let wanted = normalize(name);
                frame
                    .processes
                    .iter()
                    .filter(|p| !p.is_thread() && normalize(&p.name) == wanted)
                    .map(|p| p.pid)
                    .collect()
            }
        };
        if roots.is_empty() {
            return roots;
        }
        let children = frame.children_of();
        let mut seen: HashSet<Pid> = HashSet::new();
        let mut queue: VecDeque<Pid> = VecDeque::new();
        let mut out = Vec::new();
        for r in roots {
            if seen.insert(r) {
                queue.push_back(r);
            }
        }
        while let Some(pid) = queue.pop_front() {
            out.push(pid);
            for &c in children.get(&pid).into_iter().flatten() {
                if seen.insert(c) {
                    queue.push_back(c);
                }
            }
        }
        out
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(n) => f.write_str(n),
            Self::Pid(p) => write!(f, "PID {p}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{ProcessKind, ProcessSample, SystemSample};

    fn frame() -> Frame {
        let mut thread = ProcessSample::new(11, Some(10), "worker-thread");
        thread.kind = ProcessKind::Thread;
        Frame::new(
            SystemSample::default(),
            vec![
                ProcessSample::new(1, None, "init"),
                ProcessSample::new(10, Some(1), "App.exe"),
                ProcessSample::new(12, Some(10), "child"),
                ProcessSample::new(13, Some(12), "grandchild"),
                ProcessSample::new(20, Some(1), "app"),
                ProcessSample::new(30, Some(1), "other"),
                thread,
            ],
        )
    }

    #[test]
    fn name_matches_case_and_exe_insensitively_with_descendants() {
        let f = frame();
        let mut pids = Target::Name("app".into()).resolve(&f);
        assert_eq!(pids[..2], [10, 20]);
        pids.sort();
        assert_eq!(pids, vec![10, 11, 12, 13, 20]);
        assert_eq!(Target::Name("APP.EXE".into()).id(), Target::Name("app".into()).id());
    }

    #[test]
    fn pid_resolves_subtree_only() {
        let f = frame();
        let mut pids = Target::Pid(12).resolve(&f);
        pids.sort();
        assert_eq!(pids, vec![12, 13]);
        assert!(Target::Pid(999).resolve(&f).is_empty());
    }

    #[test]
    fn threads_never_count_as_name_roots() {
        let f = frame();
        assert!(Target::Name("worker-thread".into()).resolve(&f).is_empty());
    }

    #[test]
    fn parse_input() {
        assert_eq!(Target::parse(" 42 "), Some(Target::Pid(42)));
        assert_eq!(Target::parse("java"), Some(Target::Name("java".into())));
        assert_eq!(Target::parse("  "), None);
    }
}
