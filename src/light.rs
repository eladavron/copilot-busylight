//! Background thread: merges the chat states and drives the light. On an RDP
//! server it only reads the states, to mirror the light in the tray icon.

use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::kuando;
use crate::sessions::{self, Light, Status, Tracker};
use crate::usb::Device;

pub const IDLE_COLOR: (u8, u8, u8) = (0, 255, 0);
pub const BUSY_COLOR: (u8, u8, u8) = (255, 0, 0);
pub const ATTENTION_COLOR: (u8, u8, u8) = (255, 200, 0);

const FRAME: Duration = Duration::from_millis(50);
const POLL: Duration = Duration::from_millis(250);
/// RDP drive sharing is slow, so a remote tray reads less often.
const REMOTE_POLL: Duration = Duration::from_secs(2);
const PULSE_PERIOD_SECS: f32 = 2.0;
/// The device goes dark a few seconds after the last write.
const STEADY_REFRESH: Duration = Duration::from_secs(1);
const RECONNECT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub status: Status,
    pub paused: bool,
    /// Local: the connected light. Remote: always `None`.
    pub device: Option<&'static str>,
    /// Remote: whether the PC's sessions folder can be reached.
    pub reachable: bool,
}

pub struct Shared {
    pub remote: bool,
    pub paused: AtomicBool,
    pub stop: AtomicBool,
    report: Mutex<Report>,
    sessions_dir: Mutex<PathBuf>,
}

impl Shared {
    pub fn new(remote: bool, sessions_dir: PathBuf, paused: bool) -> Shared {
        Shared {
            remote,
            paused: AtomicBool::new(paused),
            stop: AtomicBool::new(false),
            report: Mutex::default(),
            sessions_dir: Mutex::new(sessions_dir),
        }
    }

    pub fn report(&self) -> Report {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, report: Report) {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner) = report;
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.sessions_dir.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_sessions_dir(&self, dir: PathBuf) {
        *self.sessions_dir.lock().unwrap_or_else(PoisonError::into_inner) = dir;
    }
}

pub fn color_for(light: Light, seconds: f32) -> (u8, u8, u8) {
    match light {
        Light::Off => (0, 0, 0),
        Light::Idle => IDLE_COLOR,
        Light::Attention => ATTENTION_COLOR,
        Light::Busy => {
            let level = 0.55 - 0.45 * (TAU * seconds / PULSE_PERIOD_SECS).cos();
            let dim = |channel: u8| (f32::from(channel) * level) as u8;
            (dim(BUSY_COLOR.0), dim(BUSY_COLOR.1), dim(BUSY_COLOR.2))
        }
    }
}

struct Lamp {
    device: Option<Device>,
    retry_at: Instant,
}

impl Lamp {
    fn show(&mut self, rgb: (u8, u8, u8), now: Instant) {
        if self.device.is_none() && now >= self.retry_at {
            self.device = Device::open_first();
            self.retry_at = now + RECONNECT;
        }
        if let Some(device) = &mut self.device
            && !device.write(&kuando::color_packet(rgb))
        {
            self.device = None;
            self.retry_at = now + RECONNECT;
        }
    }
}

/// Runs until `shared.stop` is set; calls `notify` whenever the report changes.
pub fn run(shared: &Shared, notify: impl Fn()) {
    let remote = shared.remote;
    let poll = if remote { REMOTE_POLL } else { POLL };
    let start = Instant::now();
    let mut lamp = Lamp {
        device: None,
        retry_at: start,
    };
    let mut dir = PathBuf::new();
    let mut tracker = Tracker::default();
    let mut status = Status::default();
    let mut reachable = false;
    let mut shown = None;
    let mut last_poll: Option<Instant> = None;
    let mut last_sent: Option<Instant> = None;
    let mut last_report = None;

    while !shared.stop.load(Relaxed) {
        let now = Instant::now();
        let current_dir = shared.sessions_dir();
        if current_dir != dir {
            dir = current_dir;
            if !remote {
                let _ = fs::create_dir_all(&dir);
            }
            tracker = Tracker::default();
            last_poll = None;
        }
        if last_poll.is_none_or(|at| now - at >= poll) {
            last_poll = Some(now);
            reachable = dir.is_dir();
            // The files on a remote PC belong to its tray app; never delete them from here.
            status = tracker.update(sessions::read_all(&dir, remote), now);
        }
        let paused = shared.paused.load(Relaxed);
        if !remote {
            let light = if paused { Light::Off } else { status.light };
            if shown != Some(light) {
                shown = Some(light);
                last_sent = None;
            }
            if light == Light::Busy || last_sent.is_none_or(|at| now - at >= STEADY_REFRESH) {
                lamp.show(color_for(light, (now - start).as_secs_f32()), now);
                last_sent = Some(now);
            }
        }

        let report = Report {
            status,
            paused,
            device: lamp.device.as_ref().map(|device| device.name),
            reachable,
        };
        if last_report != Some(report) {
            last_report = Some(report);
            shared.publish(report);
            notify();
        }
        sleep(FRAME);
    }

    if let Some(device) = &mut lamp.device {
        device.write(&kuando::color_packet((0, 0, 0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_stays_red_and_varies() {
        let dim = color_for(Light::Busy, 0.0);
        let bright = color_for(Light::Busy, PULSE_PERIOD_SECS / 2.0);
        assert_eq!((dim.1, dim.2, bright.1, bright.2), (0, 0, 0, 0));
        assert!(dim.0 < 40 && bright.0 > 240, "{dim:?} {bright:?}");
    }
}
