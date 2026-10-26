//! Fixtures the app's tests and the board's other tests share: rows, a board, and keys
//! pressed at a person's pace.

use super::*;

/// seat-b blocked and two working rows, with the status filter on working and the full
/// view open on pi, which then finishes and leaves the filter.
pub fn viewing_pi_as_it_leaves_the_filter() -> App {
    let mut app = App::default();
    let mut rows = rows();
    rows.push(row("mini/pi", "working", "working", true));
    app.set_rows(rows.clone(), hosts());
    press(&mut app, KeyCode::Char('f'));
    press(&mut app, KeyCode::Char('w'));
    app.selected = Some(Spot::Row("mini/pi".into()));
    press(&mut app, KeyCode::Char(' '));
    rows.retain(|row| row.key != "mini/pi");
    rows.push(row("mini/pi", "done", "needs_you", false));
    app.set_rows(rows, hosts());
    assert_eq!(
        app.full.as_ref().map(|full| full.key.as_str()),
        Some("mini/pi")
    );
    assert_ne!(
        app.selected,
        Some(Spot::Row("mini/pi".into())),
        "the board moved on"
    );
    app
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Types `keys` as a person would, a little over GUARD apart.
pub fn typed(app: &mut App, keys: &str) -> Vec<Option<Effect>> {
    keys.chars().map(|c| press(app, KeyCode::Char(c))).collect()
}

/// Types `text` (a newline is enter) a key every `gap` ms, with one 400 ms pause before
/// the character at `pause`. Each read answer mode asks for comes back at once as `screen`.
pub fn typed_at(
    app: &mut App,
    text: &str,
    gap: u64,
    pause: Option<usize>,
    screen: &str,
) -> Vec<Effect> {
    let t0 = Instant::now();
    let mut ms = 0;
    let mut effects = Vec::new();
    for (i, c) in text.chars().enumerate() {
        ms += if pause == Some(i) { 400 } else { gap };
        let code = if c == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(c)
        };
        let effect = app.handle_at(KeyEvent::from(code), t0 + Duration::from_millis(ms));
        if let Some(Effect::Read { key, .. }) = &effect {
            app.screen(key, Ok(Screen::parse(screen, None)));
            drawn(app);
        }
        effects.extend(effect);
    }
    effects
}

/// What typing must never do: answer an agent or close a pane.
pub fn acts_on_a_pane(effect: &Effect) -> bool {
    matches!(effect, Effect::Send { .. } | Effect::Close(_))
}

/// What a key can change on the board, to tell whether it did anything.
pub fn state_of(app: &App) -> String {
    format!(
        "{:?} {:?} {} {:?} {} {} {} {} {:?} {} {} {} {:?} {:?}",
        app.selected,
        app.sort,
        app.view.name,
        app.folded,
        app.keys,
        app.filtering,
        app.commanding,
        app.hidden,
        app.pending,
        app.closing.is_some(),
        app.answer.is_some(),
        app.full.is_some(),
        app.status,
        app.toast(),
    )
}

pub fn row(key: &str, state: &str, group: &str, seen: bool) -> Row {
    let (host, block) = key.split_once('/').unwrap();
    serde_json::from_value(serde_json::json!({
        "key": key, "host": host, "session_id": "session:api", "session": "api",
        "window_id": "window:main", "tab": "main", "block_id": block, "pane": block,
        "title": format!("{block} title"), "harness": "claude-code", "state": state,
        "kind": null, "msg": format!("{block} msg"), "updated_at": "2026-01-01T00:00:00Z",
        "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
        "since_estimated": false, "turn_started": null, "last_turn": null,
        "seen": seen, "stale": false, "suspect": false, "suspect_reason": null,
        "group": group
    }))
    .unwrap()
}

pub fn hosts() -> BTreeMap<String, String> {
    BTreeMap::from([("local".into(), "up".into()), ("mini".into(), "up".into())])
}

pub fn rows() -> Vec<Row> {
    vec![
        row("mini/seat-b", "blocked", "needs_you", false),
        row("local/claude", "done", "needs_you", false),
        row("mini/codex", "working", "working", true),
        row("local/seat-a", "idle", "quiet", true),
    ]
}

pub fn app() -> App {
    let mut app = App::default();
    app.set_rows(rows(), hosts());
    app
}

/// A key from a person: each press comes a little over GUARD after the one before, so no
/// typing guard drops it. The guards' own tests give `handle_at` exact times.
pub fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
    key(app, KeyEvent::from(code))
}

/// `o` and a sort letter.
pub fn sort(app: &mut App, letter: char) -> Option<Effect> {
    press(app, KeyCode::Char('o'));
    press(app, KeyCode::Char(letter))
}

pub fn key(app: &mut App, key: KeyEvent) -> Option<Effect> {
    thread_local! {
        static CLOCK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }
    let now = CLOCK.with(|clock| {
        let now = clock
            .get()
            .map_or_else(Instant::now, |at| at + GUARD + Duration::from_millis(1));
        clock.set(Some(now));
        now
    });
    app.handle_at(key, now)
}

/// `:seen` and enter.
pub fn seen_all(app: &mut App) -> Option<Effect> {
    typed(app, ":seen");
    press(app, KeyCode::Enter)
}

pub fn selected(app: &App) -> String {
    match app.selected.clone().unwrap() {
        Spot::Row(key) => key,
        Spot::Group(group) => format!("{group:?}"),
    }
}

/// `a` on the selected row, with its pane read back as `text`.
pub fn answering(app: &mut App, text: &str) -> String {
    let Some(Effect::Read { key, .. }) = press(app, KeyCode::Char('a')) else {
        panic!("a reads the pane first");
    };
    app.screen(&key, Ok(Screen::parse(text, None)));
    drawn(app);
    key
}

/// The card drawn as the board draws it, which tells answer mode the options on screen.
pub fn drawn(app: &App) {
    if let Some(answer) = &app.answer {
        answer.lines(chrono::Utc::now(), 120, 40);
    }
}
