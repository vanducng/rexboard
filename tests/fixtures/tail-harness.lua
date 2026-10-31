-- Runs src/lua/tail.lua against a stand-in for Rex's block calls: a pane of 5000 rows of 457
-- bytes whose blob_read replies at most <cap> bytes (<later> after the first reply, when given).
-- Usage: lua tail-harness.lua <src/lua dir> <max_bytes> <cap> [later]
-- Prints "<ok> <reads> <cut> <ends at the last row> <bytes>".
local dir, budget, cap, later = arg[1], tonumber(arg[2]), tonumber(arg[3]), tonumber(arg[4])
dofile(dir .. "/tail.lua")
local rows = {}
for n = 1, 5000 do rows[n] = string.format("row%04d %s\n", n, string.rep("x", 449)) end
local data = table.concat(rows)
local reads = 0
rex = {
  call = function() return {} end,
  block = {
    call = function(_, method, a)
      if method == "format" then return { id = "pane", total = #data } end
      reads = reads + 1
      local limit = (reads > 1 and later) or cap
      local stop = math.min(#data, a.offset + math.min(a.max_bytes, limit))
      return { data = data:sub(a.offset + 1, stop), eof = stop >= #data }
    end,
  },
}
local got = tail({ session_id = "s", block_id = "b", format = "text", max_bytes = budget })
local ends = got.ok and got.text:find("row5000", 1, true) ~= nil
print(got.ok, reads, tostring(got.cut), tostring(ends), got.ok and #got.text or got.error)
