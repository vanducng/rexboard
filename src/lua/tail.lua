-- Reads the end of one pane: `format` with a tiny max_bytes gives the size and a blob id, then
-- `blob_read` reads only the last a.max_bytes. Rex renders the whole scrollback on its side, but
-- only the tail crosses the wire. Defines `tail` only, so another script can reuse it.
-- Returns { ok = true, text, total, cut }, or { ok = false, step, error }.
function tail(a)
  local term = "com.superlogical.terminal"
  local function fail(step, err)
    return { ok = false, step = step, error = tostring(err) }
  end
  local function call(method, args)
    args.session_id, args.block_id = a.session_id, a.block_id
    return rex.block.call(term, method, args)
  end
  -- Block methods answer only on a stream attached to the block's session.
  local _, aerr = rex.call("session.attach", { session_id = a.session_id })
  if aerr then return fail("session.attach", aerr) end
  -- max_bytes 0 would mean everything.
  local f, ferr = call("format", { format = "text", unwrap = true, max_bytes = 1 })
  if not f then return fail("format", ferr) end
  if not f.id then
    local text = f.content or ""
    return { ok = true, text = text, total = #text, cut = false }
  end
  local start = math.max(0, f.total - a.max_bytes)
  local parts, offset = {}, start
  -- A read can come back short of max_bytes with more to go.
  for _ = 1, 4 do
    local r, rerr = call("blob_read", { id = f.id, offset = offset, max_bytes = a.max_bytes })
    if not r then return fail("blob_read", rerr) end
    parts[#parts + 1] = r.data
    offset = offset + #r.data
    if r.eof or #r.data == 0 then break end
  end
  local text = table.concat(parts)
  -- Reading from the middle can start partway through a line.
  if start > 0 then text = (text:gsub("^[^\n]*\n", "", 1)) end
  return { ok = true, text = text, total = f.total, cut = start > 0 }
end
