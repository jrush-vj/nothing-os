-- Flat black, 1-2px lines, no blur, no shadows. Only the focused window is lit red.
local c = function(k) return "rgb(" .. PALETTE[k] .. ")" end

hl.monitor({ output = DISPLAY.output, mode = DISPLAY.mode, position = "0x0", scale = DISPLAY.scale })
hl.monitor({ output = "", mode = "preferred", position = "auto-right", scale = 1 }) -- any external screen

hl.config({
    general = {
        layout = "dwindle",
        gaps_in = 5,
        gaps_out = 10,
        border_size = 2,
        resize_on_border = true,
        extend_border_grab_area = 10,
        col = { active_border = c("red"), inactive_border = c("line") },
    },
    decoration = {
        rounding = 14, -- Nothing phone widget corners
        rounding_power = 2,
        active_opacity = 1,
        inactive_opacity = 1,
        dim_special = 0.6,
        shadow = { enabled = false },
        blur = { enabled = false },
    },
    group = {
        col = {
            border_active = c("red"), border_inactive = c("line"),
            border_locked_active = c("white"), border_locked_inactive = c("line"),
        },
        groupbar = {
            font_family = FONT.mono,
            col = { active = c("red"), inactive = c("raised"), locked_active = c("white"), locked_inactive = c("raised") },
            text_color = c("white"),
        },
    },
    dwindle = { preserve_split = true },
    misc = {
        background_color = c("black"),
        force_default_wallpaper = 0,
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
        font_family = FONT.mono,
        middle_click_paste = false,
        enable_swallow = true,
        swallow_regex = "^(kitty)$",
        focus_on_activate = true,
        animate_manual_resizes = false,
        vrr = 3, -- adaptive sync for fullscreen games only
    },
    render = {
        direct_scanout = 2,
        new_render_scheduling = true, -- smoother frame pacing under load
    },
    xwayland = { force_zero_scaling = true },
    ecosystem = { no_update_news = true, no_donation_nag = true },
})
