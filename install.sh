#!/bin/sh
set -eu
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || { echo 'Only Linux x86_64 is supported.' >&2; exit 1; }
install_dir=${INSTALL_DIR:-"$HOME/.local/bin"}
mkdir -p "$install_dir"
tmp=$(mktemp "$install_dir/.cmrak.XXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
curl --fail --location --retry 3 --header 'Cache-Control: no-cache' --output "$tmp" 'https://github.com/jay-waves/cmrak/releases/download/0.5.0/cmrak-linux-x64'
chmod 755 "$tmp"
mv -f "$tmp" "$install_dir/cmrak"

echo "Installed cmrak to $install_dir/cmrak"
case ":$PATH:" in *":$install_dir:"*) ;; *) echo "Add $install_dir to PATH to run cmrak from any directory." ;; esac
