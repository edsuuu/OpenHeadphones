#!/bin/sh
# Installs (or updates) JBL Headphones from the latest GitHub release:
#   curl -fsSL https://edsuuu.github.io/jbl-headphones/install.sh | sh
# macOS: the .app in /Applications (or ~/Applications). curl leaves no quarantine flag, so it opens
# without the "unidentified developer" dance.
# Linux: ~/.local/share/jbl-headphones, `jbl-headphones` in ~/.local/bin and a menu entry. The app
# can then update itself there.
set -e
base=https://github.com/edsuuu/jbl-headphones/releases/latest/download
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "$(uname -s)" in
Darwin)
    curl -fL --progress-bar -o "$tmp/app.zip" "$base/jbl-headphones-mac.zip"
    dest=/Applications
    [ -w "$dest" ] || dest="$HOME/Applications"
    mkdir -p "$dest"
    pkill -x jbl-headphones 2>/dev/null || true
    rm -rf "$dest/JBL Headphones.app"
    ditto -x -k "$tmp/app.zip" "$dest"
    echo "Installed: $dest/JBL Headphones.app"
    open "$dest/JBL Headphones.app"
    ;;
Linux)
    case "$(uname -m)" in
    x86_64 | amd64) arch=x64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) echo "No Linux build for $(uname -m) (x86_64 and arm64 only)." >&2; exit 1 ;;
    esac
    data="${XDG_DATA_HOME:-$HOME/.local/share}"
    dir="$data/jbl-headphones"
    curl -fL --progress-bar -o "$tmp/app.tar.gz" "$base/jbl-headphones-linux-$arch.tar.gz"
    pkill -x jbl-headphones 2>/dev/null || true
    rm -rf "$dir"
    mkdir -p "$dir" "$HOME/.local/bin" "$data/applications"
    tar -xzf "$tmp/app.tar.gz" -C "$dir"
    ln -sf "$dir/jbl-headphones" "$HOME/.local/bin/jbl-headphones"
    cat > "$data/applications/jbl-headphones.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=JBL Headphones
Comment=Battery, noise cancelling, equalizer and settings of JBL headphones
Exec=$dir/jbl-headphones
Icon=audio-headphones
Categories=AudioVideo;Audio;Settings;
EOF
    echo "Installed: $dir (run: jbl-headphones, or JBL Headphones in the app menu)"
    case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "Add ~/.local/bin to your PATH to run it by name." ;; esac
    ;;
*)
    echo "On Windows, download https://github.com/edsuuu/jbl-headphones/releases/latest/download/jbl-headphones-windows-x64.zip" >&2
    exit 1
    ;;
esac
