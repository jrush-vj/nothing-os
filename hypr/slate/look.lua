-- Flat: no blur, no shadows, no transparency. One accent marks focus.
local c = function(k) return "rgb(" .. PALETTE[k] .. ")" end

hl.monitor({ output = DISPLAY.output, mode = DISPLAY.mode, position = "0x0", scale = DISPLAY.scale })
hl.monitor({ output = "", mode = "preferred", position = "auto-right", scale = 1 }) -- any external screen

hl.config({
    general = {
        layout = "dwindle",
        gaps_in = 4,
        gaps_out = 8,
        border_size = 2,
        resize_on_border = true,
        extend_border_grab_area = 10,
        col = { active_border = c("accent"), inactive_border = c("overlay") },
    },
    decoration = {
        rounding = 4,
        active_opacity = 1,
        inactive_opacity = 1,
        dim_special = 0.5,
        shadow = { enabled = false },
        blur = { enabled = false },
    },
    group = {
        col = {
            border_active = c("accent"), border_inactive = c("overlay"),
            border_locked_active = c("urgent"), border_locked_inactive = c("overlay"),
        },
        groupbar = {
            font_family = FONT.ui,
            col = { active = c("accent"), inactive = c("overlay"), locked_active = c("urgent"), locked_inactive = c("overlay") },
            text_color = c("bg"),
        },
    },
    dwindle = { preserve_split = true },
    misc = {
        background_color = c("bg"), -- solid wallpaper, no wallpaper daemon
        force_default_wallpaper = 0,
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
        font_family = FONT.ui,
        middle_click_paste = false,
        enable_swallow = true,
        swallow_regex = "^(kitty)$",
        focus_on_activate = true,
        vrr = 3, -- adaptive sync for fullscreen games only
    },
    render = { direct_scanout = 2 },
    xwayland = { force_zero_scaling = true },
    ecosystem = { no_update_news = true, no_donation_nag = true },
})
