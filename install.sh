#!/bin/bash
# User part of nothing-os: fonts + config. Run as your user after install-root.sh.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

echo ">> fonts (OFL, from Google Fonts)"
fonts=~/.local/share/fonts/nothing-os
mkdir -p "$fonts"
base=https://raw.githubusercontent.com/google/fonts/main/ofl
curl -fsSL -o "$fonts/Doto.ttf"         "$base/doto/Doto%5BROND,wght%5D.ttf"
curl -fsSL -o "$fonts/SpaceGrotesk.ttf" "$base/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf"
fc-cache -f "$fonts" >/dev/null
fc-list : family | grep -q "^Doto" && echo "   Doto ok" || echo "   WARNING: Doto font not registered"

echo ">> live wallpaper"
wall=~/.local/share/jarvis-wall
mkdir -p "$wall"
cp "$here/wallpaper/jarvis-wall.py" "$here/wallpaper/index.html" "$wall/"
# jarvis-core.js is not in this repo; point JARVIS_CORE at a jarvis-main/core/web build to enable it
if [ -n "${JARVIS_CORE:-}" ] && [ -f "$JARVIS_CORE/jarvis-core.js" ]; then
  cp "$JARVIS_CORE/jarvis-core.js" "$wall/" && cp -r "$JARVIS_CORE/fonts" "$wall/"
  echo "   jarvis core installed"
elif [ ! -f "$wall/jarvis-core.js" ]; then
  echo "   no JARVIS_CORE given: the dot-grid wallpaper is used (set WALLPAPER_MODE = \"dots\" to silence)"
fi

echo ">> config"
if [ -d ~/.config/hypr ] && [ ! -f ~/.config/hypr/nothing/palette.lua ]; then
  mv ~/.config/hypr ~/.config/hypr.before-nothing-os
  echo "   old ~/.config/hypr moved to ~/.config/hypr.before-nothing-os"
fi
rm -rf ~/.config/hypr/nothing ~/.config/hypr/slate
mkdir -p ~/.config/hypr
cp -r "$here/hypr/." ~/.config/hypr/

if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
  hyprctl reload >/dev/null && hyprctl configerrors
fi
echo ">> done. Log out and back in (or reboot) for the full session."
