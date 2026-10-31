-- Reads the end of one pane: `format` with a tiny max_bytes gives the size and a blob id, then
-- `blob_read` reads only the last a.max_bytes. Rex renders the whole scrollback on its side, but
-- only the tail crosses the wire. Defines `tail` only, so another script can reuse it.
-- a.format is "text" when left out; "vt" keeps the styles, as answer mode's draft check needs.
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
  local f, ferr = call("format", { format = a.format or "text", unwrap = true, max_bytes = 1 })
  if not f then return fail("format", ferr) end
  if not f.id then
    local text = f.content or ""
    return { ok = true, text = text, total = #text, cut = false }
  end
  -- A reply can come back short of what was asked (Rex caps it), so a read goes on from where it
  -- left off until the pane's end is reached. When that would take more than ROUNDS reads at the
  -- size the first reply showed, the start moves up instead: the text is always the pane's end,
  -- never a prefix of what was asked for.
  local ROUNDS = 40
  local start = math.max(0, f.total - a.max_bytes)
  local parts, offset, rounds = {}, start, 0
  while offset < f.total do
    if rounds >= ROUNDS then
      return fail("blob_read", "the end of the pane was not reached in " .. ROUNDS .. " reads")
    end
    local r, rerr = call("blob_read", { id = f.id, offset = offset, max_bytes = math.min(a.max_bytes, f.total - offset) })
    if not r then return fail("blob_read", rerr) end
    if #r.data == 0 then return fail("blob_read", "an empty read before the end of the pane") end
    rounds = rounds + 1
    parts[#parts + 1] = r.data
    offset = offset + #r.data
    local room = #r.data * (ROUNDS - rounds)
    if rounds == 1 and f.total - offset > room then
      start, offset, parts = f.total - #r.data * (ROUNDS - 1), f.total - #r.data * (ROUNDS - 1), {}
    end
    if r.eof then break end
  end
  local text = table.concat(parts)
  -- Reading from the middle can start partway through a line.
  if start > 0 then text = (text:gsub("^[^\n]*\n", "", 1)) end
  return { ok = true, text = text, total = f.total, cut = start > 0 }
end
