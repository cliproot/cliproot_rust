#!/usr/bin/env bash
# Quick provenance verification for a document.
# Usage: verify-provenance.sh [document-file]
set -euo pipefail

echo "=== Verifying clip integrity ==="
cliproot verify

if [ -n "${1:-}" ]; then
    echo ""
    echo "=== Provenance coverage for $1 ==="
    cliproot doctor "$1"
fi
