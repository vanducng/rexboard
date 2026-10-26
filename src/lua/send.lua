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
if not main or main.updated_at ~= a.expect.updated_at then return refuse("status_changed") end

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
  return false
end
local front = agent(fg.name) or agent(fg.argv0)
-- A tmux seat's pane shows tmux; its title says which seat is on screen.
if not front and fg.name == "tmux" and a.expect.title then
  local title = rex.block.call(term, "title", where)
  front = title ~= nil and title.title == a.expect.title
end
if not front then return refuse("not_harness", fg.name or "nothing") end

local t = tail(a)
if not t.ok then return t end
local lines = {}
for line in (t.text .. "\n"):gmatch("(.-)\n") do lines[#lines + 1] = line end
while #lines > 0 and lines[#lines]:match("^%s*$") do lines[#lines] = nil end
-- The fingerprint starts and ends with a newline, so it matches whole lines only.
local window = "\n" .. table.concat(lines, "\n", math.max(1, #lines - 39)) .. "\n"
if not window:find(a.expect.fingerprint, 1, true) then return refuse("prompt_changed") end

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
for _, event in ipairs(events) do
  local _, err = rex.call("block.event", { session_id = a.session_id, block_id = a.block_id, event = event })
  if err then return fail("block.event", err) end
end
return { ok = true }
