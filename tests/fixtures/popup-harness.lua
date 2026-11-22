-- Runs examples/rex/rexboard.lua against a stand-in for the Rex calls it makes.
-- Usage: lua popup-harness.lua <example> <mode> [variant] [zoomed] [flag]
--   binds                      one line per rex.bind: key, action and variant
--   layer VARIANT zoomed|tiled the layer one press builds, and the command's own arguments
--   argv VARIANT zoomed|tiled  the whole command, each element ended by a NUL
--   flag, when given, is what the api variant's `session=api` becomes: a value with a space and a *
--   fails VARIANT|-            the error a press raises: an unknown variant, a pane in no window,
--                              or a layer Rex refuses to create (variant "-" is the default)
local example, mode, variant, view_kind, flag = arg[1], arg[2], arg[3], arg[4], arg[5]
if variant == "-" then
  variant = nil
end

local actions, binds, layers = {}, {}, {}
local refuse = nil
local windows = {
  { window_id = "window:other", layers = { { blocks = { { block_id = "block:elsewhere" } } } } },
  {
    window_id = "window:here",
    zoomed_block_id = view_kind == "zoomed" and "block:pane" or nil,
    layers = { { blocks = { { block_id = "block:pane" }, { block_id = "block:side" } } } },
  },
}
rex = {
  action = function(def) actions[def.name] = def end,
  bind = function(key, action, args) binds[#binds + 1] = { key, action, args } end,
  layout = { block = function(spec) return { block = spec } end },
  session = {
    view = function() return { session_id = "session:s", windows = windows } end,
    new_layer = function(call)
      if refuse then
        return nil, refuse
      end
      layers[#layers + 1] = call
      return { layer_id = "layer:new" }
    end,
  },
}
local file = assert(io.open(example))
local source = file:read("*a")
file:close()
if flag then
  source = source:gsub('"session=api"', function() return '"' .. flag .. '"' end)
end
assert((loadstring or load)(source, "@" .. example))()

local function press(block_id, args)
  return pcall(actions.rexboard.run, { session_id = "session:s", block_id = block_id }, args)
end

if mode == "binds" then
  for _, bind in ipairs(binds) do
    print(bind[1], bind[2], bind[3] and bind[3].variant or "-")
  end
elseif mode == "fails" then
  local block = "block:pane"
  if view_kind == "nowhere" then
    block = "block:nowhere"
  elseif view_kind == "refused" then
    refuse = "no room"
  end
  local ok, err = press(block, { variant = variant })
  print(ok and "no error" or err)
else
  assert(press("block:pane", { variant = variant }))
  assert(#layers == 1, "one press, one layer")
  local layer, command = layers[1], layers[1].layout.block.options.command
  if mode == "layer" then
    print("window " .. layer.window_id)
    print("bounds " .. table.concat(layer.bounds, " "))
    print("focus " .. tostring(layer.focus))
    print("flavor " .. layer.layout.block.flavor)
    print("shell " .. layer.layout.block.options.shell)
    print("run " .. command[1] .. " " .. command[2])
    print("args " .. table.concat(command, " ", 4))
  else
    for _, element in ipairs(command) do
      io.write(element, "\0")
    end
  end
end
