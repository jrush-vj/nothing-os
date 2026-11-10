// Now playing, through playerctl (MPRIS): a follower thread reports changes; the position is extrapolated from the
// last report while playing, so the progress line moves without polling.
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub playing: bool,
    pub artist: String,
    pub title: String,
    /// Seconds; 0 when the player doesn't say.
    pub length: f64,
    position: f64,
    at: Instant,
}

impl Track {
    pub fn position(&self) -> f64 {
        let p = if self.playing { self.position + self.at.elapsed().as_secs_f64() } else { self.position };
        if self.length > 0.0 { p.min(self.length) } else { p }
    }

    /// 0..1 through the track, if the length is known.
    pub fn progress(&self) -> Option<f32> {
        (self.length > 0.0).then(|| (self.position() / self.length) as f32)
    }

    pub fn label(&self) -> String {
        match (self.artist.is_empty(), self.title.is_empty()) {
            (false, false) => format!("{} — {}", self.artist, self.title),
            (true, false) => self.title.clone(),
            (false, true) => self.artist.clone(),
            _ => String::new(),
        }
    }
}

const FORMAT: &str = "{{status}}\t{{position}}\t{{mpris:length}}\t{{artist}}\t{{title}}";

/// One playerctl line -> a track (None: no player, or stopped).
fn parse(line: &str) -> Option<Track> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 5 || f[0] == "Stopped" || f[0].is_empty() {
        return None;
    }
    let us = |s: &str| s.trim().parse::<f64>().map(|v| v / 1e6).unwrap_or(0.0);
    Some(Track {
        playing: f[0] == "Playing",
        position: us(f[1]),
        length: us(f[2]),
        artist: f[3].trim().to_string(),
        title: f[4].trim().to_string(),
        at: Instant::now(),
    })
}

pub type Shared = Arc<Mutex<Option<Track>>>;

/// Follows the active player; calls `changed` after every update. Also re-reads the position every 5 s while
/// playing, in case of seeks (playerctl only reports metadata/status changes).
pub fn watch(changed: impl Fn() + Send + Sync + 'static) -> Shared {
    let shared: Shared = Arc::default();
    let changed = Arc::new(changed);
    {
        let (shared, changed) = (shared.clone(), changed.clone());
        std::thread::spawn(move || {
            loop {
                let child = Command::new("playerctl")
                    .args(["--follow", "metadata", "--format", FORMAT])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                let Ok(mut child) = child else { return }; // no playerctl: no media
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        *shared.lock().unwrap() = parse(&line);
                        changed();
                    }
                }
                let _ = child.wait();
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }
    {
        let (shared, changed) = (shared.clone(), changed.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !shared.lock().unwrap().as_ref().is_some_and(|t| t.playing) {
                    continue;
                }
                let Ok(out) = Command::new("playerctl").args(["metadata", "--format", FORMAT]).output() else { continue };
                let line = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
                *shared.lock().unwrap() = parse(&line);
                changed();
            }
        });
    }
    shared
}

pub fn command(cmd: &str) {
    crate::sys::spawn(&format!("playerctl {cmd}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_playerctl_lines() {
        let t = parse("Playing\t12000000\t300000000\tDaft Punk\tVeridis Quo").unwrap();
        assert!(t.playing && t.length == 300.0 && t.position() >= 12.0);
        assert_eq!(t.label(), "Daft Punk — Veridis Quo");
        assert!(parse("Stopped\t0\t0\t\t").is_none());
        assert!(parse("").is_none());
        let p = parse("Paused\t\t\t\tPodcast").unwrap();
        assert_eq!((p.label().as_str(), p.progress()), ("Podcast", None));
    }
}
