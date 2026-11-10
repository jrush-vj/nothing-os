// Control-centre data and actions. Reads that spawn programs (nmcli, bluetoothctl, ...) run on a worker thread
// so the bar never stalls; actions are fire-and-forget, followed by a refresh.
// Untrusted strings (SSIDs, device names) only ever go to programs as separate argv entries, never through a shell.
use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u8,
    pub active: bool,
    pub secured: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BtDev {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

/// Toggle and list state, refreshed in the background.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub wifi_on: bool,
    pub bt_on: bool,
    pub mic_muted: bool,
    pub profile: String,
    pub networks: Vec<WifiNet>,
    pub devices: Vec<BtDev>,
    pub saved: Vec<String>,
}

fn out(prog: &str, args: &[&str]) -> String {
    Command::new(prog).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// nmcli -t escapes ':' inside fields as '\:'.
fn split_terse(line: &str) -> Vec<String> {
    let mut f = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    f.last_mut().unwrap().push(n);
                }
            }
            ':' => f.push(String::new()),
            _ => f.last_mut().unwrap().push(c),
        }
    }
    f
}

pub fn parse_networks(text: &str) -> Vec<WifiNet> {
    let mut nets: Vec<WifiNet> = Vec::new();
    for l in text.lines() {
        let f = split_terse(l);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let n = WifiNet { active: f[0] == "*", ssid: f[1].clone(), signal: f[2].parse().unwrap_or(0), secured: !f[3].is_empty() && f[3] != "--" };
        // One row per SSID (the strongest access point).
        match nets.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) if n.signal > o.signal || n.active => *o = WifiNet { active: o.active || n.active, ..n },
            Some(_) => {}
            None => nets.push(n),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets.truncate(8);
    nets
}

pub fn parse_devices(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ')?;
            Some((mac.to_string(), name.to_string()))
        })
        .collect()
}

