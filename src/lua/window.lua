-- Shows the rexboard session in this Mac's Rex app. Runs after app.lua.
-- mode "select": select the session; the app focuses a window that already shows it.
-- mode "new": open a new app window, show the session there, then destroy the session the new
-- window came with, so no unlabelled session is left behind.
-- Returns { ok = true, opened, made, destroyed }, or a failure shaped as in app.lua.
local app, why = local_app()
if not app then return why end

local function perform(action, args)
  return rex.call("client.perform", {
    client_id = app.client_id, action = action, args = args, origin = "api", timeout_ms = 5000,
  })
end

local show = { session_id = a.session_id, window_id = a.window_id }
if a.mode ~= "new" then
  local _, err = perform("session.select", show)
  if err then return fail("session.select", err) end
  return { ok = true, opened = false }
end

local function ids()
  local out = {}
  for _, s in ipairs((rex.call("session.list") or {}).sessions or {}) do out[s.session_id] = true end
  return out
end

local before = ids()
local _, werr = perform("client.window.new")
if werr then return fail("client.window.new", werr) end
local made
for _ = 1, 20 do
  for id in pairs(ids()) do
    if not before[id] and id ~= a.session_id then made = id end
  end
  if made then break end
end

local function destroy()
  if not made then return false end
  local _, derr = rex.call("session.destroy", { session_id = made })
  return derr == nil
end

local _, serr = perform("session.select", show)
if serr then
  local reply = fail("session.select", serr)
  reply.made, reply.destroyed = made, destroy()
  return reply
end
return { ok = true, opened = true, made = made, destroyed = destroy() }
