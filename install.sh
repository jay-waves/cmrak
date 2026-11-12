#!/bin/sh
set -eu
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || { echo 'Only Linux x86_64 is supported.' >&2; exit 1; }
command_path=$(command -v cmrak 2>/dev/null || true)
if [ -n "$command_path" ]; then install_dir=$(dirname "$command_path"); target="$command_path"; else install_dir=${INSTALL_DIR:-"$HOME/.local/bin"}; target="$target"; fi
mkdir -p "$install_dir"
tmp=$(mktemp "$install_dir/.cmrak.XXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
curl --fail --location --retry 3 --header 'Cache-Control: no-cache' --output "$tmp" "https://github.com/jay-waves/cmrak/releases/latest/download/cmrak-linux-x64?cachebust=$(date +%s)"
chmod 755 "$tmp"
mv -f "$tmp" "$target"

echo "Installed cmrak to $install_dir/cmrak"
"$target" --version
case ":$PATH:" in *":$install_dir:"*) ;; *) echo "Add $install_dir to PATH: export PATH=\"$install_dir:$PATH\"" ;; esac
