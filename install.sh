#!/bin/sh
# Ferrocord installer / updater (Linux x86_64, no root needed).
#
#   curl -fsSL https://raw.githubusercontent.com/FerroApps/Ferrocord/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/FerroApps/Ferrocord/main/install.sh | sh -s -- --uninstall
#
# The same command installs and updates: it downloads the latest GitHub
# Release, verifies its SHA-256 checksum and installs into the user's home:
#   binary        ${XDG_BIN_HOME:-$HOME/.local/bin}/ferrocord
#   video helper  ${XDG_BIN_HOME:-$HOME/.local/bin}/ferrocord-player (in-chat video; no codec packages needed)
#   login helper  ${XDG_BIN_HOME:-$HOME/.local/bin}/ferrocord-login (Discord's login page; needs WebKitGTK 4.1)
#   desktop entry ${XDG_DATA_HOME:-$HOME/.local/share}/applications/dev.ferro.ferrocord.desktop
#   icon          ${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps/dev.ferro.ferrocord.svg
#   version file  ${XDG_DATA_HOME:-$HOME/.local/share}/ferrocord/installed-version
# --uninstall removes exactly those files (never your chats/settings/keyring).
#
# Testing only: FERRO_RELEASE_API overrides the "latest release" API URL
# (any URL curl understands, e.g. file:///tmp/fake/latest.json).

set -eu

REPO="FerroApps/Ferrocord"
CRATE="ferrocord"
APP_ID="dev.ferro.ferrocord"
APP_NAME="Ferrocord"
API_URL="${FERRO_RELEASE_API:-https://api.github.com/repos/$REPO/releases/latest}"

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
    say "Usage: install.sh [--uninstall]"
    say "  (no option)  install or update $APP_NAME in your home directory"
    say "  --uninstall  remove everything this script installed"
}

[ -n "${HOME:-}" ] || die "HOME is not set"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_DIR/applications"
ICON_DIR="$DATA_DIR/icons/hicolor/scalable/apps"
STATE_DIR="$DATA_DIR/$CRATE"
VERSION_FILE="$STATE_DIR/installed-version"
BIN_PATH="$BIN_DIR/$CRATE"
PLAYER_NAME="$CRATE-player"
PLAYER_PATH="$BIN_DIR/$PLAYER_NAME"
LOGIN_NAME="$CRATE-login"
LOGIN_PATH="$BIN_DIR/$LOGIN_NAME"
DESKTOP_PATH="$APPS_DIR/$APP_ID.desktop"
ICON_PATH="$ICON_DIR/$APP_ID.svg"

refresh_caches() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true
    fi
    if command -v kbuildsycoca6 >/dev/null 2>&1; then
        kbuildsycoca6 >/dev/null 2>&1 || true
    elif command -v kbuildsycoca5 >/dev/null 2>&1; then
        kbuildsycoca5 >/dev/null 2>&1 || true
    fi
}

uninstall() {
    rm -f "$BIN_PATH" "$PLAYER_PATH" "$LOGIN_PATH" "$DESKTOP_PATH" "$ICON_PATH" "$VERSION_FILE"
    # The directory may also hold the app's own data: remove it only if empty.
    rmdir "$STATE_DIR" >/dev/null 2>&1 || true
    refresh_caches
    say "$APP_NAME removed (binary, video and login helpers, desktop entry, icon)."
    say "Your account data and keyring entries were left untouched."
}

fetch_stdout() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$1"
    else
        die "need curl or wget"
    fi
}

fetch_file() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    else
        wget -q -O "$2" "$1"
    fi
}

case "${1:-}" in
    "") ;;
    --uninstall) uninstall; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac

[ "$(uname -s)" = "Linux" ] || die "$APP_NAME only supports Linux (found $(uname -s))"
[ "$(uname -m)" = "x86_64" ] || die "$APP_NAME only ships x86_64 builds (found $(uname -m))"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP

say "Looking up the latest $APP_NAME release..."
fetch_stdout "$API_URL" > "$TMP/release.json" || die "could not fetch $API_URL"

