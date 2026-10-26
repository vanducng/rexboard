# 9. Release and install

**Status:** Accepted, 2026-10-25

## Context

rexboard is one binary for macOS on Apple silicon and Linux x86_64. Releases should cost no manual steps, and an install should be checkable and easy to upgrade.

## Decision

- Conventional Commits drive versions through release-please. `fix` is a patch; `feat` and breaking changes are a minor below 1.0. Nobody edits `CHANGELOG.md` or the version by hand.
- Merging the release PR tags the release, and the same workflow run builds both targets, checks `--version` against the tag, and uploads the archives with `SHA256SUMS`.
- Homebrew (`vanducng/tap`) is the main install; its formula is rendered from `SHA256SUMS`. `install.sh` stays for machines without Homebrew and for older releases, and refuses a checksum mismatch.
- `rexboard upgrade` upgrades the way rexboard was installed and never runs `sudo`. serve restarts itself on the next command ([2](0002-one-serve-daemon-many-clients.md)).

## Consequences

- A merged `feat` or `fix` reaches users without anyone building by hand.
- Commit types matter: a wrong type cuts the wrong version.
- Details are in [Deployment](../deployment.md).

## Supersedes

None.
