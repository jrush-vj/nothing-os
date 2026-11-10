-- nothing-os: a Nothing Phone styled Hyprland desktop, written entirely in Lua.
--
--   nothing/palette.lua   colours and fonts (the single source of truth)
--   nothing/settings.lua  apps, display, workspaces, idle
--   nothing/apps.lua      renders waybar, fuzzel, mako, kitty, hyprlock, hypridle, GTK, Qt
--                         and the dot-grid wallpaper from the palette
--
-- Reload: SUPER+SHIFT+R. Keybind cheatsheet: SUPER+/.
require("nothing.palette")
require("nothing.settings")
require("nothing.env")
require("nothing.look")
require("nothing.motion")
require("nothing.input")
require("nothing.keys")
require("nothing.rules")
require("nothing.apps")
require("nothing.autostart")
