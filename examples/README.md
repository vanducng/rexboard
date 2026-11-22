# Examples

Copy one, change what you like. Nothing here runs by itself, and `cargo test` keeps each file working (`tests/lua.rs`, `tests/examples.rs`).

| Example | What it gives you | Use it |
| --- | --- | --- |
| [`rex/rexboard.lua`](rex/rexboard.lua) | The board as a popup over any Rex pane, on `ctrl+x` then `f`, and variants as data: marked rows only, one view, named groups | Copy it next to Rex's `init.lua` (`~/.config/rex/`, or beside the file `REX_CONFIG` names), add `require("rexboard")` to `init.lua`, check it with `rex config check`, then `rex config reload`. [Open it from Rex](../docs/tui.md#open-it-from-rex) says how the popup works, and what it shows on a pane of another host |
| [`rexboard/config.toml`](rexboard/config.toml) | Two `[[mark]]` rules: panes by a label glob, and by an extension's field on one host | Copy it to `~/.config/rexboard/config.toml` (or merge the rules into yours). Every key is in [Configuration](../docs/configuration.md#mark) |
| [`rexboard/tui.json`](rexboard/tui.json) | `"digits": "jump"`: `1`-`99` go to the row's pane, which suits a popup | Copy it to `~/.local/state/rexboard/tui.json`. JSON has no comments, so every setting is in [`tui.json`](../docs/tui.md#tuijson). The board saves its own choices into this file, such as the card on `c` |
| [`extensions/session-prefix`](extensions/session-prefix) | A whole [extension](../docs/extensions.md): a `project` field from the session label | Name it under `extensions.enabled` in `config.toml`. [Extensions](../docs/extensions.md) has the rest |

Favorites (`*`) and kept groups (`+`, `f+`) are made on the board, not in a file: [Favorites](../docs/tui.md#favorites) and [Kept groups](../docs/tui.md#kept-groups). The popup opens on them as the board does, and `rex/rexboard.lua` shows how to open it on one.
