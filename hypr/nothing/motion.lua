-- Nothing motion: cubic-bezier only, 150-300ms, no bounce, no spring.
-- Windows opening and tiles reflowing share one curve and duration, so neighbours
-- move in lockstep with the new window instead of overlapping it.
hl.curve("ease",   { type = "bezier", points = { {0.25, 1}, {0.5, 1} } })  -- easeOutQuart
hl.curve("exit",   { type = "bezier", points = { {0.5, 0},  {0.75, 0} } }) -- easeInQuad
hl.curve("linear", { type = "bezier", points = { {0, 0},    {1, 1} } })

-- speed is in 100ms units
local anims = {
    -- leaf,               speed, curve,    style
    { "global",            3,     "ease" },
    { "windowsIn",         2.5,   "ease",   "popin 92%" },
    { "windowsMove",       2.5,   "ease" },
    { "windowsOut",        1.5,   "exit",   "popin 92%" },
    { "fadeIn",            2.5,   "ease" },
    { "fadeOut",           1.5,   "exit" },
    { "fadeSwitch",        2,     "ease" },
    { "fadeDim",           2,     "ease" },
    { "border",            2,     "ease" },
    { "layersIn",          1.5,   "ease",   "fade" },
    { "layersOut",         1,     "exit",   "fade" },
    { "fadeLayersIn",      1.5,   "ease" },
    { "fadeLayersOut",     1,     "exit" },
    { "workspaces",        3,     "ease",   "slide" },
    { "specialWorkspace",  2.5,   "ease",   "slidevert" },
    { "zoomFactor",        2,     "ease" },
}
for _, a in ipairs(anims) do
    hl.animation({ leaf = a[1], enabled = true, speed = a[2], bezier = a[3], style = a[4] })
end
