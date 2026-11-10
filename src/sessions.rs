//! Per-chat state files shared by the hook and the tray app, and the rules
//! that merge them into one light state.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Interrupting a turn (Esc) fires no Stop hook.
pub const BUSY_STALE: Duration = Duration::from_secs(15 * 60);
/// Local chats never send SessionEnd, so idle sessions expire instead.
pub const IDLE_STALE: Duration = Duration::from_secs(30 * 60);
pub const SESSION_STALE: Duration = Duration::from_secs(12 * 3600);
/// Green shown when one chat finishes while others are still working.
pub const FINISH_FLASH: Duration = Duration::from_secs(5);

const FILE_SHARE_ALL: u32 = 0x7;

/// Ordered by priority: the highest state across all chats wins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Light {
    #[default]
    Off,
    Idle,
    Busy,
    Attention,
}

impl Light {
    pub fn parse(text: &str) -> Option<Light> {
        match text.trim() {
            "idle" => Some(Light::Idle),
            "busy" => Some(Light::Busy),
            "attention" => Some(Light::Attention),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Light::Off => "off",
            Light::Idle => "idle",
            Light::Busy => "busy",
            Light::Attention => "attention",
        }
    }

    fn is_working(self) -> bool {
        matches!(self, Light::Busy | Light::Attention)
    }
}

/// Install folder for the exes.
pub fn data_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".copilot").join("busylight")
}

/// Applies the stale rules; `None` means the session is gone.
pub fn effective(state: Light, age: Duration) -> Option<Light> {
    if age > SESSION_STALE {
        return None;
    }
    let state = if state == Light::Busy && age > BUSY_STALE {
        Light::Idle
    } else {
        state
    };
    if state == Light::Idle && age > IDLE_STALE {
        return None;
    }
    Some(state)
}

/// Session file name -> (state, time since last update).
pub type Snapshot = HashMap<String, (Light, Duration)>;

/// Reads every session file, deleting the stale ones unless `read_only`.
pub fn read_all(dir: &Path, read_only: bool) -> Snapshot {
    let mut sessions = Snapshot::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return sessions;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        // Hooks write "<name>.<pid>.tmp" first, then rename.
        if path.extension().is_some() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(state) = read_shared(&path).ok().and_then(|text| Light::parse(&text)) else {
            continue;
        };
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or_default();
        match effective(state, age) {
            Some(state) => {
                sessions.insert(name.to_owned(), (state, age));
            }
            None if !read_only => {
                let _ = fs::remove_file(&path);
            }
            None => {}
        }
    }
    sessions
}

/// Opens with delete sharing so a hook can replace the file while we read it.
fn read_shared(path: &Path) -> io::Result<String> {
    let mut file = OpenOptions::new().read(true).share_mode(FILE_SHARE_ALL).open(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub light: Light,
    pub chats: usize,
    pub working: usize,
}

#[derive(Default)]
pub struct Tracker {
    previous: Snapshot,
    flash_until: Option<Instant>,
}

impl Tracker {
    pub fn update(&mut self, current: Snapshot, now: Instant) -> Status {
        if self.any_just_finished(&current) {
            self.flash_until = Some(now + FINISH_FLASH);
        }
        let mut light = current.values().map(|&(state, _)| state).max().unwrap_or_default();
        // Attention still wins over the finish flash.
        if light == Light::Busy && self.flash_until.is_some_and(|until| now < until) {
            light = Light::Idle;
        }
        let status = Status {
            light,
            chats: current.len(),
            working: current.values().filter(|(state, _)| state.is_working()).count(),
        };
        self.previous = current;
        status
    }

    /// A chat went from working to idle by itself (not via the stale timeout).
    fn any_just_finished(&self, current: &Snapshot) -> bool {
        current.iter().any(|(name, &(state, age))| {
            let was_working = self.previous.get(name).is_some_and(|(prev, _)| prev.is_working());
            was_working && state == Light::Idle && age < FINISH_FLASH
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn snapshot(entries: &[(&str, Light, Duration)]) -> Snapshot {
        entries
            .iter()
            .map(|&(name, state, age)| (name.to_owned(), (state, age)))
            .collect()
    }

    #[test]
    fn stale_rules() {
        assert_eq!(effective(Light::Busy, 14 * MIN), Some(Light::Busy));
        assert_eq!(effective(Light::Busy, 16 * MIN), Some(Light::Idle));
        assert_eq!(effective(Light::Busy, 31 * MIN), None);
        assert_eq!(effective(Light::Idle, 31 * MIN), None);
        assert_eq!(effective(Light::Attention, 60 * MIN), Some(Light::Attention));
        assert_eq!(effective(Light::Attention, 13 * 60 * MIN), None);
    }

    #[test]
    fn highest_priority_wins() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        let status = tracker.update(
            snapshot(&[
                ("a", Light::Idle, MIN),
                ("b", Light::Busy, MIN),
                ("c", Light::Attention, MIN),
            ]),
            now,
        );
        assert_eq!(
            status,
            Status {
                light: Light::Attention,
                chats: 3,
                working: 2
            }
        );
        assert_eq!(tracker.update(Snapshot::new(), now).light, Light::Off);
    }

    #[test]
    fn finish_flashes_green_then_returns_to_busy() {
        let mut tracker = Tracker::default();
        let start = Instant::now();
        let zero = Duration::ZERO;
        tracker.update(snapshot(&[("a", Light::Busy, zero), ("b", Light::Busy, zero)]), start);

        let finished = snapshot(&[("a", Light::Idle, zero), ("b", Light::Busy, zero)]);
        assert_eq!(tracker.update(finished.clone(), start).light, Light::Idle);
        let later = start + FINISH_FLASH + Duration::from_millis(1);
        assert_eq!(tracker.update(finished, later).light, Light::Busy);
    }

    #[test]
    fn attention_beats_finish_flash() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        let zero = Duration::ZERO;
        tracker.update(
            snapshot(&[("a", Light::Busy, zero), ("b", Light::Attention, zero)]),
            now,
        );
        let status = tracker.update(
            snapshot(&[("a", Light::Idle, zero), ("b", Light::Attention, zero)]),
            now,
        );
        assert_eq!(status.light, Light::Attention);
    }

    #[test]
    fn stale_busy_does_not_flash() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        tracker.update(snapshot(&[("a", Light::Busy, 14 * MIN), ("b", Light::Busy, MIN)]), now);
        // "a" turned idle through the 15-minute timeout, so its age is large.
        let status = tracker.update(snapshot(&[("a", Light::Idle, 16 * MIN), ("b", Light::Busy, MIN)]), now);
        assert_eq!(status.light, Light::Busy);
    }
}
