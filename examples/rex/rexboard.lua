-- The rexboard popup on a key, from any pane: copy this file next to Rex's init.lua and add
-- `require("rexboard")` there (see examples/README.md), then `rex config reload`.
--
-- ctrl+x then f opens `rexboard board --once` in a floating layer over the window you are in. The layer
-- closes when the board exits (a jump, `q`, `esc`); an error it printed waits for enter. Over a zoomed
-- pane the popup is the same size, and the pane is zoomed again when you close it without jumping.
--
-- The layer runs on the host that owns the pane. On a pane of another Rex host the popup shows that
-- host's rexboard, with its own config, favorites and board.json. For one board everywhere, run
-- rexboard on that host too, or wrap the command (an ssh to the host you keep the board on): your setup.
--
-- To pick another key, change KEY. Spelled `ctrl+[KeyX]>[KeyF]` it is the physical X then F key, whatever
-- the layout. If rexboard is not on the PATH of Rex's server, give BIN its full path.

local KEY = "ctrl+[KeyX]>[KeyF]"
local BIN = "rexboard"
local BOUNDS = { 0.08, 0.08, 0.84, 0.84 } -- x, y, width, height: a share of the window

-- Each variant is the flags `rexboard board --once` opens with (docs/tui.md, Popup). A flag wins for
-- that run and is not saved. Add one here, then bind it below with { variant = "name" }.
local VARIANTS = {
  all = {},
  marked = { "--status", "marked" }, -- favorites (*) and rows a [[mark]] rule marks
  sessions = { "--view", "session" },
  api = { "--keep", "session=api" }, -- named groups, as + and f+ keep them
}

-- Runs the board; shows what it printed on stderr until enter; re-zooms $2 unless the board jumped.
-- $0 is a name, $1 the session, $2 the block to zoom again (empty when none), the rest the command.
local RUN = [==[
sid=$1 zoomed=$2
shift 2
err=$(mktemp)
"$@" 2>"$err"
code=$?
if [ -s "$err" ]; then
  cat "$err"
  printf '\npress enter to close '
  read -r _
fi
rm -f "$err"
if [ "$code" != 0 ] && [ -n "$zoomed" ]; then
  rex api call -s "$sid" session.zoom_block "{\"block_id\":\"$zoomed\",\"zoom\":true}" >/dev/null 2>&1
fi
]==]

local function window_of(view, block_id)
  for _, window in ipairs(view.windows or {}) do
    for _, layer in ipairs(window.layers or {}) do
      for _, block in ipairs(layer.blocks or {}) do
        if block.block_id == block_id then
          return window
        end
      end
    end
  end
end

rex.action{
  name = "rexboard",
  title = "rexboard popup",
  args = { variant = "string?" },
  run = function(ctx, args)
    local flags = VARIANTS[args.variant or "all"]
    if flags == nil then
      error("rexboard: no variant named " .. tostring(args.variant), 0)
    end
    local view, err = rex.session.view{}
    if view == nil then
      error(err, 0)
    end
    local window = window_of(view, ctx.block_id)
    if window == nil then
      error("rexboard: the pane is in no window", 0)
    end
    local zoomed = window.zoomed_block_id
    if type(zoomed) ~= "string" then
      zoomed = ""
    end
    local command = { "sh", "-c", RUN, "rexboard", view.session_id, zoomed, BIN, "board", "--once" }
    for _, flag in ipairs(flags) do
      command[#command + 1] = flag
    end
    local _, failed = rex.session.new_layer{
      window_id = window.window_id,
      bounds = BOUNDS,
      focus = true,
      layout = rex.layout.block{
        flavor = "com.superlogical.terminal.shell",
        options = { command = command, shell = "none" },
      },
    }
    if failed ~= nil then
      error(failed, 0)
    end
  end,
}

rex.bind(KEY, "rexboard")
-- rex.bind("ctrl+[KeyX]>[KeyM]", "rexboard", { variant = "marked" })
-- rex.bind("ctrl+[KeyX]>[KeyV]", "rexboard", { variant = "sessions" })
-- rex.bind("ctrl+[KeyX]>[KeyA]", "rexboard", { variant = "api" })
