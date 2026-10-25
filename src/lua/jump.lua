-- Lands the Rex app on one pane: focus the pane on the server that owns it, then ask the app to
-- select its session. Returns { ok = true }, or { ok = false, step, error } or { ok = false, code }.
-- Runs after app.lua, which defines `a`, `fail`, `apps_in`, `labels` and `local_app`.
local remote = a.host ~= "local"
local api = remote and rex.server(a.host) or rex

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

-- focus_block acts only on a session this stream is attached to.
local _, aerr = api.call("session.attach", { session_id = a.session_id })
if aerr then return fail("session.attach", aerr) end
local _, ferr = api.call("session.focus_block", { session_id = a.session_id, block_id = a.block_id })
if ferr then return fail("session.focus_block", ferr) end

local _, perr = api.call("client.perform", {
  client_id = app.client_id,
  action = "session.select",
  args = { session_id = a.session_id, window_id = a.window_id },
  origin = "api",
  -- Under rexboard's 10s limit on rex do, so an app that does not answer (as on a locked
  -- screen) fails here with its own reason.
  timeout_ms = 5000,
})
if perr then return fail("session.select", perr) end
return { ok = true }
