-- Offline check: runs the whole rice against a fake `hl` and inspects rendered files.
-- usage: HOME=$(mktemp -d) lua test/mock_test.lua hypr
local dir = assert(arg[1], "pass the hypr config dir")
package.path = dir .. "/?.lua;" .. package.path
os.execute("mkdir -p '" .. os.getenv("HOME") .. "/.cache'")

local binds, configs = {}, 0
local function any() return setmetatable({}, { __index = function() return any() end, __call = function() return {} end }) end
hl = setmetatable({
    bind = function(k) binds[#binds + 1] = k; assert(type(k) == "string") end,
    config = function() configs = configs + 1 end,
    get_config = function() return 1.0 end,
    on = function(_, fn) assert(type(fn) == "function") end,
    dsp = any(),
}, { __index = function() return function() end end })

dofile(dir .. "/hyprland.lua")

local home = os.getenv("HOME")
local files = { "waybar/config.jsonc", "waybar/style.css", "fuzzel/fuzzel.ini", "mako/config", "kitty/kitty.conf",
                "hypr/hyprlock.conf", "hypr/hypridle.conf", "gtk-3.0/gtk.css", "gtk-4.0/settings.ini", "kdeglobals" }
for _, rel in ipairs(files) do
    local f = assert(io.open(home .. "/.config/" .. rel), "not rendered: " .. rel)
    local s = f:read("*a"); f:close()
    assert(not s:find("%${"), "unfilled placeholder in " .. rel .. ": " .. (s:match("%${[%w_]+}") or ""))
end

-- wallpaper tile: valid binary PPM of the declared size
local w = assert(io.open(WALLPAPER, "rb"), "wallpaper not rendered"):read("*a")
local sx, sy, body = w:match("^P6\n(%d+) (%d+)\n255\n(.*)$")
assert(sx and #body == sx * sy * 3, "bad PPM tile")

local seen = {}
for _, k in ipairs(binds) do assert(not seen[k], "duplicate bind: " .. k); seen[k] = true end
assert(io.open(home .. "/.cache/nothing-keys.txt"), "cheatsheet missing")
print(("ok: %d binds, %d config blocks, %d files + %sx%s wallpaper tile"):format(#binds, configs, #files, sx, sy))