urls="$(grep -o '"browser_download_url": *"[^"]*"' "$TMP/release.json" | sed 's/^"browser_download_url": *"\(.*\)"$/\1/')"
TAR_URL="$(printf '%s\n' "$urls" | grep 'linux-x86_64\.tar\.gz$' | head -n 1 || true)"
SUM_URL="$(printf '%s\n' "$urls" | grep 'linux-x86_64\.tar\.gz\.sha256$' | head -n 1 || true)"
[ -n "$TAR_URL" ] || die "latest release has no linux-x86_64 tarball"
[ -n "$SUM_URL" ] || die "latest release has no .sha256 checksum file"

TAG="$(grep -o '"tag_name": *"[^"]*"' "$TMP/release.json" | head -n 1 | sed 's/^"tag_name": *"\(.*\)"$/\1/')"
NEW_VERSION="${TAG#v}"
[ -n "$NEW_VERSION" ] || die "could not read the release version"

OLD_VERSION=""
if [ -f "$VERSION_FILE" ]; then
    OLD_VERSION="$(head -n 1 "$VERSION_FILE" 2>/dev/null || true)"
fi

TAR_NAME="${TAR_URL##*/}"
SUM_NAME="${SUM_URL##*/}"
say "Downloading $TAR_NAME..."
fetch_file "$TAR_URL" "$TMP/$TAR_NAME" || die "download failed: $TAR_URL"
fetch_file "$SUM_URL" "$TMP/$SUM_NAME" || die "download failed: $SUM_URL"

say "Verifying checksum..."
(cd "$TMP" && sha256sum -c "$SUM_NAME" >/dev/null 2>&1) \
    || die "checksum mismatch for $TAR_NAME, aborting (nothing was installed)"

mkdir "$TMP/x"
tar -xzf "$TMP/$TAR_NAME" -C "$TMP/x" || die "could not extract $TAR_NAME"
SRC="$(find "$TMP/x" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[ -n "$SRC" ] || die "unexpected tarball layout"
[ -f "$SRC/$CRATE" ] || die "tarball has no $CRATE binary"
[ -f "$SRC/packaging/$APP_ID.desktop" ] || die "tarball has no desktop entry"
[ -f "$SRC/packaging/$APP_ID.svg" ] || die "tarball has no icon"

mkdir -p "$BIN_DIR" "$APPS_DIR" "$ICON_DIR" "$STATE_DIR"

# Atomic replace: copy to a temporary name next to the target, then rename.
cp "$SRC/$CRATE" "$BIN_DIR/.$CRATE.new.$$"
chmod 755 "$BIN_DIR/.$CRATE.new.$$"
mv -f "$BIN_DIR/.$CRATE.new.$$" "$BIN_PATH"

# The video helper ships from v0.9; older tarballs have none (videos then
# offer "Open externally" only). It is installed next to the app: the app
# looks for it in its own directory.
if [ -f "$SRC/$PLAYER_NAME" ]; then
    cp "$SRC/$PLAYER_NAME" "$BIN_DIR/.$PLAYER_NAME.new.$$"
    chmod 755 "$BIN_DIR/.$PLAYER_NAME.new.$$"
    mv -f "$BIN_DIR/.$PLAYER_NAME.new.$$" "$PLAYER_PATH"
fi

# The login helper ships from v0.12: Discord's login page (with its captcha)
# in the system webview. Installed next to the app, like the video helper.
if [ -f "$SRC/$LOGIN_NAME" ]; then
    cp "$SRC/$LOGIN_NAME" "$BIN_DIR/.$LOGIN_NAME.new.$$"
    chmod 755 "$BIN_DIR/.$LOGIN_NAME.new.$$"
    mv -f "$BIN_DIR/.$LOGIN_NAME.new.$$" "$LOGIN_PATH"
fi

# Desktop entry: keep the field code of the shipped Exec=, use the absolute path.
exec_args="$(sed -n 's/^Exec=[^ ]*//p' "$SRC/packaging/$APP_ID.desktop" | head -n 1)"
case "$BIN_PATH" in
    *" "*) exec_bin="\"$BIN_PATH\"" ;;
    *) exec_bin="$BIN_PATH" ;;
esac
EXEC_LINE="Exec=$exec_bin$exec_args" awk '/^Exec=/ { print ENVIRON["EXEC_LINE"]; next } { print }' \
    "$SRC/packaging/$APP_ID.desktop" > "$APPS_DIR/.$APP_ID.desktop.new.$$"
mv -f "$APPS_DIR/.$APP_ID.desktop.new.$$" "$DESKTOP_PATH"

cp "$SRC/packaging/$APP_ID.svg" "$ICON_DIR/.$APP_ID.svg.new.$$"
mv -f "$ICON_DIR/.$APP_ID.svg.new.$$" "$ICON_PATH"

printf '%s\n' "$NEW_VERSION" > "$VERSION_FILE"
refresh_caches

if [ -z "$OLD_VERSION" ]; then
    say "Installed $APP_NAME $NEW_VERSION to $BIN_PATH"
elif [ "$OLD_VERSION" = "$NEW_VERSION" ]; then
    say "$APP_NAME is up to date (reinstalled $NEW_VERSION) at $BIN_PATH"
else
    say "Updated $APP_NAME $OLD_VERSION -> $NEW_VERSION at $BIN_PATH"
fi

case ":${PATH:-}:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR is not on your PATH; add it to your shell profile to run '$CRATE' from a terminal." ;;
esac

say "Runtime libraries (usually already present on a desktop install):"
say "  Linux: libssl3 (OpenSSL 3), libxkbcommon0, libxkbcommon-x11-0 (X11 sessions), libwayland-client0, libfontconfig1, libx11-6; a Secret Service provider such as gnome-keyring or KWallet for the keyring."
say "Video: videos play inside the chat with the bundled decoders (MP4 with H.264/AAC, WebM with VP8/VP9/Opus); no codec packages are needed, only ALSA's libasound2 for sound. Other formats offer 'Open externally'."
say "Login: scan the QR code with the Discord app, or use 'Log in with Discord's page' (needs WebKitGTK 4.1)."
if [ -f "$LOGIN_PATH" ] && command -v ldconfig >/dev/null 2>&1 \
    && ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4\.1\.so\.0'; then
    warn "WebKitGTK 4.1 was not found; 'Log in with Discord's page' needs it (Debian/Ubuntu: libwebkit2gtk-4.1-0, Fedora: webkit2gtk4.1, Arch: webkit2gtk-4.1). QR login works without it."
fi
say "Run it from your app menu or with: $CRATE"
