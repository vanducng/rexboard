# Deployment

`rexboard` ships as GitHub release archives. Nothing runs on a server.

## CI

`.github/workflows/ci.yml` runs on every pull request and on pushes to `main`:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Releases

Releases are automatic and driven by conventional commits.

1. Commits land on `main`. release-please (`.release-please-config.json`, `.release-please-manifest.json`) keeps a release PR open with the next version and its `CHANGELOG.md` entry.
2. `fix` bumps the patch. `feat` and breaking changes bump the minor while the version is below 1.0. `docs`, `refactor`, `chore`, `test`, and `ci` alone cut no release.
3. Merging the release PR updates `Cargo.toml` and `Cargo.lock`, tags `vX.Y.Z`, and creates the GitHub release.
4. The same `release` workflow run builds both targets with `cargo build --release --locked`, checks that `rexboard --version` matches the tag, packs `rexboard`, `README.md`, and `LICENSE` into `rexboard-vX.Y.Z-TARGET.tar.gz`, and uploads the archives with `SHA256SUMS`.
5. The `homebrew` job renders `Formula/rexboard.rb` from that release's `SHA256SUMS` with `scripts/homebrew-formula.sh` and commits it to [vanducng/homebrew-tap](https://github.com/vanducng/homebrew-tap) with the `HOMEBREW_TAP_GITHUB_TOKEN` secret. Without the secret it skips with a notice. It runs only after `publish` succeeds, or on its own with `homebrew_only` (see below). A formula that is already at the tag commits nothing.

Do not edit `CHANGELOG.md` or the version by hand. To force a version, push an empty commit to `main` with a `Release-As: X.Y.Z` footer.

The build and release-please share one workflow run because a tag created with `GITHUB_TOKEN` starts no new run.

## Rebuild assets

Run the `release` workflow by hand with an existing tag, like `v0.1.0`, to rebuild that tag's archives, upload them again and republish the Homebrew formula for that tag:

```bash
gh workflow run release --repo vanducng/rexboard -f tag=v0.1.0
```

That gives the archives new sha256 sums, so do not run it on a release people already installed. To publish only the Homebrew formula from a release's existing archives and `SHA256SUMS`, add `homebrew_only`. It skips release-please and the build, and leaves the release untouched:

```bash
gh workflow run release --repo vanducng/rexboard -f tag=v0.2.0 -f homebrew_only=true
```

## Install or upgrade

With Homebrew (macOS on Apple silicon, Linux x86_64):

```bash
brew install vanducng/tap/rexboard
brew upgrade rexboard
```

Without it, `install.sh` at the repo root is the installer. Switching from it to Homebrew, remove `~/.local/bin/rexboard` first, so the two do not shadow each other:

```bash
curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
```

It maps the machine to a release target, downloads that archive and `SHA256SUMS` from the release, refuses a checksum mismatch, and replaces the binary in one `mv`. It does nothing when that version is already installed. `REXBOARD_INSTALL_DIR` changes the directory (default `~/.local/bin`). `REXBOARD_VERSION=vX.Y.Z` installs a given release.

## Roll back

Install the previous release with `curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | REXBOARD_VERSION=v0.1.0 sh`, naming its tag. To pull a bad release, mark it as a pre-release or delete it in GitHub, then ship the fix as a new `fix` commit so release-please cuts the next patch.
