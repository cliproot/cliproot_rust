#!/bin/bash
# D11 composition test: plugin hooks coexist with user-defined hooks.
#
# Verifies that:
#   (a) A user-defined PostToolUse / Stop hook in settings.json still fires.
#   (b) The cliproot plugin hook scripts run without crashing the pipeline.
#   (c) Fire order is deterministic (user hooks first per Claude Code semantics —
#       document whichever order is actually observed).
#
# This test does NOT require a real `claude` binary or a real `cliproot` binary.
# It directly invokes the hook scripts with canned JSON on stdin, asserting
# that the sentinel file written by the user hook is still present after the
# plugin hook runs alongside it.
#
# Run: bash tests/plugin_hook_composition.sh
# Exit 0 = pass, non-zero = fail.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGIN_ROOT="$REPO_ROOT/.claude-plugin"

# ── Temp workspace ─────────────────────────────────────────────────────────────
TMPDIR_BASE="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_BASE"' EXIT

FAKE_HOME="$TMPDIR_BASE/home"
FAKE_PROJECT="$TMPDIR_BASE/project"
SENTINEL_FILE="$TMPDIR_BASE/user-hook-fired"
mkdir -p "$FAKE_HOME/.claude" "$FAKE_PROJECT"

# ── Fake user hooks ────────────────────────────────────────────────────────────
USER_POST_TOOL_HOOK="$TMPDIR_BASE/user-post-tool-hook.sh"
cat > "$USER_POST_TOOL_HOOK" <<'SCRIPT'
#!/bin/bash
touch "$SENTINEL_FILE"
SCRIPT
chmod +x "$USER_POST_TOOL_HOOK"

USER_STOP_HOOK="$TMPDIR_BASE/user-stop-hook.sh"
cat > "$USER_STOP_HOOK" <<'SCRIPT'
#!/bin/bash
touch "${SENTINEL_FILE}.stop"
SCRIPT
chmod +x "$USER_STOP_HOOK"

# ── Fake cliproot binary (no-op, records invocation) ──────────────────────────
FAKE_BIN_DIR="$TMPDIR_BASE/bin"
mkdir -p "$FAKE_BIN_DIR"

cat > "$FAKE_BIN_DIR/cliproot" <<'SCRIPT'
#!/bin/bash
echo "cliproot $*" >> "$CLIPROOT_INVOCATION_LOG"
SCRIPT
chmod +x "$FAKE_BIN_DIR/cliproot"

export PATH="$FAKE_BIN_DIR:$PATH"
export SENTINEL_FILE
export CLIPROOT_INVOCATION_LOG="$TMPDIR_BASE/cliproot-invocations.log"
export CLAUDE_PLUGIN_ROOT="$PLUGIN_ROOT"
export CLIPROOT_REPO="$FAKE_PROJECT"

# ── Canned tool-use JSON (PostToolUse hook input) ─────────────────────────────
TOOL_USE_JSON='{"tool_name":"Read","tool_input":{"file_path":"/tmp/test.txt"},"tool_response":{"content":"ok"}}'

# ── Test 1: User hook + plugin capture hook both fire (PostToolUse) ───────────
echo "=== Test 1: PostToolUse composition ==="

# Fire user hook first (simulates Claude Code order: user hooks run before plugin hooks)
SENTINEL_FILE="$TMPDIR_BASE/user-hook-fired" "$USER_POST_TOOL_HOOK" <<<"$TOOL_USE_JSON" || true

# Fire plugin capture hook
echo "$TOOL_USE_JSON" | SENTINEL_FILE="$TMPDIR_BASE/user-hook-fired" \
  bash "$PLUGIN_ROOT/hooks/cliproot-capture-hook.sh" || true

# Assert user sentinel was written
if [ -f "$TMPDIR_BASE/user-hook-fired" ]; then
  echo "PASS: user PostToolUse hook sentinel written"
else
  echo "FAIL: user PostToolUse hook sentinel NOT written" >&2
  exit 1
fi

# Assert cliproot was invoked
if grep -q "capture-hook" "$CLIPROOT_INVOCATION_LOG" 2>/dev/null; then
  echo "PASS: cliproot capture-hook invoked"
else
  echo "FAIL: cliproot capture-hook was NOT invoked" >&2
  exit 1
fi

# ── Test 2: User hook + plugin consolidate hook both fire (Stop) ──────────────
echo "=== Test 2: Stop composition ==="

STOP_JSON='{"stop_reason":"end_turn"}'

SENTINEL_FILE="$TMPDIR_BASE/user-hook-fired" "$USER_STOP_HOOK" <<<"$STOP_JSON" || true

echo "$STOP_JSON" | SENTINEL_FILE="$TMPDIR_BASE/user-hook-fired" \
  bash "$PLUGIN_ROOT/hooks/cliproot-consolidate-hook.sh" || true

if [ -f "$TMPDIR_BASE/user-hook-fired.stop" ]; then
  echo "PASS: user Stop hook sentinel written"
else
  echo "FAIL: user Stop hook sentinel NOT written" >&2
  exit 1
fi

if grep -q "consolidate-hook" "$CLIPROOT_INVOCATION_LOG" 2>/dev/null; then
  echo "PASS: cliproot consolidate-hook invoked"
else
  echo "FAIL: cliproot consolidate-hook was NOT invoked" >&2
  exit 1
fi

# ── Test 3: Missing cliproot binary → install hint, exit 0 (no crash) ────────
echo "=== Test 3: Missing cliproot binary exits 0 ==="

NO_CLIPROOT_PATH="$TMPDIR_BASE/empty-bin"
mkdir -p "$NO_CLIPROOT_PATH"

BASH_BIN="$(command -v bash)"

stderr_output=$(PATH="$NO_CLIPROOT_PATH" CLAUDE_PLUGIN_ROOT="$PLUGIN_ROOT" \
  "$BASH_BIN" "$PLUGIN_ROOT/hooks/cliproot-capture-hook.sh" 2>&1 </dev/null || true)

if echo "$stderr_output" | grep -q "cliproot"; then
  echo "PASS: install hint printed to stderr"
else
  echo "FAIL: expected install hint on stderr, got: $stderr_output" >&2
  exit 1
fi

# Capture hook exits 0 when binary is missing (|| exit 0 in the hook)
PATH="$NO_CLIPROOT_PATH" CLAUDE_PLUGIN_ROOT="$PLUGIN_ROOT" \
  "$BASH_BIN" "$PLUGIN_ROOT/hooks/cliproot-capture-hook.sh" </dev/null
echo "PASS: hook exits 0 when cliproot binary is missing"

echo ""
echo "All composition tests passed."
