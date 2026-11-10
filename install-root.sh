#!/bin/bash
# Clean-slate Hyprland: purge the CachyOS Hyprland/Noctalia desktop, reinstall a minimal stack.
# Run as root. snap-pac snapshots before each pacman transaction.
set -euo pipefail

purge=(cachyos-hypr-noctalia noctalia noctalia-greeter greetd-agreety hyprland hyprpicker uwsm
       xdg-desktop-portal-hyprland hyprland-guiutils)
present=$(pacman -Qq "${purge[@]}" 2>/dev/null || true)
echo ">> removing: $present"
pacman -Rns --noconfirm $present | grep -E "^(Packages|Total Removed)" || true

echo ">> installing slate stack"
pacman -S --needed --noconfirm \
  hyprland xdg-desktop-portal-hyprland xdg-desktop-portal-gtk hyprlock hypridle hyprpolkitagent hyprpicker \
  waybar fuzzel mako libnotify cliphist wl-clipboard grim slurp brightnessctl playerctl \
  greetd greetd-tuigreet adw-gtk-theme gnome-keyring kitty pavucontrol micro btop \
  | grep -E "^(Packages|Total Installed)" || true

echo ">> login screen: tuigreet"
cat > /etc/greetd/config.toml <<'TOML'
[terminal]
vt = 1

[default_session]
command = "tuigreet --time --remember --asterisks --greeting 'slate' --cmd start-hyprland"
user = "greeter"
TOML
systemctl enable greetd >/dev/null 2>&1
command -v tuigreet start-hyprland
hyprctl version 2>/dev/null | head -1 || pacman -Q hyprland
