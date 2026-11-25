#!/bin/sh
# stdlinux installer: downloads the release binary, verifies it, runs `stdlinux install`.
#   curl -fsSL https://raw.githubusercontent.com/23iq/stdlinux/main/install.sh | sh
set -eu

REPO="${STDLINUX_REPO:-23iq/stdlinux}"
BIN_DIR="${STDLINUX_BIN_DIR:-$HOME/.local/bin}"
ASSET="stdlinux-x86_64-linux"
BASE="${STDLINUX_BASE_URL:-https://github.com/$REPO/releases/latest/download}"

say() { printf '  %s\n' "$*"; }
die() { printf '  error: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Linux ] || die "stdlinux only runs on Linux"
[ "$(uname -m)" = x86_64 ] || die "only x86_64 is supported (this is $(uname -m))"
[ "$(id -u)" != 0 ] || die "run this as your normal user, not root"

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  die "curl or wget is required"
fi
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "downloading stdlinux"
fetch "$BASE/$ASSET" "$tmp/$ASSET" || die "download failed: $BASE/$ASSET"
fetch "$BASE/$ASSET.sha256" "$tmp/$ASSET.sha256" || die "checksum download failed"

want=$(cut -d' ' -f1 "$tmp/$ASSET.sha256")
got=$(sha256sum "$tmp/$ASSET" | cut -d' ' -f1)
[ "$want" = "$got" ] || die "checksum mismatch (expected $want, got $got)"

mkdir -p "$BIN_DIR"
install -m 755 "$tmp/$ASSET" "$BIN_DIR/stdlinux"
say "installed $BIN_DIR/stdlinux"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "note: $BIN_DIR is not in your PATH; add it to your shell profile" ;;
esac

[ -z "${STDLINUX_NO_RUN:-}" ] || exit 0

# Under `curl | sh` stdin is the script, so questions are read from the terminal.
if [ -r /dev/tty ]; then
  exec "$BIN_DIR/stdlinux" install "$@" </dev/tty
else
  exec "$BIN_DIR/stdlinux" install --yes "$@"
fi
