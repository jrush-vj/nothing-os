// Notifications: mako shows the popups; the bar keeps a bell (unread count) and a panel listing them.
// New ones are noticed through dbus-monitor (no polling); the list itself comes from makoctl.
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub summary: String,
}

/// makoctl's text format: "Notification 7: Summary" then indented "App name: ..." lines.
pub fn parse(text: &str) -> Vec<Note> {
    let mut out: Vec<Note> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Notification ") {
            if let Some((id, summary)) = rest.split_once(": ") {
                if let Ok(id) = id.parse() {
                    out.push(Note { id, app: String::new(), summary: summary.to_string() });
                }
            }
        } else if let Some(app) = line.trim_start().strip_prefix("App name: ")
            && let Some(n) = out.last_mut()
        {
            n.app = app.to_string();
        }
    }
    out
}

/// Showing and past notifications, newest first, without duplicates.
pub fn read() -> Vec<Note> {
    let get = |verb: &str| Command::new("makoctl").arg(verb).output().map(|o| parse(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();
    let mut all = get("list");
    all.extend(get("history"));
    all.sort_by(|a, b| b.id.cmp(&a.id));
    all.dedup_by_key(|n| n.id);
    all
}

pub fn clear_all() {
    let _ = Command::new("makoctl").args(["dismiss", "-a"]).status();
}

/// Calls `send` whenever any app sends a notification.
pub fn watch(send: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            let child = Command::new("dbus-monitor")
                .args(["--session", "type='method_call',interface='org.freedesktop.Notifications',member='Notify'"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let Ok(mut child) = child else { return };
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if line.contains("member=Notify") {
                        // mako needs a moment to take it in before makoctl lists it.
                        std::thread::sleep(std::time::Duration::from_millis(150));
                        send();
                    }
                }
            }
            let _ = child.wait();
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_makoctl_text() {
        let t = "Notification 7: Updates: 12 new\n  App name: System\n  Urgency: normal\nNotification 6: Download finished\n  App name: firefox\n";
        assert_eq!(
            parse(t),
            vec![
                Note { id: 7, app: "System".into(), summary: "Updates: 12 new".into() },
                Note { id: 6, app: "firefox".into(), summary: "Download finished".into() }
            ]
        );
        assert!(parse("").is_empty());
    }
}
