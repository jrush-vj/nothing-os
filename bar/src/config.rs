// Settings: ~/.config/nothing-bar/config, plain `key = value` lines (# comments). Re-read whenever the file
// changes, so edits (by hand or from the settings page) apply live. Unknown keys and bad values keep the default.
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Position {
    Top,
    Bottom,
    /// Top, out of the way until the pointer touches the top edge.
    AutoHide,
    /// Top; collapses to the centre island while the active window is maximized or fullscreen.
    Shrink,
}

impl Position {
    pub fn at_top(self) -> bool {
        self != Position::Bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Strip,
    Floating,
    Islands,
    Island,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WsStyle {
    Red,
    Block,
    Dot,
    Pill,
    Dots,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Solid,
    Translucent,
    /// Translucent; Hyprland blurs behind the `nothing-bar` layer (layer rule in the rice).
    Frosted,
    Clear,
    Fade,
    Light,
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
    pub shape: Shape,
    pub workspaces: WsStyle,
    /// Scales the whole UI (bar, control centre, tooltips); `height` and `font_px` are the sizes at 1.0.
    pub size: f32,
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
            shape: Shape::Strip,
            workspaces: WsStyle::Red,
            // 130%: a 36 px bar with 15.6 px text, comfortable on a 15.6" 1080p laptop panel.
            size: 1.3,
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
                "position" => c.position = by_name(&POSITIONS, v).unwrap_or(c.position),
                "material" => c.material = by_name(&MATERIALS, v).unwrap_or(c.material),
                "clock" => c.clock = by_name(&CLOCKS, v).unwrap_or(c.clock),
                "shape" => c.shape = by_name(&SHAPES, v).unwrap_or(c.shape),
                "size" => {
                    c.size = by_name(&SIZES, v).or_else(|| v.trim_end_matches('%').parse::<f32>().ok().map(|n| if n > 4.0 { n / 100.0 } else { n }))
                        .filter(|z| (0.75..=2.0).contains(z))
                        .unwrap_or(c.size)
                }
                "workspaces" => c.workspaces = by_name(&WS_STYLES, v).unwrap_or(c.workspaces),
                "hairline" => c.hairline = v != "off" && v != "false",
                "font" if !v.is_empty() => c.font = v.to_string(),
                "font_px" => c.font_px = v.parse().ok().filter(|p| (8.0..=24.0).contains(p)).unwrap_or(c.font_px),
                _ => {}
            }
        }
        c
    }
}

