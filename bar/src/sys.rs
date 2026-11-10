// System readings from /proc, plus the process plumbing: signals and the single-instance lock.
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Cpu {
    last: Option<(u64, u64)>, // (busy, total) jiffies
}

impl Cpu {
    pub fn new() -> Self {
        let mut c = Self { last: None };
        c.sample();
        c
    }

    /// Percent busy across all cores since the previous call.
    pub fn sample(&mut self) -> Option<f32> {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        let (busy, total) = parse_cpu(&stat)?;
        let pct = self.last.and_then(|(b0, t0)| {
            let dt = total.saturating_sub(t0);
            (dt > 0).then(|| busy.saturating_sub(b0) as f32 / dt as f32 * 100.0)
        });
        self.last = Some((busy, total));
        pct
    }
}

fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
    let v: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 5 {
        return None;
    }
    let total: u64 = v.iter().take(8).sum(); // user..steal; guest is already counted in user
    let idle = v[3] + v[4]; // idle + iowait
    Some((total - idle, total))
}

/// Used RAM in GiB (MemTotal - MemAvailable).
pub fn ram_used_gib() -> Option<f32> {
    let m = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |k: &str| m.lines().find_map(|l| l.strip_prefix(k)?.split_whitespace().next()?.parse::<f64>().ok());
    Some(((kb("MemTotal:")? - kb("MemAvailable:")?) / 1024.0 / 1024.0) as f32)
}

pub static QUIT: AtomicBool = AtomicBool::new(false);
pub static USR1: AtomicBool = AtomicBool::new(false);
pub static USR2: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    if sig == libc::SIGUSR1 {
        USR1.store(true, Ordering::Relaxed);
    } else if sig == libc::SIGUSR2 {
        USR2.store(true, Ordering::Relaxed);
    } else {
        QUIT.store(true, Ordering::Relaxed);
    }
}

pub fn install_signals() {
    for s in [libc::SIGUSR1, libc::SIGUSR2, libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only stores to atomics, which is async-signal-safe.
        unsafe {
            libc::signal(s, on_signal as *const () as libc::sighandler_t);
        }
    }
}

/// One bar per session: a second copy exits instead of stacking another bar.
pub fn single_instance() -> Result<std::fs::File, String> {
    use std::os::fd::AsRawFd;
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let f = std::fs::File::create(format!("{dir}/nothing-bar.lock")).map_err(|e| e.to_string())?;
    // SAFETY: flock on a file descriptor we own.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("already running".into());
    }
    Ok(f)
}

/// Local wall-clock time: (hour, minute, second, weekday 0=Sun, day, month 0..11).
pub fn local_time() -> (u32, u32, u32, u32, u32, u32) {
    // SAFETY: time() and localtime_r() with valid pointers; tm is plain data.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32, tm.tm_wday as u32, tm.tm_mday as u32, tm.tm_mon as u32)
    }
}

/// Starts a program detached from the bar (it survives a bar restart).
pub fn spawn(cmd: &str) {
    let r = std::process::Command::new("sh").args(["-c", &format!("setsid -f {cmd} >/dev/null 2>&1")]).spawn();
    if let Ok(mut child) = r {
        std::thread::spawn(move || child.wait()); // reap the shell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_line_parses_busy_and_total() {
        let (busy, total) = parse_cpu("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3").unwrap();
        assert_eq!((busy, total), (150, 1000));
    }

    #[test]
    fn readings_are_sane_on_linux() {
        let mut c = Cpu::new();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let p = c.sample().unwrap();
        assert!((0.0..=100.0).contains(&p));
        assert!(ram_used_gib().unwrap() > 0.0);
    }
}
