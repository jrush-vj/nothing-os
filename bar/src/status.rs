// Readings for the status cluster: network, brightness, volume, battery, RTX 4060. Files under /sys and /proc
// where possible; wpctl/brightnessctl for audio and backlight changes (both installed by the rice).
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub wifi: bool,
    /// Download / upload rates, bytes per second.
    pub rx_rate: f64,
    pub tx_rate: f64,
}

/// The interface of the default route (the one traffic actually uses).
fn default_iface(route: &str) -> Option<String> {
    route.lines().skip(1).find_map(|l| {
        let mut f = l.split_whitespace();
        let iface = f.next()?;
        (f.next()? == "00000000").then(|| iface.to_string())
    })
}

pub struct NetMeter {
    last: Option<(Instant, String, u64, u64)>,
}

impl NetMeter {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// None when there's no default route (offline).
    pub fn sample(&mut self) -> Option<Net> {
        let iface = default_iface(&std::fs::read_to_string("/proc/net/route").ok()?)?;
        let base = format!("/sys/class/net/{iface}");
        let read = |f: &str| -> Option<u64> { std::fs::read_to_string(format!("{base}/statistics/{f}")).ok()?.trim().parse().ok() };
        let (rx, tx) = (read("rx_bytes")?, read("tx_bytes")?);
        let now = Instant::now();
        let (rx_rate, tx_rate) = match &self.last {
            Some((t, i, r, x)) if *i == iface => {
                let dt = now.duration_since(*t).as_secs_f64().max(0.001);
                (rx.saturating_sub(*r) as f64 / dt, tx.saturating_sub(*x) as f64 / dt)
            }
            _ => (0.0, 0.0),
        };
        self.last = Some((now, iface, rx, tx));
        Some(Net { wifi: std::path::Path::new(&format!("{base}/wireless")).exists(), rx_rate, tx_rate })
    }
}

/// "↓2.4M", "↓180K", "↓0K"
pub fn fmt_rate(bps: f64) -> String {
    if bps >= 999_500.0 {
        format!("↓{:.1}M", bps / 1e6)
    } else {
        format!("↓{:.0}K", bps / 1e3)
    }
}

fn backlight() -> Option<std::path::PathBuf> {
    std::fs::read_dir("/sys/class/backlight").ok()?.flatten().map(|e| e.path()).next()
}

/// Screen brightness in percent, if there's a backlight.
pub fn brightness() -> Option<f32> {
    let dir = backlight()?;
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok()?.trim().parse::<f32>().ok();
    let max = read("max_brightness")?;
    (max > 0.0).then(|| read("brightness").map(|b| b / max * 100.0))?
}

pub fn change_brightness(up: bool) {
    let _ = Command::new("brightnessctl").args(["-q", "set", if up { "5%+" } else { "5%-" }]).status();
}

/// (percent, muted) of the default output.
pub fn volume() -> Option<(f32, bool)> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    parse_volume(&String::from_utf8_lossy(&out.stdout))
}

fn parse_volume(s: &str) -> Option<(f32, bool)> {
    let v: f32 = s.strip_prefix("Volume: ")?.split_whitespace().next()?.parse().ok()?;
    Some((v * 100.0, s.contains("[MUTED]")))
}

pub fn change_volume(up: bool) {
    let _ = Command::new("wpctl").args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", if up { "5%+" } else { "5%-" }]).status();
}

pub fn toggle_mute() {
    let _ = Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]).status();
}

/// Calls `send` whenever the audio server reports a sink change (volume keys, other apps).
pub fn watch_audio(send: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            let child = Command::new("pactl").arg("subscribe").stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
            let Ok(mut child) = child else { return };
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if line.contains("on sink") || line.contains("on server") {
                        send();
                    }
                }
            }
            let _ = child.wait();
            std::thread::sleep(std::time::Duration::from_secs(2)); // audio server restarted
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
}

pub fn battery() -> Option<Battery> {
    let dir = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery"))?;
    let percent = std::fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()?;
    let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some(Battery { percent, charging: status.trim() == "Charging" })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dgpu {
    Asleep,
    Awake,
    /// The card is there but no driver is bound: it can neither work nor power down.
    NoDriver,
    Absent,
}

/// The NVIDIA GPU's state from sysfs. Reading this never wakes it, unlike nvidia-smi.
pub fn dgpu() -> Dgpu {
    let Ok(dir) = std::fs::read_dir("/sys/bus/pci/devices") else { return Dgpu::Absent };
    for p in dir.flatten().map(|e| e.path()) {
        let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap_or_default();
        if read("vendor").trim() == "0x10de" && read("class").starts_with("0x03") {
            if !p.join("driver").exists() {
                return Dgpu::NoDriver;
            }
            return if read("power/runtime_status").trim() == "active" { Dgpu::Awake } else { Dgpu::Asleep };
        }
    }
    Dgpu::Absent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_route_wins() {
        let r = "Iface\tDestination\tGateway\nenp7s0\t001FA8C0\t00000000\nwlan0\t00000000\t011FA8C0\n";
        assert_eq!(default_iface(r).as_deref(), Some("wlan0"));
        assert_eq!(default_iface("Iface\tDestination\n"), None);
    }

    #[test]
    fn rates_and_volume_format() {
        assert_eq!(fmt_rate(2_400_000.0), "↓2.4M");
        assert_eq!(fmt_rate(180_000.0), "↓180K");
        assert_eq!(fmt_rate(12.0), "↓0K");
        assert_eq!(parse_volume("Volume: 0.45\n"), Some((45.0, false)));
        let (v, muted) = parse_volume("Volume: 0.30 [MUTED]\n").unwrap();
        assert!((v - 30.0).abs() < 1e-3 && muted);
        assert_eq!(parse_volume("garbage"), None);
    }
}
