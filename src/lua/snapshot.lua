local function on(host, api)
  local ok, snap = pcall(snapshot, api)
  if not ok then return { host = host, error = tostring(snap) } end
  snap.host = host
  return snap
end

local want = rex.args and rex.args.host
local hosts = {}
if not want or want == "local" then hosts[#hosts + 1] = on("local", rex) end
for _, label in ipairs(rex.servers()) do
  if not want or want == label then hosts[#hosts + 1] = on(label, rex.server(label)) end
end
return { hosts = hosts }
