# Contributing

rexboard is small on purpose. Read [the decisions](docs/decisions/README.md) first, since most "why not just…" questions are answered there. If your change goes against one of them, propose a new decision record in the same PR.

## Setup

Rust stable (1.88 or newer); `rust-toolchain.toml` pins the channel and the lockfile is committed.

```bash
git clone https://github.com/vanducng/rexboard
cd rexboard
cargo build
cargo run -- --help
```

The tests run against `tests/fixtures/fake-rex`, so you do not need Rex to work on most of the code. To try the board against your own Rex, `cargo run` is enough; to install from source, `cargo install --locked --git https://github.com/vanducng/rexboard`.

## The three checks

CI runs these on every PR, and a change is not done until they pass:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## How to add…

**A key.** Add one entry to the right table in `src/tui/keys.rs` (`BOARD` or `FULL`): its key sequences, how `?` lists it and its footer word. A new action also needs its `Act` variant handled in `src/tui/app.rs`. The tests in `keys.rs` fail if a sequence is bound twice or a key does nothing. Use a plain key, or one of vim's ctrl chords, as [decision 6](docs/decisions/0006-vim-keymap-from-one-table.md) says. Update the Keys table in [docs/tui.md](docs/tui.md#keys).

**A CLI command or flag.** Add it to `src/cli.rs` (help text is interface) and dispatch it in `src/lib.rs`. It takes `--json`. A failure is an `Error` from `src/error.rs` with a code, a message that names the bad value, and `next_steps` a caller can run; exit 1 for a failure, 2 for bad usage. Test it in `tests/cli.rs` against the fake rex. Document it in [docs/cli.md](docs/cli.md) with a sample, its JSON output and its exit codes.

**An extension.** You do not need a PR: an extension lives in its own directory. `rexboard ext new NAME` starts one, and [Extensions](docs/extensions.md) has the contract. A change to the contract itself is a new contract version and a decision record.

**A doc page.** Put it in `docs/`, add it to the Docs table in `README.md`, and say each fact once: link to the page that owns it instead of repeating it. Write plain sentences, no em dashes.

## Conventions

- **CLI output is a contract.** New fields, codes and flags are fine. Renaming or removing a JSON field, an error code or a flag is a breaking change.
- **Comments:** none by default. Write one only when a reader would be surprised: a Rex quirk, a hidden constraint, a non-obvious invariant.
- **Generic names only** in code, tests, fixtures, docs, screenshots and commit messages: hosts like `studio` and `mini`, sessions like `api` and `web`. Never commit tailnet hostnames, real labels, home paths, transcripts or people's names. Scrub a recorded fixture before committing it ([decision 10](docs/decisions/0010-public-repo-hygiene.md)).
- **Tests earn their place:** test behaviour a change could break, not that a library works or a constant exists.
- **Never** edit `CHANGELOG.md` or the version by hand; release-please owns them.

## Pull requests

- **One concern per PR.** A fix, a feature or a docs pass, not all three.
- **The title is the squash commit's subject**, so write it as a [Conventional Commit](https://www.conventionalcommits.org): `fix(tui): …`, `feat(cli): …`, `docs: …`. The type decides the next version ([Deployment](docs/deployment.md#releases)).
- **UI changes show screens** at 80 and 140 columns in the PR body, from a generic fixture. Screens go in the PR, not the repo.
- **Say what you checked:** the three checks, and for anything that talks to Rex, how you tried it. Live checks that type into or close a pane use a throwaway `zz-rexboard-probe-*` pane, never a real one ([decision 7](docs/decisions/0007-acting-on-panes-is-guarded.md)).
- **Changing a decision** means a new record under `docs/decisions/` that supersedes the old one, in the same PR.

[Development](docs/development-guidelines.md) maps the code and the tests.
