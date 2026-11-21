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
    /// The MPRIS instance ("firefox.instance_1_268"), so re-reads ask the same player.
    player: String,
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
}

const FORMAT: &str = "{{status}}\t{{position}}\t{{mpris:length}}\t{{artist}}\t{{title}}\t{{playerInstance}}";

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
        player: f.get(5).map_or("", |p| p.trim()).to_string(),
    })
}

/// Firefox briefly reports no length and position 0 after a seek while paused; keep what we knew about the same
/// track instead of dropping the progress line.
fn merge(old: Option<&Track>, mut new: Option<Track>) -> Option<Track> {
    if let (Some(o), Some(n)) = (old, new.as_mut())
        && n.length == 0.0
        && o.length > 0.0
        && (&o.player, &o.title) == (&n.player, &n.title)
    {
        (n.length, n.position) = (o.length, o.position());
    }
    new
}

pub type Shared = Arc<Mutex<Option<Track>>>;

/// Follows the active player; calls `changed` after every update. Also re-reads the position every 2 s (playing
/// or paused), in case of seeks (playerctl only reports metadata/status changes).
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
                        let mut cur = shared.lock().unwrap();
                        *cur = merge(cur.as_ref(), parse(&line));
                        drop(cur);
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
                std::thread::sleep(std::time::Duration::from_secs(2));
                let Some(player) = shared.lock().unwrap().as_ref().map(|t| t.player.clone()) else { continue };
                let mut cmd = Command::new("playerctl");
                if !player.is_empty() {
                    cmd.args(["-p", &player]);
                }
                let Ok(out) = cmd.args(["metadata", "--format", FORMAT]).output() else { continue };
                let line = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
                let mut cur = shared.lock().unwrap();
                // The follower may have moved to another player while we asked; its report wins.
                if cur.as_ref().is_some_and(|t| t.player == player) {
                    *cur = merge(cur.as_ref(), parse(&line));
                    drop(cur);
                    changed();
                }
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
        let t = parse("Playing\t12000000\t300000000\tDaft Punk\tVeridis Quo\tspotify").unwrap();
        assert_eq!(t.player, "spotify");
        // A length-less report of the same track keeps the length; another track doesn't.
        let gap = merge(Some(&t), parse("Paused\t0\t\tDaft Punk\tVeridis Quo\tspotify")).unwrap();
        assert!(gap.length == 300.0 && gap.position() >= 12.0);
        let other = merge(Some(&t), parse("Paused\t0\t\tDaft Punk\tOne More Time\tspotify")).unwrap();
        assert_eq!(other.progress(), None);
        assert!(t.playing && t.length == 300.0 && t.position() >= 12.0);
        assert_eq!((t.artist.as_str(), t.title.as_str()), ("Daft Punk", "Veridis Quo"));
        assert!(parse("Stopped\t0\t0\t\t").is_none());
        assert!(parse("").is_none());
        let p = parse("Paused\t\t\t\tPodcast").unwrap();
        assert_eq!((p.title.as_str(), p.progress()), ("Podcast", None));
    }
}
