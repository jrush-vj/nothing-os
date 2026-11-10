// Hyprland IPC: requests on .socket.sock (plain-text replies), events on .socket2.sock.
// Hyprland 0.57 runs a Lua config, so dispatches are Lua expressions: `dispatch hl.dsp.focus({ workspace = 2 })`.
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let rt = std::env::var_os("XDG_RUNTIME_DIR")?;
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(PathBuf::from(rt).join("hypr").join(sig))
}

pub fn request(cmd: &str) -> Option<String> {
    let mut s = UnixStream::connect(dir()?.join(".socket.sock")).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn dispatch(lua: &str) {
    if request(&format!("dispatch {lua}")).is_none() {
        eprintln!("nothing-bar: dispatch failed: {lua}");
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    /// Normal workspaces that have windows, by id.
    pub workspaces: Vec<i32>,
    pub active: i32,
    pub class: String,
    pub title: String,
    /// Window address -> workspace id, to place urgent events.
    pub windows: Vec<(String, i32)>,
}

/// "workspace 3 (3) on monitor eDP-1:" -> 3
fn header_id(line: &str) -> Option<i32> {
    line.strip_prefix("workspace ")?.split_whitespace().next()?.parse().ok()
}

fn field<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|l| l.trim_start().strip_prefix(key)?.strip_prefix(": "))
}

pub fn parse_workspaces(text: &str) -> Vec<i32> {
    let mut out: Vec<i32> = text
        .split("\n\n")
        .filter_map(|b| {
            let id = header_id(b.trim_start().lines().next()?)?;
            let windows: u32 = field(b, "windows")?.parse().ok()?;
            (id > 0 && windows > 0).then_some(id) // id <= 0: special workspaces (scratchpad)
        })
        .collect();
    out.sort_unstable();
    out
}

/// "Window 55b7740038e0 -> title:" blocks -> (0x55b7740038e0, workspace id)
pub fn parse_clients(text: &str) -> Vec<(String, i32)> {
    text.split("\n\n")
        .filter_map(|b| {
            let addr = b.trim_start().strip_prefix("Window ")?.split_whitespace().next()?;
            let ws = field(b, "workspace")?.split_whitespace().next()?.parse().ok()?;
            Some((format!("0x{addr}"), ws))
        })
        .collect()
}

pub fn snapshot() -> Option<Snapshot> {
    let active = header_id(request("activeworkspace")?.lines().next()?)?;
    let win = request("activewindow").unwrap_or_default();
    Some(Snapshot {
        workspaces: parse_workspaces(&request("workspaces")?),
        active,
        class: field(&win, "class").unwrap_or("").to_string(),
        title: field(&win, "title").unwrap_or("").to_string(),
        windows: parse_clients(&request("clients").unwrap_or_default()),
    })
}

pub enum Event {
    /// Something about workspaces or windows changed: take a new snapshot.
    Changed,
    /// A window wants attention (its address).
    Urgent(String),
}

/// Reads the event socket on a thread and hands relevant events to `send`. Reconnects if Hyprland restarts.
pub fn watch(send: impl Fn(Event) + Send + 'static) {
    std::thread::spawn(move || {
        const CHANGED: [&str; 10] = [
            "workspacev2>>", "createworkspacev2>>", "destroyworkspacev2>>", "openwindow>>", "closewindow>>",
            "movewindowv2>>", "activewindow>>", "windowtitlev2>>", "focusedmonv2>>", "configreloaded>>",
        ];
        loop {
            let Some(stream) = dir().and_then(|d| UnixStream::connect(d.join(".socket2.sock")).ok()) else {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            };
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if let Some(addr) = line.strip_prefix("urgent>>") {
                    send(Event::Urgent(format!("0x{}", addr.trim_start_matches("0x"))));
                } else if CHANGED.iter().any(|p| line.starts_with(p)) {
                    send(Event::Changed);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "workspace 1 (1) on monitor eDP-1:\n\tmonitorID: 0\n\twindows: 2\n\thasfullscreen: 0\n\n\
workspace 4 (4) on monitor eDP-1:\n\tmonitorID: 0\n\twindows: 1\n\n\
workspace 2 (2) on monitor eDP-1:\n\tmonitorID: 0\n\twindows: 0\n\n\
workspace -98 (special:magic) on monitor eDP-1:\n\tmonitorID: 0\n\twindows: 1\n\n";

    #[test]
    fn workspaces_with_windows_sorted_without_special() {
        assert_eq!(parse_workspaces(WS), vec![1, 4]);
    }

    #[test]
    fn clients_map_address_to_workspace() {
        let c = "Window 55b7740038e0 -> sleep:\n\tmapped: 1\n\tworkspace: 3 (3)\n\tclass: probe\n\n\
Window 55b774111111 -> other: thing:\n\tworkspace: 1 (1)\n\n";
        assert_eq!(parse_clients(c), vec![("0x55b7740038e0".into(), 3), ("0x55b774111111".into(), 1)]);
    }

    #[test]
    fn fields_are_found_after_tabs() {
        let w = "Window 1 -> t:\n\tclass: firefox\n\ttitle: GitHub: a: b\n";
        assert_eq!(field(w, "class"), Some("firefox"));
        assert_eq!(field(w, "title"), Some("GitHub: a: b"));
    }
}
