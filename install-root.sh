#!/bin/bash
# System part of nothing-os: purge any previous Hyprland desktop, install the minimal stack,
# theme the login screen. Run as root (sudo bash install-root.sh). Arch / CachyOS.
set -euo pipefail

purge=(cachyos-hypr-noctalia noctalia noctalia-greeter greetd-agreety uwsm)
present=$(pacman -Qq "${purge[@]}" 2>/dev/null || true)
if [ -n "$present" ]; then
  echo ">> removing: $present"
  pacman -Rns --noconfirm $present | grep -E "^(Packages|Total Removed)" || true
fi

echo ">> installing"
pacman -S --needed --noconfirm \
  hyprland xdg-desktop-portal-hyprland xdg-desktop-portal-gtk hyprlock hypridle hyprpolkitagent hyprpicker \
  waybar fuzzel mako libnotify swaybg cliphist wl-clipboard grim slurp brightnessctl playerctl \
  greetd greetd-tuigreet adw-gtk-theme gnome-keyring kitty pavucontrol micro btop ttf-space-mono-nerd curl \
  python-gobject webkitgtk-6.0 gtk4-layer-shell \
  $(pacman -Si cachyos-packageinstaller >/dev/null 2>&1 && echo cachyos-packageinstaller) \
  | grep -E "^(Packages|Total Installed)" || true

echo ">> login screen: tuigreet, black and red"
cat > /etc/greetd/config.toml <<'TOML'
[terminal]
vt = 1

[default_session]
command = "tuigreet --time --time-format '%H:%M' --remember --asterisks --greeting 'nothing.' --window-padding 2 --container-padding 2 --theme 'border=darkgray;text=gray;prompt=white;time=red;action=darkgray;button=red;container=black;input=white;greet=white;title=white' --cmd start-hyprland"
user = "greeter"
TOML
systemctl enable greetd >/dev/null 2>&1
echo ">> done"
