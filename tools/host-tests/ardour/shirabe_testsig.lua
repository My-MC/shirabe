ardour { ["type"] = "dsp", name = "Shirabe Test Signal", category = "Utility",
  license = "MIT", description = "Deterministic stereo test signal (sines + hashed noise) derived from the transport position" }
function dsp_ioconfig () return { { audio_in = 0, audio_out = 2 } } end
local sr = 48000
function dsp_init (rate) sr = rate end
-- Noise from a hash of the absolute sample position, so every render is identical.
local function noise (n)
  local h = (n * 2654435761 + 1013904223) % 4294967296
  h = (h ~ (h >> 13)) * 1274126177 % 4294967296
  return (h / 4294967296 - 0.5) * 0.2
end
function dsp_run (ins, outs, n_samples)
  local l = outs[1]:array()
  local r = outs[2]:array()
  local pos = Session:transport_sample ()
  for i = 1, n_samples do
    local n = pos + i - 1
    local w = noise (n)
    l[i] = 0.5 * math.sin (2 * math.pi * 997 * n / sr) + w
    r[i] = 0.25 * math.sin (2 * math.pi * 1501 * n / sr) - w
  end
end