const SIZES: [(f32, &str); 4] = [(1.0, "small"), (1.15, "medium"), (1.3, "large"), (1.45, "xlarge")];
const POSITIONS: [(Position, &str); 4] = [
    (Position::Top, "top"),
    (Position::Bottom, "bottom"),
    (Position::AutoHide, "autohide"),
    (Position::Shrink, "shrink"),
];
const MATERIALS: [(Material, &str); 6] = [
    (Material::Solid, "solid"),
    (Material::Translucent, "translucent"),
    (Material::Frosted, "frosted"),
    (Material::Clear, "clear"),
    (Material::Fade, "fade"),
    (Material::Light, "light"),
];
const SHAPES: [(Shape, &str); 4] = [(Shape::Strip, "strip"), (Shape::Floating, "floating"), (Shape::Islands, "islands"), (Shape::Island, "island")];
const WS_STYLES: [(WsStyle, &str); 5] = [
    (WsStyle::Red, "red"),
    (WsStyle::Block, "block"),
    (WsStyle::Dot, "dot"),
    (WsStyle::Pill, "pill"),
    (WsStyle::Dots, "dots"),
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

fn by_name<T: Copy>(table: &[(T, &'static str)], v: &str) -> Option<T> {
    table.iter().find(|(_, n)| *n == v).map(|(t, _)| *t)
}

fn next<T: PartialEq + Copy>(table: &[(T, &'static str)], v: T) -> T {
    let i = table.iter().position(|(t, _)| *t == v).unwrap_or(0);
    table[(i + 1) % table.len()].0
}

impl Config {
    /// The settings page's rows: (label, current value).
    pub fn rows(&self) -> [(&'static str, String); 7] {
        [
            ("SHAPE", name(&SHAPES, self.shape).to_uppercase()),
            ("SIZE", format!("{:.0}%", self.size * 100.0)),
            ("POSITION", name(&POSITIONS, self.position).to_uppercase()),
            ("MATERIAL", name(&MATERIALS, self.material).to_uppercase()),
            ("WORKSPACES", name(&WS_STYLES, self.workspaces).to_uppercase()),
            ("CLOCK", name(&CLOCKS, self.clock).to_uppercase()),
            ("HAIRLINE", if self.hairline { "ON".into() } else { "OFF".into() }),
        ]
    }

    /// Steps row `i` to its next value.
    pub fn cycle(&mut self, i: usize) {
        match i {
            0 => self.shape = next(&SHAPES, self.shape),
            1 => {
                let i = SIZES.iter().position(|(z, _)| (z - self.size).abs() < 0.01).map_or(0, |i| i + 1);
                self.size = SIZES[i % SIZES.len()].0;
            }
            2 => self.position = next(&POSITIONS, self.position),
            3 => self.material = next(&MATERIALS, self.material),
            4 => self.workspaces = next(&WS_STYLES, self.workspaces),
            5 => self.clock = next(&CLOCKS, self.clock),
            6 => self.hairline = !self.hairline,
            _ => {}
        }
    }

    pub fn render(&self) -> String {
        [
            "# nothing-bar settings (also changed from the control centre). Changes apply when the file is saved.".to_string(),
            "# shape strip, floating, islands, island | size small, medium, large, xlarge (or a percentage)".into(),
            "# position top, bottom, autohide, shrink | height and font_px are the sizes at 100%".into(),
            "# material solid, translucent, frosted, clear, fade, light | workspaces red, block, dot, pill, dots".into(),
            "# clock hh:mm, hh:mm:ss, date, 12h | hairline on, off".into(),
            format!("shape = {}", name(&SHAPES, self.shape)),
            format!("size = {}", SIZES.iter().find(|(z, _)| (z - self.size).abs() < 0.01).map_or(format!("{:.0}%", self.size * 100.0), |(_, n)| n.to_string())),
            format!("height = {}", self.height),
            format!("position = {}", name(&POSITIONS, self.position)),
            format!("material = {}", name(&MATERIALS, self.material)),
            format!("workspaces = {}", name(&WS_STYLES, self.workspaces)),
            format!("clock = {}", name(&CLOCKS, self.clock)),
            format!("hairline = {}", if self.hairline { "on" } else { "off" }),
            format!("font = {}", self.font),
            format!("font_px = {}", self.font_px),
        ]
        .join("\n")
            + "\n"
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
        for i in 0..7 {
            c.cycle(i);
        }
        assert_eq!(Config::parse(&c.render()), c);
        assert!(c.render().lines().all(|l| !l.starts_with(' ')));
        assert_eq!((c.shape, c.size, c.position, c.material), (Shape::Floating, 1.45, Position::Bottom, Material::Translucent));
        assert_eq!((c.workspaces, c.clock, c.hairline), (WsStyle::Block, ClockFormat::HhMmSs, false));
        for _ in 0..4 {
            c.cycle(1);
        }
        assert_eq!(c.size, 1.45);
        assert_eq!(Config::parse("size = 120%").size, 1.2);
        assert_eq!(Config::parse("size = 9").size, 1.3, "out of range keeps the default");
        let c = Config::parse("position = autohide\nmaterial = light\nshape = island\nworkspaces = dots");
        assert_eq!((c.position, c.material, c.shape, c.workspaces), (Position::AutoHide, Material::Light, Shape::Island, WsStyle::Dots));
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(Config::parse(""), Config::default());
    }
}
