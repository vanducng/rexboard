#!/bin/sh
# Installs or updates rexboard from GitHub releases:
#   curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
# REXBOARD_INSTALL_DIR picks the directory (default ~/.local/bin). REXBOARD_VERSION pins a tag like v0.3.0.
set -eu

repo=vanducng/rexboard
dir=${REXBOARD_INSTALL_DIR:-$HOME/.local/bin}

fail() {
  echo "rexboard install: $*" >&2
  exit 1
}

case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) target=aarch64-apple-darwin ;;
  Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
  *) fail "no release build for $(uname -s) $(uname -m); build it with: cargo install --locked --git https://github.com/$repo" ;;
esac

tag=${REXBOARD_VERSION:-}
if [ -z "$tag" ]; then
  latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest") ||
    fail "could not reach github.com"
  tag=${latest##*/}
fi
case "$tag" in v[0-9]*) ;; *) fail "could not find the latest release" ;; esac

if [ -x "$dir/rexboard" ] && [ "$("$dir/rexboard" --version 2>/dev/null)" = "rexboard ${tag#v}" ]; then
  echo "rexboard ${tag#v} is already installed at $dir/rexboard"
  exit 0
fi

archive="rexboard-$tag-$target.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
base="https://github.com/$repo/releases/download/$tag"
echo "downloading rexboard ${tag#v} for $target"
curl -fsSL -o "$tmp/$archive" "$base/$archive" || fail "could not download $archive"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || fail "could not download SHA256SUMS"

if command -v sha256sum >/dev/null; then
  got=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
else
  got=$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)
fi
want=$(grep " $archive\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
[ -n "$want" ] && [ "$got" = "$want" ] || fail "checksum mismatch for $archive"

tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$dir"
cp "$tmp/rexboard-$tag-$target/rexboard" "$dir/.rexboard.new"
chmod 755 "$dir/.rexboard.new"
mv -f "$dir/.rexboard.new" "$dir/rexboard"
echo "installed rexboard ${tag#v} at $dir/rexboard"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "add $dir to PATH, for example: export PATH=\"$dir:\$PATH\"" ;;
esac
