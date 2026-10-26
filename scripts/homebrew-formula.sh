#!/bin/sh
# Prints Formula/rexboard.rb for a release tag from that release's SHA256SUMS.
# Usage: scripts/homebrew-formula.sh v0.2.0 SHA256SUMS > rexboard.rb
set -eu

tag=$1
sums=$2
base="https://github.com/vanducng/rexboard/releases/download/$tag"

sha() {
  awk -v file="rexboard-$tag-$1.tar.gz" '$2 == file { print $1 }' "$sums"
}
mac=$(sha aarch64-apple-darwin)
linux=$(sha x86_64-unknown-linux-gnu)
[ -n "$mac" ] && [ -n "$linux" ] || {
  echo "$sums has no sha256 for both $tag tarballs" >&2
  exit 1
}

cat <<EOF
class Rexboard < Formula
  desc "Attention board for coding-agent panes across Rex hosts"
  homepage "https://github.com/vanducng/rexboard"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/rexboard-$tag-aarch64-apple-darwin.tar.gz"
      sha256 "$mac"
    end
  end

  on_linux do
    on_intel do
      url "$base/rexboard-$tag-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$linux"
    end
  end

  def install
    bin.install "rexboard"
  end

  test do
    assert_match "rexboard #{version}", shell_output("#{bin}/rexboard --version")
  end
end
EOF
