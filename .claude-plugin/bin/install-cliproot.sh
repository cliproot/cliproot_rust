#!/bin/bash
# install-cliproot.sh — PATH-check for the cliproot binary.
# Called at the start of every hook script.
# Exits 0 silently if cliproot is found.
# Exits 1 with an install hint on stderr if cliproot is missing.

if command -v cliproot >/dev/null 2>&1; then
  exit 0
fi

OS="$(uname -s 2>/dev/null || printf 'Unknown')"

printf '[cliproot] '"'"'cliproot'"'"' binary not found on PATH — hooks are disabled.\n' >&2
printf 'To enable provenance capture, install cliproot:\n\n' >&2

case "$OS" in
  Darwin)
    printf '  macOS — install via cargo (recommended):\n' >&2
    printf '    cargo install --git https://github.com/cliproot/cliproot-rust cliproot-cli\n\n' >&2
    printf '  Or download a pre-built binary from:\n' >&2
    printf '    https://github.com/cliproot/cliproot-rust/releases/latest\n' >&2
    printf '    (cliproot-aarch64-apple-darwin.tar.gz  or  cliproot-x86_64-apple-darwin.tar.gz)\n' >&2
    ;;
  Linux)
    printf '  Linux — install via cargo (recommended):\n' >&2
    printf '    cargo install --git https://github.com/cliproot/cliproot-rust cliproot-cli\n\n' >&2
    printf '  Or download a pre-built binary from:\n' >&2
    printf '    https://github.com/cliproot/cliproot-rust/releases/latest\n' >&2
    printf '    (cliproot-x86_64-unknown-linux-gnu.tar.gz)\n' >&2
    ;;
  *)
    printf '  Windows / Other — install via cargo:\n' >&2
    printf '    cargo install --git https://github.com/cliproot/cliproot-rust cliproot-cli\n\n' >&2
    printf '  Or download a pre-built binary from:\n' >&2
    printf '    https://github.com/cliproot/cliproot-rust/releases/latest\n' >&2
    printf '    (cliproot-x86_64-pc-windows-msvc.zip)\n' >&2
    ;;
esac

exit 1
