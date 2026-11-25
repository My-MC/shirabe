#!/bin/sh
# Linux host tests for the Shirabe plug-ins.
#
#   1. Bundle VST3/CLAP (release) and link them into ~/.vst3 and ~/.clap
#   2. clap-validator
#   3. pluginval (strictness 5; editor tests only when $DISPLAY is set)
#   4. Ardour (headless): render through the plug-in and compare with a bypassed render
#
# Environment:
#   SHIRABE_GL_SHIM   directory prepended to LD_LIBRARY_PATH for the editor tests
#                     (only needed on X servers without an sRGB-capable GLX config)
#   PLUGINVAL         path to pluginval (default: pluginval in PATH)
#   CLAP_VALIDATOR    path to clap-validator (default: clap-validator in PATH)
#   SKIP_BUNDLE=1     use the existing target/bundled output
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/tools/host-tests"
WORK="$ROOT/target/host-tests"
NAME="Shirabe Meter Suite"
VST3="$HOME/.vst3/$NAME.vst3"
CLAP="$HOME/.clap/$NAME.clap"
PLUGINVAL="${PLUGINVAL:-pluginval}"
CLAP_VALIDATOR="${CLAP_VALIDATOR:-clap-validator}"
if [ -n "${SHIRABE_GL_SHIM:-}" ]; then
  export LD_LIBRARY_PATH="$SHIRABE_GL_SHIM${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

failed=""
step() { printf '\n=== %s\n' "$1"; }
fail() { echo "FAILED: $1"; failed="$failed $1"; }

step "bundle"
if [ "${SKIP_BUNDLE:-}" != 1 ]; then
  (cd "$ROOT" && cargo xtask bundle shirabe-plugin --release)
fi
mkdir -p "$HOME/.vst3" "$HOME/.clap"
ln -sfn "$ROOT/target/bundled/$NAME.vst3" "$VST3"
ln -sfn "$ROOT/target/bundled/$NAME.clap" "$CLAP"

step "clap-validator"
# Known failures in the plug-in framework's CLAP state handling are tracked in
# https://github.com/My-MC/shirabe/issues/3 and excluded until it is fixed.
KNOWN_CLAP_FAILURES='^(state-invalid-random|state-reproducibility-basic|state-reproducibility-binary|state-reproducibility-buffered)$'
"$CLAP_VALIDATOR" validate --only-failed -x "$KNOWN_CLAP_FAILURES" "$CLAP" || fail clap-validator

step "pluginval"
if [ -n "${DISPLAY:-}" ]; then gui=""; else gui="--skip-gui-tests"; fi
"$PLUGINVAL" --strictness-level 5 $gui --validate "$VST3" > "$ROOT/target/pluginval.log" 2>&1 \
  && tail -n 1 "$ROOT/target/pluginval.log" \
  || { tail -n 20 "$ROOT/target/pluginval.log"; fail pluginval; }

step "ardour pass-through"
if command -v ardour8-lua >/dev/null 2>&1; then
  rm -rf "$WORK"
  mkdir -p "$WORK/config/ardour8/scripts" "$WORK/config/ardour8/export" "$WORK/cache"
  cp "$HERE/ardour/shirabe_testsig.lua" "$WORK/config/ardour8/scripts/"
  cp "$HERE/ardour/shirabe-f32.preset" "$WORK/config/ardour8/export/"
  export XDG_CONFIG_HOME="$WORK/config" XDG_CACHE_HOME="$WORK/cache"
  # Ardour only indexes VST3 plug-ins on demand; index ours explicitly.
  LD_LIBRARY_PATH=/usr/lib/ardour8 /usr/lib/ardour8/ardour-vst3-scanner -q -f "$VST3" || true
  (cd "$WORK" && ardour8-lua "$HERE/ardour/passthrough.lua" "$WORK/session" "$NAME" vst3) \
    > "$WORK/ardour.log" 2>&1 || { tail -n 20 "$WORK/ardour.log"; fail ardour; }
  out="$WORK/session/out"
  if [ -f "$out/oscillator.wav" ]; then
    python3 "$HERE/compare_wav.py" "$out/active.wav" "$out/bypassed.wav" || fail ardour-passthrough
    # Negative control: the comparison must notice when the plug-in changes the audio.
    if python3 "$HERE/compare_wav.py" "$out/active.wav" "$out/oscillator.wav" >/dev/null 2>&1; then
      fail ardour-negative-control
    else
      echo "negative control: differences detected as expected"
    fi
  fi
else
  echo "ardour8-lua not found; skipped"
fi

echo
if [ -n "$failed" ]; then
  echo "host tests FAILED:$failed"
  exit 1
fi
echo "host tests passed"
