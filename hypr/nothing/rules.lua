local function float(match) hl.window_rule({ match = match, float = true }) end

-- Dialogs and small utilities float
for _, m in ipairs({
    { title = "^(Open|Open File|Save As|Choose Files|Authentication Required|File Operation Progress|Confirm to replace files)$" },
    { class = "^(org.pulseaudio.pavucontrol|nm-connection-editor|xdg-desktop-portal-gtk|hyprland-share-picker|hyprpolkitagent)$" },
    { class = "^(.*dialog.*)$" },
}) do float(m) end
hl.window_rule({ match = { float = true }, persistent_size = true })
hl.window_rule({ match = { class = "^(org.pulseaudio.pavucontrol)$" }, center = true, size = { "monitor_w*0.4", "monitor_h*0.5" } })

-- Picture-in-picture pinned on top
hl.window_rule({
    match = { title = "^([Pp]icture[-\\s]?[Ii]n[-\\s]?[Pp]icture)(.*)$" },
    float = true, pin = true, keep_aspect_ratio = true,
    size = { "monitor_w*0.25", "monitor_h*0.25" },
})

-- Games: own workspace, no border, real fullscreen
hl.window_rule({ match = { class = "^(steam_app.*|gamescope)$" }, workspace = "name:gaming", content = "game", decorate = false, fullscreen_state = 2 })
hl.window_rule({ match = { content = "game" }, workspace = "name:gaming" })

-- Tiling decides sizes, not apps
hl.window_rule({ name = "no-maximize", match = { class = ".*" }, suppress_event = "maximize" })

-- XWayland drag ghosts must not steal focus
hl.window_rule({ name = "xwayland-drag", match = { class = "^$", title = "^$", xwayland = true, float = true, fullscreen = false, pin = false }, no_focus = true })
