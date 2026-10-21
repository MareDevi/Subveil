#!/usr/bin/env bash
set -euo pipefail
prefix="${1:-$HOME/.local}"
[[ "$prefix" == /* ]] || { echo "Install prefix must be absolute" >&2; exit 1; }
[[ "$prefix" != *$'\n'* && "$prefix" != *$'\r'* ]] || { echo "Install prefix contains a line break" >&2; exit 1; }
source_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
install -d "$prefix/lib/subveil" "$prefix/bin" "$prefix/share/applications" "$prefix/share/doc/subveil"
cp -a "$source_dir/lib/subveil/." "$prefix/lib/subveil/"
cp -a "$source_dir/share/doc/subveil/." "$prefix/share/doc/subveil/"
# A CPU bundle must not retain a CUDA helper from a different build.
if [[ ! -f "$source_dir/lib/subveil/subveil-whisper-cuda" ]]; then
    rm -f -- "$prefix/lib/subveil/subveil-whisper-cuda"
fi
ln -sfn ../lib/subveil/subveil "$prefix/bin/subveil"
# Desktop Entry Exec uses its own quoting rules, not shell quoting.
desktop_exec="${prefix//\\/\\\\}"
desktop_exec="${desktop_exec//\"/\\\"}"
desktop_exec="${desktop_exec//\$/\\\$}"
desktop_exec="${desktop_exec//\`/\\\`}"
desktop_exec="${desktop_exec//%/%%}"
printf '[Desktop Entry]\nType=Application\nName=Subveil\nExec="%s/lib/subveil/subveil"\nIcon=accessories-text-editor\nCategories=AudioVideo;Accessibility;\nTerminal=false\n' "$desktop_exec" > "$prefix/share/applications/subveil.desktop"
"$prefix/bin/subveil" --check-installation
