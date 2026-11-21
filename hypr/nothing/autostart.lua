hl.on("hyprland.start", function()
    hl.exec_cmd("dbus-update-activation-environment --systemd --all")
    hl.exec_cmd("systemctl --user start hyprpolkitagent")
    -- The wallpaper picked in SUPER+W (line 2 of its file), else the generated dot-grid tile.
    local f = io.open(os.getenv("HOME") .. "/.config/nothing-bar/wallpaper")
    local picked = f and select(2, f:read("l", "l"))
    if f then f:close() end
    local ok = picked and io.open(picked)
    if ok then
        ok:close()
        hl.exec_cmd("swaybg -m fill -c '#" .. PALETTE.black .. "' -i '" .. picked:gsub("'", "") .. "'")
    else
        hl.exec_cmd("swaybg -m tile -c '#" .. PALETTE.black .. "' -i " .. WALLPAPER)
    end
    hl.exec_cmd(BAR == "waybar" and "waybar" or NOTHING_BAR)
    hl.exec_cmd("mako")
    hl.exec_cmd("hypridle")
    hl.exec_cmd("wl-paste --watch cliphist store")
    hl.exec_cmd("gsettings set org.gnome.desktop.interface color-scheme prefer-dark")
    hl.exec_cmd("gsettings set org.gnome.desktop.interface gtk-theme adw-gtk3-dark")
    hl.exec_cmd("gsettings set org.gnome.desktop.interface cursor-theme Bibata-Modern-Ice")
    hl.exec_cmd("gsettings set org.gnome.desktop.interface font-name '" .. FONT.ui .. " 10'")
    hl.exec_cmd("gsettings set org.gnome.desktop.interface monospace-font-name '" .. FONT.mono .. " 10'")
end)
