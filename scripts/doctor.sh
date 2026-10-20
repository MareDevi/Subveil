#!/usr/bin/env bash
set -euo pipefail

rustc --version
cargo --version
rustfmt --version
cargo clippy --version

if [[ "$(uname -s)" != Linux ]]; then
    echo "M00 runtime validation supports Linux only." >&2
    exit 1
fi

pkg-config --modversion wayland-client xkbcommon fontconfig freetype2 vulkan openssl libzstd
echo "PipeWire (needed at M02):"
pkg-config --modversion libpipewire-0.3
echo "Display: WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}, DISPLAY=${DISPLAY:-unset}"
