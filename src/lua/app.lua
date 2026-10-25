-- Shared by jump.lua and window.lua: this Mac's Rex app among a server's clients.
local a = rex.args

local function fail(step, err)
  return { ok = false, step = step, error = tostring(err) }
end

local function apps_in(list)
  local apps = {}
  for _, c in ipairs(list and list.clients or {}) do
    if c.info and c.info.kind == "app" then apps[#apps + 1] = c end
  end
  return apps
end

local function labels(apps)
  local out = {}
  for _, c in ipairs(apps) do out[#out + 1] = (c.info and c.info.label) or c.client_id end
  return out
end

-- This Mac's app on the local server: the one asked for, else the only app on the unix socket.
local function local_app()
  local list, err = rex.call("client.list")
  if not list then return nil, fail("client.list", err) end
  local apps = apps_in(list)
  if a.app_client then
    for _, c in ipairs(apps) do
      if c.client_id == a.app_client or (c.info and c.info.label == a.app_client) then return c end
    end
    return nil, { ok = false, code = "no_client", error = "no Rex app client named " .. a.app_client,
      labels = labels(apps) }
  end
  if #apps == 0 then return nil, { ok = false, code = "no_app" } end
  local mine = {}
  for _, c in ipairs(apps) do
    if c.principal and c.principal.transport == "unix" then mine[#mine + 1] = c end
  end
  if #mine == 1 then return mine[1] end
  if #mine == 0 then return nil, { ok = false, code = "no_unix_app", labels = labels(apps) } end
  return nil, { ok = false, code = "ambiguous_client", labels = labels(mine) }
end
