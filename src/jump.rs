use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::board::{Group, Row};
use crate::client::{self, Rows};
use crate::config;
use crate::error::{Error, Result};
use crate::output::{labels, path};
use crate::rex;

const SCRIPT: &str = concat!(
    include_str!("lua/app.lua"),
    "\n",
    include_str!("lua/jump.lua")
);

/// `rexboard jump`: TARGET is a key (`HOST/BLOCK_ID`), N, the nth NEEDS YOU row, or `-`.
pub fn jump(target: &str, json: bool) -> Result<()> {
    let row = if target == "-" {
        previous()?
    } else {
        let rows: Rows =
            serde_json::from_value(client::call(&json!({ "cmd": "rows" }))?).map_err(|err| {
                Error::serve(format!("serve sent rows in an unexpected shape: {err}"))
            })?;
        find(rows.rows, target)?
    };
    go(&row)?;
    done(&row, json);
    Ok(())
}

/// `rexboard jump --shown N`: the row the resident board shows as N. With `partial`, a number
/// that a longer one could extend (N times ten is shown) exits 3 and goes nowhere.
pub fn shown(n: usize, partial: bool, json: bool) -> Result<()> {
    let reply = client::ask_running(&json!({ "cmd": "shown", "n": n }))
        .map_err(|err| match err {
            // This ask never starts or replaces a serve, so one from before `shown` answers it as a bad request.
            Error::Usage { .. } => Error::serve_with(
                "serve is older than this rexboard and has no board numbers · rexboard board restarts it",
                &["rexboard board"],
            ),
            err => err,
        })?
        .ok_or(Error::NoBoardNumbers)?;
    let count = reply["count"].as_u64().unwrap_or_default() as usize;
    if partial && n * 10 <= count {
        if json {
            println!("{}", json!({ "more": true }));
        }
        return Err(Error::More);
    }
    let row: Row = serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))?;
    go(&row)?;
    done(&row, json);
    Ok(())
}

/// `rexboard next`: the NEEDS YOU row after this pane (`REX_BLOCK`), unseen rows first; with
/// `marked` (a source, or `any`), the row that source marked after it instead.
pub fn next(back: bool, marked: Option<&str>, json: bool) -> Result<()> {
    let after = std::env::var("REX_BLOCK").ok().filter(|id| !id.is_empty());
    let reply = client::call(&json!({
        "cmd": "next", "after": after, "back": back,
        "marked": marked.is_some(), "source": marked,
    }))?;
    let row: Row = serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))?;
    go(&row)?;
    done(&row, json);
    Ok(())
}

/// The pane the jump before the last went to. Going there makes it the last, so the next `-`
/// comes back, as `cd -` does.
pub fn previous() -> Result<Row> {
    let reply = client::call(&json!({ "cmd": "back" }))?;
    serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))
}

fn find(rows: Vec<Row>, target: &str) -> Result<Row> {
    if let Ok(n) = target.parse::<usize>() {
        let needs: Vec<Row> = rows
            .into_iter()
            .filter(|row| row.group == Group::NeedsYou)
            .collect();
        if needs.is_empty() {
            return Err(Error::NobodyNeedsYou);
        }
        let count = needs.len();
        return n
            .checked_sub(1)
            .and_then(|i| needs.into_iter().nth(i))
            .ok_or_else(|| Error::NotFound(format!("NEEDS YOU has {count} rows, not a row {n}")));
    }
    if !target.contains('/') {
        return Err(Error::usage_with(
            format!("TARGET is HOST/BLOCK_ID or a NEEDS YOU row number: {target}"),
            &["rexboard ls --json"],
        ));
    }
    rows.into_iter()
        .find(|row| row.key == target)
        .ok_or_else(|| Error::NotFound(format!("no pane {target}")))
}

/// How long the app has to answer one `session.select`; jump.lua's own limit, under `rex do`'s 10s.
const EACH: Duration = Duration::from_secs(5);

/// How long a jump to a pane on another host keeps asking a slow app, from its start.
const PATIENCE: Duration = Duration::from_secs(30);

/// The least time between two asks, so an app that refuses at once is not asked in a tight loop.
const PACE: Duration = Duration::from_millis(250);

/// What a jump tells its caller, and what the caller can tell it.
pub struct Hooks<'a> {
    /// Called once, with the app's name, when the app has not answered and the jump asks again.
    pub waiting: &'a dyn Fn(&str),
    /// Whether to give up waiting (`Error::Cancelled`).
    pub cancelled: &'a dyn Fn() -> bool,
}

/// `jump`, `next` and `pick` from a command line: a terminal is told once when the jump waits.
pub fn go(row: &Row) -> Result<()> {
    go_with(
        row,
        &Hooks {
            waiting: &|app| {
                if std::io::stderr().is_terminal() {
                    eprintln!("waiting for the Rex app on {app}…");
                }
            },
            cancelled: &|| false,
        },
    )
}

