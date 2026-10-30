-- Answers one pane, after checking it still shows what the operator answered: the same status
-- report, an agent (not a shell) in front, and the same dialog or input line on screen. Runs after
-- tail.lua, so the re-read is the card's own. Sends one digit key with no Enter, or a paste then
-- Enter; never Esc or Ctrl+C. Returns { ok = true }, { ok = false, code, detail } for a refusal
-- that sent nothing, or { ok = false, step, error }.
local a = rex.args
local term = "com.superlogical.terminal"
local where = { session_id = a.session_id, block_id = a.block_id }

local function refuse(code, detail)
  return { ok = false, code = code, detail = detail }
end

local function fail(step, err)
  local text = tostring(err)
  if text:find("not_found", 1, true) then return refuse("gone", text) end
  return { ok = false, step = step, error = text }
end

local _, aerr = rex.call("session.attach", { session_id = a.session_id })
if aerr then return fail("session.attach", aerr) end

-- The pane's main record, picked as pane.rs derive does: the root, else the most urgent, never
-- the hook's `turn` record.
local status, serr = rex.block.call(term, "program_status", where)
if not status then return fail("program_status", serr) end
local rank = { blocked = 0, error = 1, done = 2, working = 3, idle = 4 }
local main
for _, r in ipairs(status.records or {}) do
  local id = r.id or ""
  if id == "" then
    main = r
    break
  end
  if id ~= "turn" and (not main or (rank[r.state] or 5) < (rank[main.state] or 5)) then main = r end
end
-- Same state and kind: a fresh report of the same wait (a hook's notification for the dialog
-- already up) is no change. The fingerprint below tells one dialog from another.
local function wait(state, kind)
  if not state then return "no status" end
  return kind and (state .. "/" .. kind) or state
end
local was, is = wait(a.expect.state, a.expect.kind), wait(main and main.state, main and main.kind)
if was ~= is then return refuse("status_changed", was .. " → " .. is) end

local process, perr = rex.block.call(term, "process", where)
if not process then return fail("process", perr) end
local fg = process.foreground or {}
local function agent(name)
  if not name then return false end
  name = name:match("[^/]+$") or name
  -- Claude Code's binary is named for its version.
  if name:match("^%d+%.%d+%.%d+$") then return true end
  for _, h in ipairs(a.harness or {}) do
    if h == name then return true end
  end
  for _, prefix in ipairs(a.harness_prefixes or {}) do
    if name:sub(1, #prefix) == prefix then return true end
  end
  return false
end
local front = agent(fg.name) or agent(fg.argv0)
-- A tmux seat's pane shows tmux; its title says which seat is on screen.
if not front and fg.name == "tmux" and a.expect.title then
  local title = rex.block.call(term, "title", where)
  front = title ~= nil and title.title == a.expect.title
end
-- OpenRig's `/bin/sh -c ... tmux attach` wrapper: input through Rex cannot be pinned to the
-- pane's own tmux client, so it is refused.
if not front and (fg.argv0 or ""):match("/sh$") then return refuse("wrapped", fg.name) end
if not front then return refuse("not_harness", fg.name or "nothing") end

local t = tail(a)
if not t.ok then return t end
local lines = {}
for line in (t.text .. "\n"):gmatch("(.-)\n") do lines[#lines + 1] = line end
while #lines > 0 and lines[#lines]:match("^%s*$") do lines[#lines] = nil end
local last = {}
for i = math.max(1, #lines - 39), #lines do last[#last + 1] = lines[i] end
-- The fingerprint starts and ends with a newline, so it matches whole lines only.
local window = "\n" .. table.concat(last, "\n") .. "\n"
if not window:find(a.expect.fingerprint, 1, true) then return refuse("prompt_changed") end

-- The marks come from the board's own grammar (a.dialog), so the two read a screen alike.
local function marked(rest, marks)
  for _, mark in ipairs(marks) do
    if rest:sub(1, #mark) == mark then return rest:sub(#mark + 1) end
  end
end

-- `1. Yes` or `❯ 2) No`: the option's digit.
local function option(line)
  local rest = line:gsub("^%s+", "")
  rest = (marked(rest, a.dialog.cursor) or rest):gsub("^%s+", "")
  local digit, after = rest:match("^([1-9])(.*)$")
  if not digit then return nil end
  after = marked(after, a.dialog.separators)
  if after and after:match("^%s+%S") then return digit end
end

local function input_line(line)
  if option(line) then return false end
  local rest = line:gsub("^%s+", "")
  rest = (marked(rest, { "│" }) or rest):gsub("^%s+", "")
  return marked(rest, a.dialog.input) ~= nil
end

-- A dialog is up when options 1 and 2 show with no input line below them. Text into a dialog
-- would be read as its highlighted option, whatever the board believes it shows.
local function dialog_up()
  local one, two = false, false
  for _, line in ipairs(last) do
    local digit = option(line)
    if digit == "1" then
      one, two = true, false
    elseif digit == "2" and one then
      two = true
    elseif input_line(line) then
      one, two = false, false
    end
  end
  return two
end
if a.action.text and dialog_up() then return refuse("dialog_up") end

local events
if a.action.pick then
  local digit = a.action.pick
  if not digit:match("^[1-9]$") then return refuse("bad_action", digit) end
  -- Without `text` the key types nothing.
  events = { { type = "key", payload = { key = "digit_" .. digit, action = "press", text = digit } } }
else
  local text = a.action.text or ""
  if text == "" or text:find("%c") then return refuse("bad_action", "text") end
  events = {
    { type = "paste", payload = { text = text } },
    { type = "key", payload = { key = "enter", action = "press" } },
  }
end
for i, event in ipairs(events) do
  local _, err = rex.call("block.event", { session_id = a.session_id, block_id = a.block_id, event = event })
  -- After a paste, a failed Enter leaves the text in the pane's input line.
  if err then return fail(i == 2 and "enter" or "block.event", err) end
end
return { ok = true }
