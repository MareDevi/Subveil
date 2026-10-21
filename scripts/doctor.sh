#!/usr/bin/env bash
set -euo pipefail

rustc --version
cargo --version
rustfmt --version
cargo clippy --version
for tool in cmake clang glslc; do
    command -v "$tool" >/dev/null || { echo "Missing native build tool: $tool" >&2; exit 1; }
done

if [[ "$(uname -s)" != Linux ]]; then
    echo "Runtime validation currently supports Linux only." >&2
    exit 1
fi

pkg-config --modversion wayland-client xkbcommon fontconfig freetype2 vulkan openssl libzstd
echo "PipeWire (system output capture):"
pkg-config --modversion libpipewire-0.3
if command -v secret-tool >/dev/null; then
    echo "Optional credential storage: secret-tool found; Secret Service availability is checked on use."
else
    echo "Optional credential storage unavailable: install libsecret tools for keyring save/load (memory-only settings remain usable)."
fi
echo "Display: WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}, DISPLAY=${DISPLAY:-unset}"
