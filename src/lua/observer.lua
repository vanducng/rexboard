local function emit(line)
  line.v = 1
  print(rex.json.encode(line))
end

local function ids(t)
  local out = {}
  for i, v in ipairs(t or {}) do out[i] = v end
  if #out > 0 then return out end
end

local block_events = {
  program_status_changed = function(ev)
    return { type = "status", record_id = ev.id, record = ev.record }
  end,
  program_status_removed = function(ev)
    return { type = "status_removed", ids = ids(ev.ids), reason = ev.reason }
  end,
  title_changed = function(ev)
    return { type = "title", title = ev.title }
  end,
  process_changed = function(ev)
    if ev.foreground then
      return { type = "process", name = ev.foreground.name, cwd = ev.foreground.cwd, pid = ev.foreground.pid }
    end
  end,
  child_exited = function(ev)
    return { type = "child_exited", exit_code = ev.exit_code }
  end,
}

rex.on("block_event", function(_, ev)
  local shape = block_events[ev.name]
  local line = shape and shape(ev)
  if line then
    line.session_id, line.block_id = ev.session_id, ev.block_id
    emit(line)
  end
end)

local unplaced = {}

local function terminal_blocks(session_id)
  local listed = rex.call("session.list_blocks", { session_id = session_id }) or {}
  local out = {}
  for _, b in ipairs(listed.blocks or {}) do
    if b.creator_name == term then out[#out + 1] = b end
  end
  return out
end

local function announce(session_id, b)
  emit({ type = "block_created", session_id = session_id, window_id = b.window_id,
    block = block(rex, session_id, b) })
end

-- Rex can send block_created before it places the block; session_view_changed follows the placement.
rex.on("block_created", function(_, ev)
  for _, b in ipairs(terminal_blocks(ev.session_id)) do
    if b.block_id == ev.block_id then
      if b.placement == "placed" then
        unplaced[ev.block_id] = nil
        announce(ev.session_id, b)
      else
        unplaced[ev.block_id] = ev.session_id
      end
    end
  end
end)

rex.on("session_view_changed", function(_, ev)
  local waiting = false
  for _, session_id in pairs(unplaced) do
    waiting = waiting or session_id == ev.session_id
  end
  if not waiting then return end
  for _, b in ipairs(terminal_blocks(ev.session_id)) do
    if unplaced[b.block_id] and b.placement == "placed" then
      unplaced[b.block_id] = nil
      announce(ev.session_id, b)
    end
  end
end)

rex.on("block_closed", function(_, ev)
  unplaced[ev.block_id] = nil
  emit({ type = "block_closed", session_id = ev.session_id, block_id = ev.block_id })
end)
rex.on("window_created", function(_, ev)
  emit({ type = "window_created", session_id = ev.session_id, window_id = ev.window_id, label = ev.label })
end)
rex.on("window_closed", function(_, ev)
  emit({ type = "window_closed", session_id = ev.session_id, window_id = ev.window_id })
end)
rex.on("session_label_changed", function(_, ev)
  emit({ type = "label", kind = "session", session_id = ev.session_id, id = ev.session_id, label = ev.label })
end)
rex.on("window_label_changed", function(_, ev)
  emit({ type = "label", kind = "window", session_id = ev.session_id, id = ev.window_id, label = ev.label })
end)
rex.on("active_window_changed", function(_, ev)
  emit({ type = "active_window", session_id = ev.session_id, window_id = ev.window_id })
end)
rex.on("session_created", function(_, ev)
  if ev.session then emit({ type = "session_created", session = session_entry(rex, ev.session, unplaced) }) end
end)
rex.on("session_destroyed", function(_, ev)
  for id, session_id in pairs(unplaced) do
    if session_id == ev.session_id then unplaced[id] = nil end
  end
  emit({ type = "session_destroyed", session_id = ev.session_id })
end)
rex.every(30, function() emit({ type = "tick" }) end)

local snap = snapshot(rex, unplaced)
local version = rex.call("server.version")
snap.type, snap.rex_version = "snapshot", version and version.rex_version
emit(snap)
