-- Runs src/lua/send.lua against a stand-in for Rex's Lua API, for one pane in front.
-- Usage: lua send-harness.lua <src/lua dir> <front> <title seen>
-- <front> is "tmux", "wrapper" (OpenRig's /bin/sh -c wrapper around an attached tmux client, pid
-- 310) or "shell". Prints "sent" when the pane got its input, else "refused <code> <detail>".
local dir, front, seen = arg[1], arg[2], arg[3]

local foreground = ({
  tmux = { pid = 311, name = "tmux", argv0 = "tmux" },
  wrapper = { pid = 310, name = "bash", argv0 = "/bin/sh" },
  shell = { pid = 320, name = "zsh", argv0 = "-zsh" },
})[front]

local sent = {}
rex = {
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
