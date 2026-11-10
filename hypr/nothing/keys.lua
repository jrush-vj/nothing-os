-- One table drives the binds, `hyprctl binds` descriptions and the SUPER+/ cheatsheet.
local SUPER = "SUPER"
local HOME  = os.getenv("HOME")
local run   = function(cmd) return hl.dsp.exec_cmd(cmd) end

-- On-screen feedback: mako draws the progress bar, the sync hint replaces the previous bubble
local NOTIFY = "notify-send -a osd -t 1200 -h string:x-canonical-private-synchronous:osd "
local OSD = {
    volume     = NOTIFY .. [[-h int:value:$(wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print int($2*100)}') "VOLUME$(wpctl get-volume @DEFAULT_AUDIO_SINK@ | grep -q MUTED && echo ' · MUTED')"]],
    mic        = NOTIFY .. [["MIC$(wpctl get-volume @DEFAULT_AUDIO_SOURCE@ | grep -q MUTED && echo ' · MUTED' || echo ' · LIVE')"]],
    brightness = NOTIFY .. [[-h int:value:$(brightnessctl -m | cut -d, -f4 | tr -d %) BRIGHTNESS]],
}
local function with_osd(cmd, kind) return run(cmd .. " && " .. OSD[kind]) end

local function shot(grim_args)
    return run(([[mkdir -p ~/Pictures/Screenshots && f=~/Pictures/Screenshots/$(date +%F_%H-%M-%S).png && grim ARGS "$f" && wl-copy < "$f" && notify-send -a nothing -i "$f" "SCREENSHOT" "$f"]]):gsub("ARGS", grim_args, 1))
end

-- Turbo: animations off for games and battery
local turbo = false
local function toggle_turbo()
    turbo = not turbo
    hl.config({ animations = { enabled = not turbo } })
    hl.exec_cmd("pkill -USR1 -x nothing-bar") -- the bar re-reads animations:enabled and stops its own
    hl.exec_cmd("notify-send -a nothing 'TURBO " .. (turbo and "ON" or "OFF") .. "'")
end
-- Global so the bar's control centre can run it: `hyprctl eval "TOGGLE_TURBO()"`.
TOGGLE_TURBO = toggle_turbo

local function zoom(step)
    local z = hl.get_config("cursor:zoom_factor") + step
    hl.config({ cursor = { zoom_factor = math.max(1.0, math.min(3.0, z)) } })
end

-- { key after SUPER, description, action, opts }; a one-item row is a section header.
-- opts.bare = no SUPER prefix
local keys = {
    { "apps" },
    { "Return",          "terminal",                  run(APPS.terminal) },
    { "Space",           "launcher",                  run("fuzzel") },
    { "E",               "files",                     run(APPS.files) },
    { "B",               "browser",                   run(APPS.browser) },
    { "N",               "editor",                    run(APPS.editor) },
    { "SHIFT + Escape",  "system monitor",            run(APPS.monitor) },

    { "windows" },
    { "Q",               "close",                     hl.dsp.window.close() },
    { "F",               "fullscreen",                hl.dsp.window.fullscreen() },
    { "M",               "maximize",                  hl.dsp.window.fullscreen({ mode = 1 }) },
    { "T",               "float / tile",              hl.dsp.window.float({ action = "toggle" }) },
    { "R",               "rotate split",              hl.dsp.layout("togglesplit") },
    { "H",               "focus left",                hl.dsp.focus({ direction = "left" }) },
    { "L",               "focus right",               hl.dsp.focus({ direction = "right" }) },
    { "K",               "focus up",                  hl.dsp.focus({ direction = "up" }) },
    { "J",               "focus down",                hl.dsp.focus({ direction = "down" }) },
    { "Left",            "focus left",                hl.dsp.focus({ direction = "left" }) },
    { "Right",           "focus right",               hl.dsp.focus({ direction = "right" }) },
    { "Up",              "focus up",                  hl.dsp.focus({ direction = "up" }) },
    { "Down",            "focus down",                hl.dsp.focus({ direction = "down" }) },
    { "SHIFT + H",       "move left",                 hl.dsp.window.move({ direction = "l" }) },
    { "SHIFT + L",       "move right",                hl.dsp.window.move({ direction = "r" }) },
    { "SHIFT + K",       "move up",                   hl.dsp.window.move({ direction = "u" }) },
    { "SHIFT + J",       "move down",                 hl.dsp.window.move({ direction = "d" }) },
    { "SHIFT + Left",    "move left",                 hl.dsp.window.move({ direction = "l" }) },
    { "SHIFT + Right",   "move right",                hl.dsp.window.move({ direction = "r" }) },
    { "SHIFT + Up",      "move up",                   hl.dsp.window.move({ direction = "u" }) },
    { "SHIFT + Down",    "move down",                 hl.dsp.window.move({ direction = "d" }) },
    { "mouse:272",       "drag window",               hl.dsp.window.drag() },
    { "mouse:273",       "resize window",             hl.dsp.window.resize() },
    { "ALT + Tab",       "cycle windows",             hl.dsp.window.cycle_next(), { bare = true } },

    { "workspaces" },
    { "1..0",            "go to workspace" },
    { "SHIFT + 1..0",    "send window to workspace" },
    { "Tab",             "previous workspace",        hl.dsp.focus({ workspace = "previous" }) },
    { "CONTROL + Right", "next workspace",            hl.dsp.focus({ workspace = "m+1" }) },
    { "CONTROL + Left",  "previous workspace",        hl.dsp.focus({ workspace = "m-1" }) },
    { "mouse_down",      "next workspace",            hl.dsp.focus({ workspace = "m+1" }) },
    { "mouse_up",        "previous workspace",        hl.dsp.focus({ workspace = "m-1" }) },
    { "S",               "scratchpad",                hl.dsp.workspace.toggle_special() },
    { "SHIFT + S",       "send to scratchpad",        hl.dsp.window.move({ workspace = "special" }) },

    { "system" },
    { "V",               "clipboard history",         run([[cliphist list | fuzzel --dmenu --prompt "CLIPBOARD  " | cliphist decode | wl-copy]]) },
    { "comma",           "dismiss notifications",     run("makoctl dismiss --all") },
    { "SHIFT + comma",   "restore notification",      run("makoctl restore") },
    { "A",               "do not disturb",            run("makoctl mode -t dnd") },
    { "SHIFT + B",       "hide / show bar",           run(BAR == "waybar" and "pkill -SIGUSR1 -x waybar" or "pkill -USR2 -x nothing-bar") },
    { "Escape",          "lock",                      run("loginctl lock-session") },
    { "BackSpace",       "power menu",                run(POWER_MENU) },
    { "SHIFT + R",       "reload desktop",            run("hyprctl reload") },
    { "F12",             "turbo (animations off)",    toggle_turbo },

    { "tools" },
    { "Print",           "screenshot region",         shot([[-g "$(slurp -d)"]]), { bare = true } },
    { "SHIFT + Print",   "screenshot screen",         shot("") },
    { "SHIFT + C",       "colour picker",             run("hyprpicker -a") },
    { "equal",           "zoom in",                   function() zoom(0.3) end,  { repeating = true } },
    { "minus",           "zoom out",                  function() zoom(-0.3) end, { repeating = true } },
    { "slash",           "this cheatsheet",           run("fuzzel --dmenu --prompt 'KEYS  ' --width 60 --lines 22 < " .. HOME .. "/.cache/nothing-keys.txt > /dev/null") },
}

local sheet = {}
for _, k in ipairs(keys) do
    local combo, desc, action, opts = k[1], k[2], k[3], k[4] or {}
    if not desc then
        sheet[#sheet + 1] = "── " .. combo:upper() .. " ──"
    else
        local full = opts.bare and combo or (SUPER .. " + " .. combo)
        sheet[#sheet + 1] = string.format("%-24s %s", full:gsub("mouse:272", "LMB"):gsub("mouse:273", "RMB"), desc)
        if action then hl.bind(full, action, { repeating = opts.repeating, description = desc }) end
    end
end

for i = 1, WORKSPACES do
    local key = tostring(i % 10)
    hl.bind(SUPER .. " + " .. key,         hl.dsp.focus({ workspace = i }),                   { description = "workspace " .. i })
    hl.bind(SUPER .. " + SHIFT + " .. key, hl.dsp.window.move({ workspace = tostring(i) }),   { description = "send to " .. i })
end

-- Hardware keys, also active on the lock screen
local hw = {
    { "XF86AudioRaiseVolume",  with_osd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+", "volume"),   true },
    { "XF86AudioLowerVolume",  with_osd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-", "volume"),        true },
    { "XF86AudioMute",         with_osd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle", "volume") },
    { "XF86AudioMicMute",      with_osd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle", "mic") },
    { "XF86MonBrightnessUp",   with_osd("brightnessctl -q set 5%+", "brightness"),                     true },
    { "XF86MonBrightnessDown", with_osd("brightnessctl -q set 5%-", "brightness"),                     true },
    { "XF86AudioPlay",         run("playerctl play-pause") },
    { "XF86AudioPause",        run("playerctl play-pause") },
    { "XF86AudioNext",         run("playerctl next") },
    { "XF86AudioPrev",         run("playerctl previous") },
}
for _, h in ipairs(hw) do hl.bind(h[1], h[2], { locked = true, repeating = h[3] or false }) end

local f = io.open(HOME .. "/.cache/nothing-keys.txt", "w")
if f then f:write(table.concat(sheet, "\n") .. "\n"); f:close() end
