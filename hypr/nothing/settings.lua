APPS = {
    terminal = "kitty",
    files    = "dolphin",
    browser  = "firefox",
    editor   = "kitty -e micro",
    monitor  = "kitty -e btop",
    mixer    = "pavucontrol",
    network  = "kitty -e nmtui",
}

DISPLAY = { output = "eDP-1", mode = "1920x1080@144", scale = 1 }

WORKSPACES = 10 -- SUPER+1..9, SUPER+0 = 10; the bar always shows 5 dots

-- Seconds of inactivity before locking / turning the screen off. 0 = never (the display stays on).
IDLE = { lock = 0, screen_off = 0 }

WALLPAPER = os.getenv("HOME") .. "/.cache/nothing/dots.ppm" -- generated dot-grid tile, always underneath

-- "jarvis": live JARVIS core, native Rust renderer (jarvis-main/core/wall-rs)
-- "jarvis-web": the same core in WebKit (wallpaper/jarvis-wall.py, ~3x the CPU, ~4x the RAM), "dots": static grid only
WALLPAPER_MODE = "jarvis"
JARVIS_WALL = os.getenv("HOME") .. "/.local/bin/jarvis-wall"
JARVIS_WALL_WEB = os.getenv("HOME") .. "/.local/share/jarvis-wall/jarvis-wall.py"

-- Shared by the keybind and the bar's power button. Single quotes only (it is embedded in JSON).
POWER_MENU = [[sh -c 'case $(printf "LOCK\nLOGOUT\nREBOOT\nSHUTDOWN" | fuzzel --dmenu --lines 4 --width 20 --prompt "POWER  ") in LOCK) loginctl lock-session;; LOGOUT) loginctl terminate-session "$XDG_SESSION_ID";; REBOOT) systemctl reboot;; SHUTDOWN) systemctl poweroff;; esac']]
