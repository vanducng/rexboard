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

`cargo test` also keeps the repo on the direction its docs set, offline and in well under a second:

- every JSON sample in README, CONTRIBUTING and `docs/` parses;
- `docs/tui.md` names every key the board and the full view bind, and its Keys table lists no key the board does not (`src/tui/keys.rs`);
- `docs/cli.md` names every command and flag in `rexboard --help`;
- `docs/`, `tests/`, `examples/`, README and CONTRIBUTING name no home directory, tailnet host or private key ([decision 10](docs/decisions/0010-public-repo-hygiene.md));
- `docs/decisions/README.md` links every record, and each has a `**Status:**` line.

The last four are in `tests/direction.rs`.

## How to add…

**A key.** Add one entry to the right table in `src/tui/keys.rs` (`BOARD` or `FULL`): its key sequences, how `?` lists it and its footer word. A new action also needs its `Act` variant handled in `App::act` (`src/tui/app/mod.rs`). `every_key_is_bound_once_and_no_chord_is_off_the_list` and `every_key_in_the_table_does_something` fail if a sequence is bound twice or a key does nothing, and `the_tui_doc_names_every_bound_key_and_no_other` if the docs miss it. Use a plain key, or one of vim's ctrl chords, as [decision 6](docs/decisions/0006-vim-keymap-from-one-table.md) says. Update the Keys table in [docs/tui.md](docs/tui.md#keys).

**A CLI command or flag.** Add it to `src/cli.rs` (help text is interface) and dispatch it in `src/lib.rs`. It takes `--json`. A failure is an `Error` from `src/error.rs` with a code, a message that names the bad value, and `next_steps` a caller can run; exit 1 for a failure, 2 for bad usage (or every host unreachable). Test it in `tests/cli.rs` against the fake rex. Document it in [docs/cli.md](docs/cli.md) with a sample, its JSON output and its exit codes.

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
- **Try harness writes in a sandbox.** Before running `hooks install` or `rexboard hook` by hand, point `HOME` at a temp dir and unset or redirect `CODEX_HOME`, `CLAUDE_CONFIG_DIR` and `PI_CODING_AGENT_DIR` as well, in every command: each one wins over `HOME`. Run `--dry-run` first and check the paths it prints are inside the sandbox ([decision 14](docs/decisions/0014-writes-to-user-config-are-explicit-reversible-and-fail-open.md)).
- **Remove a hook's old target last.** A change that retires something an installed hook runs lands only after the replacement is installed and `rexboard hooks status` says so.
- **Changing a decision** means a new record under `docs/decisions/` that supersedes the old one, in the same PR.

[Development](docs/development-guidelines.md) maps the code and the tests.
