use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rexboard",
    version,
    about = "Unofficial attention board for agent panes across Rex hosts.",
    after_help = "Errors: {\"ok\":false,\"error\":{code,message,next_steps}} on stderr with --json. Exit 1: Rex or serve failed, the key was not found, or a check refused to answer or close. Exit 2: bad usage, or every host is unreachable."
)]
pub struct Cli {
    /// Print JSON on stdout, and errors as JSON on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List agent panes on this host and every `rex hosts` entry, most urgent first.
    Ls(LsArgs),
    /// Stream the agent rows (`--json`) or raw events (`--raw`) from this host and every `rex hosts` entry.
    Watch(WatchArgs),
    /// Run the daemon the board and the other commands share. They start it when it is not running.
    Serve(ServeArgs),
    /// Open the board: agent panes grouped by what they need. `rexboard` with no command opens it
    /// in a terminal.
    Board,
    /// Mark a pane seen by the `key` from `ls --json`, or every unseen done row with `--all-done`.
    Seen(SeenArgs),
    /// What the board's card says about a pane, by its `key`; `--tail N` adds its last lines.
    Show {
        /// HOST/BLOCK_ID, the key from ls --json.
        #[arg(value_name = "KEY")]
        key: String,
        /// Also read the pane's last N lines, as the card does (at most 200).
        #[arg(long, value_name = "N")]
        tail: Option<usize>,
    },
    /// Answer a pane's dialog (`--pick N`) or input line (`--text`), by its `key`, checked as the
    /// board's answer mode checks it.
    Answer(AnswerArgs),
    /// Close a pane by its `key`. On a terminal it asks first; `--yes` closes without asking.
    Close {
        /// HOST/BLOCK_ID, the key from ls --json.
        #[arg(value_name = "KEY")]
        key: String,
        /// Close without asking; needed when there is no terminal to ask on.
        #[arg(long)]
        yes: bool,
    },
    /// Keep what an agent's last turn used (model, context, tokens) for the board: run by the
    /// harness's status line or hook, with its JSON on stdin. Prints nothing itself.
    Usage {
        /// The harness that sends the JSON: claude, codex, pi or grok.
        #[arg(value_name = "HARNESS")]
        harness: String,
        /// Your own status line command, run with the same JSON; its output and exit code pass
        /// through.
        #[arg(last = true, value_name = "CMD")]
        then: Vec<String>,
    },
    /// List the views `ls --view` and the board's `v` take, and what each groups by.
    Views,
    /// List the hosts serve watches: each one's state, agent rows and last error.
    Hosts,
    /// Go to a pane in the Rex window you are in, on its host, and mark it seen.
    Jump {
        /// HOST/BLOCK_ID (the `key` from `ls --json`), N for the nth NEEDS YOU row, or `-` for the
        /// pane before the last jump.
        #[arg(value_name = "TARGET")]
        target: String,
    },
    /// Go to the next pane that needs you, after this one. Exit 1 when nobody needs you.
    Next {
        /// Go the other way.
        #[arg(long)]
        back: bool,
    },
    /// Pick a pane from the board's list and go to it. Enter goes and exits 0; esc exits 1.
    Pick,
    /// List extensions, run one once against real rows, or start a new one from the example.
    #[command(subcommand)]
    Ext(ExtCommand),
    /// Tell Rex an agent's status from one of its hooks, with the hook's JSON on stdin. The hooks
    /// `rexboard hooks install` writes run it; it never fails the hook.
    Hook {
        /// The harness whose hook runs it: claude, codex, pi or grok.
        #[arg(value_name = "HARNESS")]
        harness: String,
        /// The hook event, for JSON that names none.
        #[arg(value_name = "EVENT")]
        event: Option<String>,
    },
    /// Put `rexboard hook` and `rexboard usage` into each harness's own settings, or say whether
    /// they are there.
    #[command(subcommand)]
    Hooks(HooksCommand),
    /// Upgrade rexboard the way it was installed (Homebrew, or install.sh), then stop serve so the
    /// next command runs the new one.
    #[command(alias = "update")]
    Upgrade {
        /// Only say whether a newer release is out.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum HooksCommand {
    /// Merge rexboard's hooks into each harness this host has, or the ones named, after a
    /// backup beside each file. A second install changes nothing.
    Install {
        /// claude, codex, pi or grok; every one with settings on this host when none is named.
        #[arg(value_name = "HARNESS")]
        harness: Vec<String>,
        /// Say what would change, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Each harness: whether its settings have rexboard's hooks, and what an install would change.
    Status,
    /// Send each pane's last status again to a tmux client that just attached to SESSION; for
    /// tmux's client-attached hook.
    Replay {
        #[arg(value_name = "SESSION")]
        session: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ExtCommand {
    /// One line per extension found: its contract, whether it is enabled, how its last run in
    /// a running serve went, and its directory. A broken one says why.
    Ls,
    /// Run an extension once, enabled or not, and check its answer against the contract. Exit 1
    /// when the answer breaks it.
    Test {
        #[arg(value_name = "NAME")]
        name: String,
        /// Send the rows in FILE, as `rexboard ls --json` prints them, instead of serve's.
        #[arg(long, value_name = "FILE")]
        rows: Option<PathBuf>,
    },
    /// Start extension NAME from the example, in `$XDG_CONFIG_HOME/rexboard/extensions/NAME`.
    New {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Run in this terminal instead of in the background.
    #[arg(long, conflicts_with = "stop")]
    pub foreground: bool,
    /// Stop the running serve and its observers.
    #[arg(long)]
    pub stop: bool,
}

#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Print one JSON line per event, tagged with its `host`.
    #[arg(long)]
    pub raw: bool,
    /// Only this host: `local`, or a label from `rex hosts`.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Stop after this many seconds. Without it, watch runs until interrupted.
    #[arg(long = "for", value_name = "SECONDS")]
    pub duration: Option<u64>,
    /// Run this command's own observers instead of going through serve. `--raw` always does.
    #[arg(long)]
    pub direct: bool,
}

#[derive(Debug, Args)]
pub struct SeenArgs {
    /// HOST/BLOCK_ID, the key from ls --json.
    #[arg(value_name = "KEY", required_unless_present = "all_done")]
    pub key: Option<String>,
    /// Every unseen done row, or those --host and --filter keep.
    #[arg(long, conflicts_with = "key")]
    pub all_done: bool,
    /// With --all-done, only this host.
    #[arg(long, value_name = "HOST", conflicts_with = "key")]
    pub host: Option<String>,
    /// With --all-done, only rows this filter keeps, as with `ls --filter`.
    #[arg(long, value_name = "TEXT", conflicts_with = "key")]
    pub filter: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("reply").required(true).args(["pick", "text"])))]
pub struct AnswerArgs {
    /// HOST/BLOCK_ID, the key from ls --json.
    #[arg(value_name = "KEY")]
    pub key: String,
    /// Pick the dialog's option N: one key press, no Enter.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(1..=9))]
    pub pick: Option<u8>,
    /// Reply with a line: a paste, then Enter. Not into a dialog, or an input line holding text.
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,
    /// The `dialog` from `show KEY --json`: the answer goes only to the dialog or input line you
    /// read.
    #[arg(long, value_name = "ID")]
    pub dialog: String,
    /// Send only if the row is still in this state, or state and kind: `blocked`,
    /// `blocked:permission`.
    #[arg(long, value_name = "STATE[:KIND]")]
    pub expect: Option<String>,
    /// Send an option that grants something for later (`always`, `don't ask again`), which the
    /// board takes only on a second press.
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Include plain shells, not just panes with a status record or a known harness.
    #[arg(long)]
    pub all: bool,
    /// Only this host: `local`, or a label from `rex hosts`.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Take one snapshot here instead of asking serve.
    #[arg(long, conflicts_with_all = ["view", "filter", "status", "sort"])]
    pub direct: bool,
    /// Group and order the rows as this board view does (`rexboard views` lists them).
    #[arg(long, value_name = "NAME")]
    pub view: Option<String>,
    /// Keep only rows this text matches, as the board's `/` does: words match any label, and
    /// `key:value` one key (host, state, harness, session, window, block, a field). Repeat to
    /// narrow further.
    #[arg(long, value_name = "TEXT")]
    pub filter: Vec<String>,
    /// Keep only one status, by name or first letter: working, done, question, permission,
    /// error, unreported, idle.
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
    /// Sort within each group by a column (state, age, turn, harness, host, where, session,
    /// window, block, msg) or a field, with `:asc` or `:desc`.
    #[arg(long, value_name = "COLUMN[:asc|:desc]")]
    pub sort: Option<String>,
}

impl LsArgs {
    pub fn query(&self) -> crate::tui::query::Query<'_> {
        crate::tui::query::Query {
            view: self.view.as_deref(),
            filters: &self.filter,
            status: self.status.as_deref(),
            sort: self.sort.as_deref(),
        }
    }
}
