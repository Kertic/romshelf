#!/bin/sh
# Installs (or reinstalls) Romshelf on a Steam Deck, from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/Kertic/romshelf/main/scripts/install.sh | sh
#
# Puts the AppImage at ~/Applications/romshelf.AppImage (the fixed path every Steam shortcut
# runs), links ~/.local/bin/romshelf to it, adds a desktop entry, and adds Romshelf itself to
# Steam. Everything stays under your home folder, which SteamOS updates keep.
set -eu

REPO="${ROMSHELF_REPO:-Kertic/romshelf}"
APPS="$HOME/Applications"
DEST="$APPS/romshelf.AppImage"
API="https://api.github.com/repos/$REPO/releases/latest"

say() { printf '%s\n' "$*"; }
fail() { printf 'Romshelf could not be installed: %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null || fail "curl is missing."
command -v sha256sum >/dev/null || fail "sha256sum is missing."

say "Finding the latest Romshelf…"
release=$(curl -fsSL "$API") || fail "GitHub can't be reached. Check the internet connection and try again."
asset_url() {
  printf '%s' "$release" | grep -o "\"browser_download_url\": *\"[^\"]*/$1\"" | head -n 1 | sed 's/.*"\(https[^"]*\)"/\1/'
}
appimage_url=$(asset_url romshelf.AppImage)
manifest_url=$(asset_url romshelf-update.json)
[ -n "$appimage_url" ] || fail "the latest release has no romshelf.AppImage."
[ -n "$manifest_url" ] || fail "the latest release has no romshelf-update.json."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
say "Downloading…"
curl -fL --progress-bar "$appimage_url" -o "$tmp/romshelf.AppImage" || fail "the download stopped. Try again."
curl -fsSL "$manifest_url" -o "$tmp/manifest.json" || fail "the release's checksum file couldn't be downloaded."

# The release's manifest carries the AppImage's SHA-256 (the app checks its signature too when
# it updates itself).
want=$(grep -o '"sha256": *"[0-9a-f]*"' "$tmp/manifest.json" | sed 's/.*"\([0-9a-f]*\)"$/\1/')
have=$(sha256sum "$tmp/romshelf.AppImage" | cut -d' ' -f1)
if [ -z "$want" ] || [ "$want" != "$have" ]; then
  fail "the download doesn't match the release's checksum. Nothing was changed."
fi

mkdir -p "$APPS" "$HOME/.local/bin" "$HOME/.local/share/applications"
chmod +x "$tmp/romshelf.AppImage"
if [ -f "$DEST" ]; then
  cp -p "$DEST" "$DEST.previous"
fi
mv "$tmp/romshelf.AppImage" "$DEST"
ln -sf "$DEST" "$HOME/.local/bin/romshelf"

cat > "$HOME/.local/share/applications/romshelf.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Romshelf
Comment=Your ROMs from Google Drive, played from Steam
Exec=$DEST
Terminal=false
Categories=Game;Emulator;
EOF

say "Adding Romshelf to Steam…"
if "$DEST" steam add-self; then
  say "Restart Steam to see Romshelf in your library (in Game Mode: Steam button, Power, Restart)."
else
  say "Romshelf is installed, but couldn't be added to Steam yet. Start Steam once, then run: romshelf steam add-self"
fi

say ""
say "Romshelf is installed at $DEST"
say "Next: open Romshelf from Steam (or run 'romshelf'), sign in to Google, and choose a Drive folder."
say "Google setup: https://github.com/$REPO/blob/main/docs/google-setup.md"
