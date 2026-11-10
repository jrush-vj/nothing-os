// Settings: ~/.config/nothing-bar/config, plain `key = value` lines (# comments). Re-read whenever the file
// changes, so edits (by hand or from the settings page) apply live. Unknown keys and bad values keep the default.
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Position {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Solid,
    Translucent,
    /// Translucent; Hyprland blurs behind the `nothing-bar` layer (layer rule in the rice).
    Frosted,
    Clear,
    Fade,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClockFormat {
    HhMm,
    HhMmSs,
    Date,
    TwelveHour,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub height: u32,
    pub position: Position,
    pub material: Material,
    pub clock: ClockFormat,
    pub hairline: bool,
    pub font: String,
    pub font_px: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            height: 28,
            position: Position::Top,
            material: Material::Solid,
            clock: ClockFormat::HhMm,
            hairline: true,
            font: "JetBrains Mono".into(),
            font_px: 12.0,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            match k {
                "height" => c.height = v.parse().ok().filter(|h| (16..=64).contains(h)).unwrap_or(c.height),
                "position" => match v {
                    "top" => c.position = Position::Top,
                    "bottom" => c.position = Position::Bottom,
                    _ => {}
                },
                "material" => match v {
                    "solid" => c.material = Material::Solid,
                    "translucent" => c.material = Material::Translucent,
                    "frosted" => c.material = Material::Frosted,
                    "clear" => c.material = Material::Clear,
                    "fade" => c.material = Material::Fade,
                    _ => {}
                },
                "clock" => match v {
                    "hh:mm" => c.clock = ClockFormat::HhMm,
                    "hh:mm:ss" => c.clock = ClockFormat::HhMmSs,
                    "date" => c.clock = ClockFormat::Date,
                    "12h" => c.clock = ClockFormat::TwelveHour,
                    _ => {}
                },
                "hairline" => c.hairline = v != "off" && v != "false",
                "font" if !v.is_empty() => c.font = v.to_string(),
                "font_px" => c.font_px = v.parse().ok().filter(|p| (8.0..=24.0).contains(p)).unwrap_or(c.font_px),
                _ => {}
            }
        }
        c
    }
}

const HEIGHTS: [u32; 4] = [22, 28, 34, 40];
const POSITIONS: [(Position, &str); 2] = [(Position::Top, "top"), (Position::Bottom, "bottom")];
const MATERIALS: [(Material, &str); 5] = [
    (Material::Solid, "solid"),
    (Material::Translucent, "translucent"),
    (Material::Frosted, "frosted"),
    (Material::Clear, "clear"),
    (Material::Fade, "fade"),
];
const CLOCKS: [(ClockFormat, &str); 4] = [
    (ClockFormat::HhMm, "hh:mm"),
    (ClockFormat::HhMmSs, "hh:mm:ss"),
    (ClockFormat::Date, "date"),
    (ClockFormat::TwelveHour, "12h"),
];

fn name<T: PartialEq + Copy>(table: &[(T, &'static str)], v: T) -> &'static str {
    table.iter().find(|(t, _)| *t == v).map_or("?", |(_, n)| n)
}

fn next<T: PartialEq + Copy>(table: &[(T, &'static str)], v: T) -> T {
    let i = table.iter().position(|(t, _)| *t == v).unwrap_or(0);
    table[(i + 1) % table.len()].0
}

impl Config {
    /// The settings page's rows: (label, current value).
    pub fn rows(&self) -> [(&'static str, String); 5] {
        [
            ("HEIGHT", format!("{} PX", self.height)),
            ("POSITION", name(&POSITIONS, self.position).to_uppercase()),
            ("MATERIAL", name(&MATERIALS, self.material).to_uppercase()),
            ("CLOCK", name(&CLOCKS, self.clock).to_uppercase()),
            ("HAIRLINE", if self.hairline { "ON".into() } else { "OFF".into() }),
        ]
    }

    /// Steps row `i` to its next value.
    pub fn cycle(&mut self, i: usize) {
        match i {
            0 => self.height = HEIGHTS[(HEIGHTS.iter().position(|h| *h == self.height).map_or(0, |p| p + 1)) % HEIGHTS.len()],
            1 => self.position = next(&POSITIONS, self.position),
            2 => self.material = next(&MATERIALS, self.material),
            3 => self.clock = next(&CLOCKS, self.clock),
            4 => self.hairline = !self.hairline,
            _ => {}
        }
    }

    pub fn render(&self) -> String {
        format!(
            "# nothing-bar settings (also changed from the control centre). Changes apply when the file is saved.
             # height 16..64 | position top, bottom | material solid, translucent, frosted, clear, fade
             # clock hh:mm, hh:mm:ss, date, 12h | hairline on, off
             height = {}
position = {}
material = {}
clock = {}
hairline = {}
font = {}
font_px = {}
",
            self.height,
            name(&POSITIONS, self.position),
            name(&MATERIALS, self.material),
            name(&CLOCKS, self.clock),
            if self.hairline { "on" } else { "off" },
            self.font,
            self.font_px,
        )
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, self.render())
    }
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("nothing-bar").join("config")
}

/// Watches the config file's modification time.
pub struct Watcher {
    seen: Option<SystemTime>,
}

impl Watcher {
    pub fn new() -> Self {
        Self { seen: None }
    }

    /// The config if the file changed since the last call (or on the first call); defaults if there's no file.
    pub fn poll(&mut self) -> Option<Config> {
        let p = path();
        let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
        if self.seen.is_some() && mtime == self.seen {
            return None;
        }
        let first = self.seen.is_none();
        self.seen = mtime.or(Some(SystemTime::UNIX_EPOCH));
        if !first && mtime.is_none() {
            return None;
        }
        Some(std::fs::read_to_string(&p).map(|t| Config::parse(&t)).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values_and_ignores_junk() {
        let c = Config::parse("# comment\nheight = 34\nposition=bottom\nmaterial = \"frosted\"  # inline\nclock = 12h\nhairline = off\nbogus = 1\nheight = 999\n");
        assert_eq!(c.height, 34);
        assert_eq!(c.position, Position::Bottom);
        assert_eq!(c.material, Material::Frosted);
        assert_eq!(c.clock, ClockFormat::TwelveHour);
        assert!(!c.hairline);
    }

    #[test]
    fn render_round_trips_and_cycles_wrap() {
        let mut c = Config::default();
        for i in 0..5 {
            c.cycle(i);
        }
        assert_eq!(Config::parse(&c.render()), c);
        assert_eq!((c.height, c.position, c.material, c.clock, c.hairline), (34, Position::Bottom, Material::Translucent, ClockFormat::HhMmSs, false));
        for _ in 0..4 {
            c.cycle(0);
        }
        assert_eq!(c.height, 34);
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(Config::parse(""), Config::default());
    }
}
