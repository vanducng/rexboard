-- Lands the Rex app on one pane: focus the pane on the server that owns it, then ask the app to
-- select its session. Returns { ok = true }, or { ok = false, step, error } or { ok = false, code }.
local a = rex.args
local remote = a.host ~= "local"
local api = remote and rex.server(a.host) or rex

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
  local mine = {}
  for _, c in ipairs(apps) do
    if c.principal and c.principal.transport == "unix" then mine[#mine + 1] = c end
  end
  if #mine == 1 then return mine[1] end
  return nil, { ok = false, code = "ambiguous_client", labels = labels(#mine > 1 and mine or apps) }
end

-- This Mac's app as a client of a remote server: the app connected from this Mac's node.
local function remote_app()
  local me, err = api.call("server.whoami")
  local node = me and me.principal and me.principal.node
  if not node then return nil, fail("server.whoami", err or ("no Tailscale identity on " .. a.host)) end
  local list, lerr = api.call("client.list")
  if not list then return nil, fail("client.list", lerr) end
  for _, c in ipairs(apps_in(list)) do
    if c.principal and c.principal.node == node then return c end
  end
  return nil, fail("client.list", "this Mac's Rex app is not connected to " .. a.host)
end

local app, why
if remote then app, why = remote_app() else app, why = local_app() end
if not app then return why end

local _, ferr = api.call("session.focus_block", { session_id = a.session_id, block_id = a.block_id })
if ferr then return fail("session.focus_block", ferr) end

local _, perr = api.call("client.perform", {
  client_id = app.client_id,
  action = "session.select",
  args = { session_id = a.session_id, window_id = a.window_id },
  origin = "api",
})
if perr then return fail("session.select", perr) end
return { ok = true }
