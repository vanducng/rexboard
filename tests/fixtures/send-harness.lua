-- Runs src/lua/send.lua against a stand-in for Rex's Lua API, for one pane in front.
-- Usage: lua send-harness.lua <src/lua dir> <front> <proof> <title seen>
-- <front> is "tmux", "wrapper" (a sh -c wrapper, pid 310) or "shell". <proof> is what rexboard
-- names for the wrapper, and what the host then shows: "-" names none; "attached" a tmux child
-- (311) the server lists; "unlisted" one the server does not list (a tmux wait-for); "left" one
-- that is listed until the pane is read again; "gone" one that is not the wrapper's child (its parent is another process).
-- Prints "sent" when the pane got its input, else "refused <code> <detail>".
local dir, front, proof, seen = arg[1], arg[2], arg[3], arg[4]

local foreground = ({
  tmux = { pid = 311, name = "tmux", argv0 = "tmux" },
  wrapper = { pid = 310, name = "bash", argv0 = "/bin/sh" },
  shell = { pid = 320, name = "zsh", argv0 = "-zsh" },
})[front]

local sent, listed = {}, 0
rex = {
  exec = function(argv)
    if argv[1] == "ps" then
      return { stdout = proof == "gone" and "  1 /usr/bin/tmux\n" or "  310 /usr/bin/tmux\n" }
    end
    listed = listed + 1
    local lists = proof == "attached" or proof == "gone" or (proof == "left" and listed == 1)
    return { stdout = lists and "311\n312\n" or "312\n" }
  end,
  args = {
    session_id = "session:api",
    block_id = "block:seat-a",
    harness = { "claude", "codex", "pi" },
    harness_prefixes = {},
    dialog = { cursor = { "❯" }, input = { "❯" }, separators = { "." } },
    action = { text = "go on" },
    expect = {
      state = "idle",
      fingerprint = "\n❯ \n",
      title = "seat-a",
      wrapper = proof ~= "-" and { pid = 310, client = 311, socket = "/tmp/probe.sock" } or nil,
    },
  },
  call = function(method)
    if method == "block.event" then sent[#sent + 1] = method end
    return {}
  end,
  block = {
    call = function(_, method)
      if method == "program_status" then return { records = { { id = "", state = "idle" } } } end
      if method == "process" then return { foreground = foreground } end
      if method == "title" then return { title = seen } end
    end,
  },
}
tail = function() return { ok = true, text = "\n❯ \n" } end

local file = assert(io.open(dir .. "/send.lua"))
local source = file:read("*a")
file:close()
local result = assert(load(source, "send"))()
if result.ok and #sent == 2 then
  print("sent")
else
  print("refused " .. tostring(result.code) .. " " .. tostring(result.detail))
end