pub fn read_state(lists: bool) -> State {
    let mut s = State {
        wifi_on: out("nmcli", &["radio", "wifi"]).trim() == "enabled",
        bt_on: out("bluetoothctl", &["show"]).contains("Powered: yes"),
        mic_muted: out("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).contains("MUTED"),
        profile: out("powerprofilesctl", &["get"]).trim().to_string(),
        ..State::default()
    };
    if lists {
        if s.wifi_on {
            s.networks = parse_networks(&out("nmcli", &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list"]));
            s.saved = out("nmcli", &["-t", "-f", "NAME", "connection", "show"]).lines().map(|l| split_terse(l).remove(0)).collect();
        }
        if s.bt_on {
            s.devices = parse_devices(&out("bluetoothctl", &["devices", "Paired"]))
                .into_iter()
                .map(|(mac, name)| BtDev { connected: out("bluetoothctl", &["info", &mac]).contains("Connected: yes"), mac, name })
                .collect();
        }
    }
    s
}

pub type Shared = Arc<Mutex<State>>;

/// Refreshes `shared` on a thread, then calls `done` (which wakes the bar).
pub fn refresh(shared: &Shared, lists: bool, done: impl Fn() + Send + 'static) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let s = read_state(lists);
        *shared.lock().unwrap() = s;
        done();
    });
}

/// Runs a program detached, with each argument passed as-is (no shell).
pub fn run(args: &[&str]) {
    if let Ok(mut c) = Command::new("setsid").arg("-f").args(args).spawn() {
        std::thread::spawn(move || c.wait());
    }
}

/// Runs and waits (for actions whose result the next refresh should see).
pub fn run_wait(args: &[&str]) {
    if let Some((prog, rest)) = args.split_first() {
        let _ = Command::new(prog).args(rest).status();
    }
}

pub fn connect_wifi(net: &WifiNet, saved: &[String]) {
    if saved.iter().any(|s| s == &net.ssid) {
        run(&["nmcli", "connection", "up", "id", &net.ssid]);
    } else if net.secured {
        run(&["kitty", "--class", "nmtui", "-e", "nmtui", "connect", &net.ssid]); // asks for the password
    } else {
        run(&["nmcli", "dev", "wifi", "connect", &net.ssid]);
    }
}

// ---------------------------------------------------------------- graphs

pub const HISTORY: usize = 60; // 2 minutes at one sample per 2 s

#[derive(Clone, Debug, Default)]
pub struct Series(pub VecDeque<f32>);

impl Series {
    pub fn push(&mut self, v: f32) {
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }
    pub fn last(&self) -> Option<f32> {
        self.0.back().copied()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Graphs {
    pub cpu: Series,
    pub ram: Series,
    pub temp: Series,
    pub igpu: Series,
    pub dgpu: Series,
    pub rx: Series,
    pub tx: Series,
    /// (used, total) bytes of /
    pub disk: (f64, f64),
    pub dgpu_awake: bool,
}

pub fn cpu_temp() -> Option<f32> {
    let dir = std::fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|e| e.path()).find(|p| {
        std::fs::read_to_string(p.join("name")).is_ok_and(|n| n.trim() == "coretemp" || n.trim() == "k10temp")
    })?;
    Some(std::fs::read_to_string(dir.join("temp1_input")).ok()?.trim().parse::<f32>().ok()? / 1000.0)
}

/// Intel iGPU busy-ness, approximated by its current clock against its maximum (no root needed).
pub fn igpu_load() -> Option<f32> {
    let gt = std::fs::read_dir("/sys/class/drm").ok()?.flatten().map(|e| e.path().join("gt/gt0")).find(|p| p.join("rps_act_freq_mhz").exists())?;
    let read = |f: &str| std::fs::read_to_string(gt.join(f)).ok()?.trim().parse::<f32>().ok();
    let max = read("rps_max_freq_mhz")?;
    (max > 0.0).then(|| read("rps_act_freq_mhz").map(|a| a / max * 100.0))?
}

/// RTX 4060 power draw in watts — only asked while it's awake (querying wakes it otherwise).
pub fn dgpu_watts() -> Option<f32> {
    out("nvidia-smi", &["--query-gpu=power.draw", "--format=csv,noheader,nounits"]).trim().parse().ok()
}

pub fn disk_usage() -> (f64, f64) {
    let path = std::ffi::CString::new("/").unwrap();
    // SAFETY: statvfs fills a plain struct for a valid NUL-terminated path.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut st) != 0 {
            return (0.0, 0.0);
        }
        let total = st.f_blocks as f64 * st.f_frsize as f64;
        let free = st.f_bavail as f64 * st.f_frsize as f64;
        (total - free, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_terse_lines_with_escaped_colons() {
        let t = "*:Home\\:5G:78:WPA2\n:Cafe:40:\n:Home\\:5G:60:WPA2\n:Office:90:WPA2 WPA3\n:::\n";
        let n = parse_networks(t);
        assert_eq!(n[0], WifiNet { ssid: "Home:5G".into(), signal: 78, active: true, secured: true });
        assert_eq!(n[1].ssid, "Office");
        assert_eq!(n.iter().filter(|x| x.ssid == "Home:5G").count(), 1);
        assert!(!n.iter().find(|x| x.ssid == "Cafe").unwrap().secured);
    }

    #[test]
    fn paired_devices() {
        let d = parse_devices("Device AA:BB:CC:DD:EE:FF Nothing Ear (2)\ngarbage\n");
        assert_eq!(d, vec![("AA:BB:CC:DD:EE:FF".into(), "Nothing Ear (2)".into())]);
    }

    #[test]
    fn series_keeps_the_last_two_minutes() {
        let mut s = Series::default();
        for i in 0..100 {
            s.push(i as f32);
        }
        assert_eq!((s.0.len(), s.0[0], s.last()), (HISTORY, 40.0, Some(99.0)));
    }
}
