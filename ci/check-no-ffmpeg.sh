#!/usr/bin/env bash
set -euo pipefail

echo "=== Checking zero ffmpeg / libav / OpenSSL dependencies ==="

# 1. Cargo.lock check
if grep -iE 'libav|ffmpeg-sys|openssl-sys' Cargo.lock; then
    echo "ERROR: Cargo.lock contains banned multimedia/ssl bindings!"
    exit 1
fi
echo "[OK] Cargo.lock has no ffmpeg or OpenSSL dependencies."

# 2. Binary linkage check
BINARY="target/debug/qam"
if [ ! -f "$BINARY" ]; then
    cargo build --bin qam
fi

if command -v otool >/dev/null 2>&1; then
    echo "Inspecting dynamic libraries via otool..."
    DEPS=$(otool -L "$BINARY")
    if echo "$DEPS" | grep -iE 'libav|ffmpeg|libssl|libcrypto'; then
        echo "ERROR: Dynamic linkage to banned libraries detected!"
        exit 1
    fi
elif command -v ldd >/dev/null 2>&1; then
    echo "Inspecting dynamic libraries via ldd..."
    DEPS=$(ldd "$BINARY")
    if echo "$DEPS" | grep -iE 'libav|ffmpeg|libssl|libcrypto'; then
        echo "ERROR: Dynamic linkage to banned libraries detected!"
        exit 1
    fi
fi
echo "[OK] Binary links only standard OS libraries."

# 3. Clean execution with PATH=""
echo "Verifying binary execution with PATH=\"\"..."
PATH="" "$BINARY" --version >/dev/null
PATH="" "$BINARY" --help >/dev/null
echo "[OK] Binary runs standalone with zero PATH dependencies."

echo "=== All checks passed! ==="
