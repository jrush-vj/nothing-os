-- Short and decisive: no bounce, no overshoot.
hl.curve("out",    { type = "bezier", points = { {0.16, 1}, {0.3, 1} } })
hl.curve("in",     { type = "bezier", points = { {0.7, 0},  {0.84, 0} } })
hl.curve("linear", { type = "bezier", points = { {0, 0},    {1, 1} } })

local anims = {
    -- leaf,                 speed, curve,    style
    { "global",              3,     "out" },
    { "windowsIn",           2.5,   "out",    "popin 95%" },
    { "windowsOut",          1.5,   "in",     "popin 95%" },
    { "windowsMove",         2.5,   "out" },
    { "fade",                2,     "out" },
    { "border",              2,     "linear" },
    { "layers",              2,     "out",    "fade" },
    { "workspaces",          2.5,   "out",    "slide" },
    { "specialWorkspace",    2.5,   "out",    "slidevert" },
}
for _, a in ipairs(anims) do
    hl.animation({ leaf = a[1], enabled = true, speed = a[2], bezier = a[3], style = a[4] })
end
