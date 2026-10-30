-- Runs src/lua/lib.lua and observer.lua against a stand-in for Rex's Lua API, then replays events.
-- Usage: lua observer-harness.lua <src/lua dir>. Each line printed is "<type> <block id>".
local dir = assert(arg[1], "usage: observer-harness.lua <src/lua dir>")

local handlers = {}
local blocks = { { block_id = "boot", creator_name = "com.superlogical.terminal", placement = "detached" } }
rex = {
  json = {
    encode = function(line)
      local block = line.block and line.block.block_id or line.block_id or ""
      return line.type .. " " .. block
    end,
  },
  block = { call = function() return {} end },
  on = function(name, fn) handlers[name] = fn end,
  every = function() end,
  call = function(method)
    if method == "session.list_blocks" then return { blocks = blocks } end
    if method == "session.list" then return { sessions = { { session_id = "s" } } } end
    if method == "session.view" then return { windows = { { window_id = "w" } } } end
    return {}
  end,
}

local function load_all(...)
  local source = {}
  for _, name in ipairs({ ... }) do
    local file = assert(io.open(dir .. "/" .. name))
    source[#source + 1] = file:read("*a")
    file:close()
  end
  assert(load(table.concat(source, "\n"), "observer"))()
end
load_all("lib.lua", "observer.lua")

local function block(id, placement, window)
  return { block_id = id, creator_name = "com.superlogical.terminal", placement = placement, window_id = window or "w" }
end
local function fire(name, ev)
  ev.session_id = "s"
  if handlers[name] then handlers[name](nil, ev) end
end

blocks = { block("late", "detached") }
fire("block_created", { block_id = "late" })
fire("session_view_changed", {})
blocks = { block("late", "placed") }
fire("session_view_changed", {})
fire("session_view_changed", {})

blocks = { block("late", "placed"), block("now", "placed") }
fire("block_created", { block_id = "now" })

blocks = { block("late", "placed"), block("now", "placed"), block("gone", "detached") }
fire("block_created", { block_id = "gone" })
fire("block_closed", { block_id = "gone" })
blocks = { block("late", "placed"), block("now", "placed") }
fire("session_view_changed", {})

blocks = { block("dup", "detached") }
fire("session_created", { session = { session_id = "s" } })
blocks = { block("boot", "detached"), block("dup", "placed") }
fire("block_created", { block_id = "dup" })
fire("session_view_changed", {})

blocks = { block("boot", "placed"), block("dup", "placed") }
fire("session_view_changed", {})
fire("session_view_changed", {})

-- A block that moves to another window is moved, and before its old window closes, or the board
-- would drop it with that window. One that is detached leaves, and is announced once when placed.
local rest = { block("boot", "placed"), block("dup", "placed") }
local function with(extra)
  local out = {}
  for i, b in ipairs(rest) do out[i] = b end
  out[#out + 1] = extra
  return out
end
blocks = with(block("mv", "placed"))
fire("block_created", { block_id = "mv" })
blocks = with(block("mv", "placed", "w2"))
fire("session_view_changed", {})
fire("session_view_changed", {})
blocks = with(block("mv", "placed", "w3"))
fire("window_closed", { window_id = "w2" })
blocks = with(block("mv", "detached"))
fire("session_view_changed", {})
fire("session_view_changed", {})
blocks = with(block("mv", "placed", "w1"))
fire("session_view_changed", {})
fire("session_view_changed", {})
