-- slate: a minimal flat Hyprland desktop, written entirely in Lua.
--
--   slate/palette.lua   colours and fonts (the single source of truth)
--   slate/settings.lua  apps, display, workspaces
--   slate/apps.lua      renders waybar, fuzzel, mako, kitty, hyprlock, hypridle, GTK and Qt
--                       configs from the palette, so one edit recolours the whole desktop
--
-- Reload with SUPER+SHIFT+R. Keybind cheatsheet: SUPER+/.
require("slate.palette")
require("slate.settings")
require("slate.env")
require("slate.look")
require("slate.motion")
require("slate.input")
require("slate.keys")
require("slate.rules")
require("slate.apps")
require("slate.autostart")
