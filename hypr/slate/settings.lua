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

WORKSPACES = 10 -- SUPER+1..9, SUPER+0 = 10

IDLE = { lock = 300, screen_off = 360 } -- seconds

-- Shared by the keybind and the bar's power button. Single quotes only (it is embedded in JSON).
POWER_MENU = [[sh -c 'case $(printf "lock\nlogout\nreboot\nshutdown" | fuzzel --dmenu --lines 4 --prompt "power  ") in lock) loginctl lock-session;; logout) loginctl terminate-session "$XDG_SESSION_ID";; reboot) systemctl reboot;; shutdown) systemctl poweroff;; esac']]
