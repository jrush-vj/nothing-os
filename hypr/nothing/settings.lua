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

WALLPAPER = os.getenv("HOME") .. "/.cache/nothing/dots.ppm" -- generated dot-grid tile

-- "nothing": nothing-bar (bar/, Rust; settings in ~/.config/nothing-bar/config), "waybar": the older Waybar bar
BAR = "nothing"
NOTHING_BAR = os.getenv("HOME") .. "/.local/bin/nothing-bar"

-- Shared by the keybind and the bar's power button. Single quotes only (it is embedded in JSON).
POWER_MENU = [[sh -c 'case $(printf "LOCK\nLOGOUT\nREBOOT\nSHUTDOWN" | fuzzel --dmenu --lines 4 --width 20 --prompt "POWER  ") in LOCK) loginctl lock-session;; LOGOUT) loginctl terminate-session "$XDG_SESSION_ID";; REBOOT) systemctl reboot;; SHUTDOWN) systemctl poweroff;; esac']]
