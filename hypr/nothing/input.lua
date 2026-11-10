hl.config({
    input = {
        kb_layout = "us",
        repeat_rate = 40,
        repeat_delay = 250,
        accel_profile = "flat",
        follow_mouse = 1,
        touchpad = { natural_scroll = true, tap_to_click = true, disable_while_typing = true },
    },
    cursor = { inactive_timeout = 5 },
})

hl.gesture({ fingers = 3, direction = "horizontal", action = "workspace" })
hl.gesture({ fingers = 3, direction = "down",       action = "close" })
hl.gesture({ fingers = 3, direction = "up",         action = "fullscreen" })