/// Lands the Rex app on the row's pane and marks it seen.
pub fn go_with(row: &Row, hooks: &Hooks) -> Result<()> {
    let config = config::load()?;
    let app_client = app_client(config.client);
    let p = &row.pane;
    let remote = p.host != "local";
    let patience = std::env::var("REXBOARD_TEST_JUMP_WAIT_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map_or(PATIENCE, Duration::from_millis);
    let reply = land(
        &mut |select| {
            rex::run_script_cancellable(
                None,
                SCRIPT,
                &json!({
                    "host": p.host,
                    "session_id": p.session_id,
                    "window_id": p.window_id,
                    "block_id": p.block_id,
                    "app_client": app_client,
                    "select_ms": select.as_millis(),
                }),
                hooks.cancelled,
            )
        },
        remote.then_some(patience),
        PACE,
        hooks,
    )?;
    let waited = remote.then_some((p.host.as_str(), patience));
    check(
        &reply,
        "jump",
        &format!("rexboard jump {}", row.key),
        waited,
    )?;
    client::call(&json!({ "cmd": "seen", "key": row.key, "jump": true }))?;
    Ok(())
}

/// Runs `attempt` (a whole jump, given how long the app may take to answer) until it is not a
/// timeout of the app's `session.select`, or `patience` has passed since the first call. With no
/// `patience`, as on a local pane, the first reply is the answer. Selecting twice lands once, so
/// asking again is safe. The first attempt is the same as without any waiting.
fn land(
    attempt: &mut dyn FnMut(Duration) -> Result<Value>,
    patience: Option<Duration>,
    pace: Duration,
    hooks: &Hooks,
) -> Result<Value> {
    let began = Instant::now();
    let mut told = false;
    loop {
        let left = patience.map_or(EACH, |total| total.saturating_sub(began.elapsed()));
        let asked = Instant::now();
        let reply = attempt(EACH.min(left))?;
        let timed_out = select_timed_out(&reply);
        let more = patience.is_some_and(|total| began.elapsed() + pace < total);
        if !(timed_out && more) {
            return Ok(reply);
        }
        if !told {
            told = true;
            (hooks.waiting)(&app_name(reply["error"].as_str().unwrap_or("")));
        }
        while asked.elapsed() < pace {
            if (hooks.cancelled)() {
                return Err(Error::Cancelled);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Whether the reply says the app did not answer `session.select` in time.
fn select_timed_out(reply: &Value) -> bool {
    let error = reply["error"].as_str().unwrap_or("");
    reply["ok"] != true
        && reply["step"] == "session.select"
        && (error.contains("code=timeout") || error.contains("status=504"))
}

/// Set by the board when `esc` gives up a jump that is waiting.
static GIVE_UP: AtomicBool = AtomicBool::new(false);

/// Stops the jump that is waiting for the app.
pub fn give_up() {
    GIVE_UP.store(true, Ordering::SeqCst);
}

/// A jump the board starts can be given up from now on, and until the next one starts.
pub fn given_up() -> bool {
    GIVE_UP.load(Ordering::SeqCst)
}

pub fn rearm() {
    GIVE_UP.store(false, Ordering::SeqCst);
}

/// The Rex app client to ask: `REX_CLIENT`, else `client` from config.toml.
fn app_client(configured: Option<String>) -> Option<String> {
    std::env::var("REX_CLIENT")
        .ok()
        .filter(|label| !label.is_empty())
        .or(configured)
}

/// Turns a Lua reply into an error the reader can act on. `what` names the action in Rex's own
/// failures, and `retry` is the command to run again once the app answers. `waited` is the host
/// a jump kept asking through, and for how long.
fn check(reply: &Value, what: &str, retry: &str, waited: Option<(&str, Duration)>) -> Result<()> {
    if reply["ok"] == true {
        return Ok(());
    }
    let labels: Vec<String> = reply["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|label| label.as_str().map(str::to_owned))
        .collect();
    let text = |field: &str| {
        reply[field]
            .as_str()
            .unwrap_or("no reason given")
            .to_owned()
    };
    let pick_one = || {
        labels
            .iter()
            .map(|label| format!("client = \"{label}\" in config.toml, or REX_CLIENT={label}"))
            .collect()
    };
    let (step, error) = (text("step"), text("error"));
    Err(match reply["code"].as_str() {
        Some("ambiguous_client") => Error::AmbiguousClient {
            message: format!(
                "more than one Rex app is on this host's unix socket: {}",
                labels.join(", ")
            ),
            steps: pick_one(),
        },
        Some("no_unix_app") => Error::AmbiguousClient {
            message: format!(
                "no Rex app is on this host's unix socket; app clients: {}",
                labels.join(", ")
            ),
            steps: pick_one(),
        },
        Some("no_app") => Error::AmbiguousClient {
            message: "no Rex app is connected to this host's Rex server".to_owned(),
            steps: vec!["open the Rex app".to_owned(), "rex client ls".to_owned()],
        },
        Some("no_client") => Error::Config(format!(
            "{}; the app clients are: {}",
            text("error"),
            labels.join(", ")
        )),
        _ if select_timed_out(reply) => {
            let app = app_name(&error);
            let message = match waited {
                Some((host, total)) => format!(
                    "the Rex app on {app} did not answer through {host} within {}s; it may still \
                     switch to the pane. A locked screen or a stalled connection to that host can \
                     cause this.",
                    total.as_secs()
                ),
                None => format!(
                    "the Rex app on {app} did not answer within {}s; it may still switch to the \
                     pane",
                    EACH.as_secs()
                ),
            };
            Error::RexExplained {
                message,
                steps: vec![retry.to_owned(), "rex client ls".to_owned()],
                detail: error,
            }
        }
        _ => Error::Rex(format!("{what} failed at {step}: {error}")),
    })
}

/// The app's name from Rex's timeout text, as in `"studio" did not answer`.
fn app_name(error: &str) -> String {
    error
        .split_once(" did not answer")
        .and_then(|(head, _)| head.rsplit('"').nth(1))
        .map_or_else(|| "this Mac".to_owned(), crate::output::clean)
}

/// How a jump names its row: `session › tab › pane on host`.
fn name(row: &Row) -> String {
    format!(
        "{} on {}",
        path(&labels(row, false), usize::MAX),
        row.pane.host
    )
}

pub fn done(row: &Row, json: bool) {
    if json {
        println!("{}", json!({ "ok": true, "key": row.key }));
    } else {
        println!("jumped to {}", name(row));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const TIMEOUT: &str = r#"{"ok":false,"step":"session.select","error":"stream response \"control-2\" failed: \"studio\" did not answer within 5s (status=504 code=timeout)"}"#;
    const NO_APP: &str = r#"{"ok":false,"code":"no_app"}"#;
    const LANDED: &str = r#"{"ok":true}"#;
    const PACE: Duration = Duration::from_millis(5);

    /// `land` over these replies, one per ask, the last one repeating: the reply it ends on, how
    /// many asks it made, and what it said while waiting.
    fn run(
        replies: &[&str],
        patience: Option<Duration>,
        cancel_after: Option<usize>,
    ) -> (Result<Value>, usize, Vec<String>) {
        let asks = Cell::new(0);
        let said = RefCell::new(Vec::new());
        let reply = land(
            &mut |_| {
                let n = asks.get();
                asks.set(n + 1);
                let text = replies[n.min(replies.len() - 1)];
                Ok(serde_json::from_str(text).unwrap())
            },
            patience,
            PACE,
            &Hooks {
                waiting: &|app| said.borrow_mut().push(app.to_owned()),
                cancelled: &|| cancel_after.is_some_and(|n| asks.get() >= n),
            },
        );
        (reply, asks.get(), said.into_inner())
    }

    const SOON: Option<Duration> = Some(Duration::from_millis(200));

    #[test]
    fn a_quick_answer_is_one_ask_and_no_word_about_waiting() {
        let (reply, asks, said) = run(&[LANDED], SOON, None);
        assert_eq!(
            (reply.unwrap()["ok"] == true, asks, said.len()),
            (true, 1, 0)
        );
    }

    #[test]
    fn a_timeout_is_asked_again_until_the_app_answers_and_waiting_is_said_once() {
        let (reply, asks, said) = run(&[TIMEOUT, TIMEOUT, LANDED], SOON, None);
        assert_eq!(reply.unwrap()["ok"], true);
        assert_eq!(asks, 3);
        assert_eq!(said, ["studio"]);
    }

    #[test]
    fn it_stops_asking_when_the_patience_has_passed_and_returns_the_timeout() {
        let (reply, asks, said) = run(&[TIMEOUT], SOON, None);
        let reply = reply.unwrap();
        assert!(select_timed_out(&reply), "{reply}");
        assert!((3..=40).contains(&asks), "{asks}");
        assert_eq!(said, ["studio"]);
    }

    #[test]
    fn a_failure_that_is_not_a_timeout_is_not_asked_again() {
        let (reply, asks, said) = run(&[NO_APP, LANDED], SOON, None);
        assert_eq!(
            (reply.unwrap()["code"].clone(), asks, said.len()),
            ("no_app".into(), 1, 0)
        );
    }

    #[test]
    fn a_local_pane_has_no_patience_and_its_timeout_is_the_answer() {
        let (reply, asks, said) = run(&[TIMEOUT, LANDED], None, None);
        assert!(select_timed_out(&reply.unwrap()));
        assert_eq!((asks, said.len()), (1, 0));
    }

    #[test]
    fn giving_up_while_it_waits_is_cancelled() {
        let (reply, asks, _) = run(&[TIMEOUT], Some(Duration::from_secs(60)), Some(2));
        assert!(matches!(reply, Err(Error::Cancelled)));
        assert_eq!(asks, 2);
    }

    #[test]
    fn the_error_names_the_app_and_host_and_does_not_guess() {
        let reply: Value = serde_json::from_str(TIMEOUT).unwrap();
        let err = check(
            &reply,
            "jump",
            "rexboard jump mini/b",
            Some(("mini", PATIENCE)),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "the Rex app on studio did not answer through mini within 30s; it may still switch \
             to the pane. A locked screen or a stalled connection to that host can cause this."
        );
        let local = check(&reply, "jump", "rexboard jump local/b", None).unwrap_err();
        assert_eq!(
            local.to_string(),
            "the Rex app on studio did not answer within 5s; it may still switch to the pane"
        );
    }
}
