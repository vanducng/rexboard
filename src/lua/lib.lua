local term = "com.superlogical.terminal"

local function list(t)
  local out = {}
  for i, v in ipairs(t or {}) do out[i] = v end
  return out
end

local function block(api, session_id, b)
  local args = { session_id = session_id, block_id = b.block_id }
  local status = api.block.call(term, "program_status", args)
  local title = api.block.call(term, "title", args)
  local process = api.block.call(term, "process", args)
  local fg = process and process.foreground or {}
  return {
    block_id = b.block_id,
    label = b.label,
    records = list(status and status.records),
    title = title and title.title,
    process = { name = fg.name, cwd = fg.cwd, pid = fg.pid },
  }
end

local function session_entry(api, s)
  local view = api.call("session.view", { session_id = s.session_id }) or {}
  local listed = api.call("session.list_blocks", { session_id = s.session_id }) or {}
  local windows, by_id = {}, {}
  for _, w in ipairs(view.windows or {}) do
    local win = { window_id = w.window_id, label = w.label, blocks = {} }
    windows[#windows + 1] = win
    by_id[w.window_id] = win
  end
  for _, b in ipairs(listed.blocks or {}) do
    local win = by_id[b.window_id]
    if win and b.creator_name == term and b.placement == "placed" then
      win.blocks[#win.blocks + 1] = block(api, s.session_id, b)
    end
  end
  return { session_id = s.session_id, label = s.label, windows = windows }
end

local function snapshot(api)
  local _, err = api.call("session.attach_all", { event_scope = "session" })
  if err then error(err, 0) end
  local sessions = {}
  for _, s in ipairs(api.call("session.list").sessions or {}) do
    sessions[#sessions + 1] = session_entry(api, s)
  end
  return { sessions = sessions }
end
