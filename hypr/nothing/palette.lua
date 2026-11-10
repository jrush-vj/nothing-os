-- Nothing OS: monochrome ground, one red signal. Hex without '#'.
-- Red marks focus and alerts only; everything else is black, grey or white.
PALETTE = {
    black   = "000000", -- canvas (OLED black)
    surface = "0d0d0d", -- cards, popups, notifications
    raised  = "1a1a1a", -- bar, elevated surfaces
    hover   = "242424", -- hover / selected rows
    line    = "2e2e2e", -- 1px borders, inactive windows
    faint   = "2a2a2a", -- wallpaper dot grid
    white   = "ffffff", -- primary text, occupied workspaces
    grey    = "999999", -- labels, secondary text
    dim     = "555555", -- placeholders, disabled
    red     = "d71921", -- Nothing red: focus, alerts
    red_dim = "3a0a0a", -- red-tinted surfaces

    -- terminal ANSI 0-15: muted hues that sit on black
    ansi0 = "1a1a1a", ansi1 = "e5484d", ansi2 = "8fc79a", ansi3 = "d9c27a",
    ansi4 = "8fb3d9", ansi5 = "c79fd9", ansi6 = "8fcccc", ansi7 = "cccccc",
    ansi8 = "555555", ansi9 = "ff5c61", ansi10 = "a8d9b1", ansi11 = "e6d49a",
    ansi12 = "a8c7e6", ansi13 = "d9b8e6", ansi14 = "a8e0e0", ansi15 = "ffffff",
}

FONT = {
    dot  = "Doto",                -- dot-matrix display (clock, lock screen), OFL stand-in for Ndot
    mono = "SpaceMono Nerd Font", -- labels, bar, terminal
    ui   = "Space Grotesk",       -- app text, notifications
    size = 11,
    bar  = "JetBrains Mono",      -- nothing-bar, its control centre, notification popups
}
