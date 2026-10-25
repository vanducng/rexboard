-- The Rex calls behind `rexboard window`; rexboard decides what to call. Runs after app.lua.
-- mode "open": ask this Mac's app for a new window; the reply is the app's answer.
-- mode "select": ask the app to show the session (it focuses a window that already shows it).
-- mode "destroy": destroy the one session rexboard named, by its id.
if a.mode == "destroy" then
  -- session.destroy acts only on a session this stream is attached to.
  local _, aerr = rex.call("session.attach", { session_id = a.session_id })
  if aerr then return fail("session.attach", aerr) end
  local _, err = rex.call("session.destroy", { session_id = a.session_id })
  if err then return fail("session.destroy", err) end
  return { ok = true }
end

local app, why = local_app()
if not app then return why end

local function perform(action, args)
  return rex.call("client.perform", {
    client_id = app.client_id, action = action, args = args, origin = "api", timeout_ms = 5000,
  })
end

if a.mode == "open" then
  local reply, err = perform("client.window.new")
  if err then return fail("client.window.new", err) end
  return { ok = true, reply = reply }
end

local _, err = perform("session.select", { session_id = a.session_id, window_id = a.window_id })
if err then return fail("session.select", err) end
return { ok = true }
