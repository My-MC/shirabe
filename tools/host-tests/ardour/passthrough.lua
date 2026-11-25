-- Headless Ardour check: render a deterministic signal through the plugin, once with the
-- plugin active and once deactivated, and export both as 32-bit float WAV.
-- Usage: ardour8-lua passthrough.lua <session-dir> <plugin-name> [vst3]
-- Run through tools/host-tests/run.sh, which prepares the Ardour config directory.
local dir, plugname, kind = arg[1], arg[2], arg[3] or "vst3"
local ptype = ({vst3 = ARDOUR.PluginType.VST3, lv2 = ARDOUR.PluginType.LV2})[kind]

AudioEngine:set_backend ("None (Dummy)", "", "")
local s = create_session (dir, "pt", 48000)
assert (s, "session")
local rl = s:new_audio_route (0, 2, nil, 1, "Bus", ARDOUR.PresentationInfo.Flag.AudioBus, ARDOUR.PresentationInfo.max_order)
local bus = rl:front ()
local gen = ARDOUR.LuaAPI.new_luaproc (s, "Shirabe Test Signal")
assert (not gen:isnil (), "generator")
bus:add_processor_by_index (gen, 0, nil, true)
local p = ARDOUR.LuaAPI.new_plugin (s, plugname, ptype, "")
assert (not p:isnil (), "plugin not found: " .. plugname)
bus:add_processor_by_index (p, 1, nil, true)
print ("chain:")
local i = 0
while true do
  local proc = bus:nth_plugin (i)
  if proc:isnil () then break end
  print ("  " .. i .. ": " .. proc:display_name () .. " active=" .. tostring (proc:active ()))
  i = i + 1
end

local function export (name)
  s:goto_start (false)
  local e = s:simple_export ()
  e:set_name (name)
  e:set_folder (dir .. "/out")
  e:set_range (Temporal.timepos_t (0):samples (), 48000 * 3)
  assert (e:set_preset ("5d1c3b8e-2f4a-4c55-9b7e-5f3a1d2c9e01"), "preset")
  assert (e:check_outputs (), "outputs")
  assert (e:run_export (), "export " .. name)
  print ("exported " .. name)
end

export ("active")
p:deactivate ()
export ("bypassed")
-- Negative control: with the test oscillator on, the output must differ.
p:activate ()
local osc = ARDOUR.LuaAPI.set_processor_param (p, 0, 1.0)
print ("test oscillator on: " .. tostring (osc))
export ("oscillator")
-- Leave the session in a normal state for manual checks.
ARDOUR.LuaAPI.set_processor_param (p, 0, 0.0)
s:save_state ("", false, false, false, false, false)
close_session ()
