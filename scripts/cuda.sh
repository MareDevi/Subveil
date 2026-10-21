#!/usr/bin/env bash
set -euo pipefail
# The default targets the project's RTX 4060 Laptop (Ada / SM89). Override for other GPUs.
export CMAKE_CUDA_ARCHITECTURES="${CMAKE_CUDA_ARCHITECTURES:-89}"
# CachyOS ships GCC 16; CUDA 13.4 supports GCC 15 as the host compiler.
if [[ -z "${CMAKE_CUDA_HOST_COMPILER:-}" ]] && command -v g++-15 >/dev/null; then
    export CMAKE_CUDA_HOST_COMPILER
    CMAKE_CUDA_HOST_COMPILER="$(command -v g++-15)"
fi
command -v nvcc >/dev/null || { echo "CUDA toolkit missing: install it with the OS package manager." >&2; exit 1; }
exec cargo "$@"
