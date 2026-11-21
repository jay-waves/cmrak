#!/bin/sh
set -eu
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || { echo 'Only Linux x86_64 is supported.' >&2; exit 1; }
command_path=$(command -v cmrak 2>/dev/null || true)
if [ -n "$command_path" ]; then
    target=$(realpath -- "$command_path")
    install_dir=$(dirname -- "$target")
else
    install_dir=$(realpath -m -- "${INSTALL_DIR:-"$HOME/.local/bin"}")
    target="$install_dir/cmrak"
    case "$install_dir" in
        *:*) echo 'The install directory cannot contain a colon.' >&2; exit 1 ;;
    esac
fi

mkdir -p "$install_dir"
tmp=$(mktemp "$install_dir/.cmrak.XXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
curl --fail --location --retry 3 --header 'Cache-Control: no-cache' --output "$tmp" "https://github.com/jay-waves/cmrak/releases/latest/download/cmrak-linux-x64?cachebust=$(date +%s)"
chmod 755 "$tmp"
# Match the selected executable so other installations are left running.
if [ -f "$target" ]; then
    for exe in /proc/[0-9]*/exe; do
        if [ "$exe" -ef "$target" ]; then
            pid=${exe#/proc/}
            pid=${pid%/exe}
            if ! kill -KILL "$pid" && [ "$exe" -ef "$target" ]; then
                exit 1
            fi
        fi
    done
fi
mv -fT -- "$tmp" "$target"

if [ -z "$command_path" ]; then
    append_path_rule() {
        if [ ! -f "$1" ] || ! grep -Fqx -- "$2" "$1"; then
            printf '\n%s\n' "$2" >> "$1"
        fi
    }

    # Quote independently for POSIX shell and fish startup files.
    quoted_dir=$(printf '%s' "$install_dir" | sed "s/'/'\\\\''/g")
    path_rule="case \":\$PATH:\" in *':$quoted_dir:'*) ;; *) export PATH='$quoted_dir':\"\$PATH\" ;; esac"
    login_shell=${SHELL:-sh}
    case "${login_shell##*/}" in
        bash)
            login_profile="$HOME/.profile"
            for profile in "$HOME/.bash_profile" "$HOME/.bash_login"; do
                if [ -r "$profile" ]; then
                    login_profile="$profile"
                    break
                fi
            done
            append_path_rule "$login_profile" "$path_rule"
            append_path_rule "$HOME/.bashrc" "$path_rule"
            ;;
        zsh)
            append_path_rule "${ZDOTDIR:-$HOME}/.zshenv" "$path_rule"
            ;;
        fish)
            fish_config=${XDG_CONFIG_HOME:-"$HOME/.config"}/fish/conf.d
            mkdir -p -- "$fish_config"
            fish_dir=$(printf '%s' "$install_dir" | sed -e 's/\\/\\\\/g' -e "s/'/\\\\'/g")
            append_path_rule "$fish_config/cmrak-path.fish" "fish_add_path --path '$fish_dir'"
            ;;
        *)
            append_path_rule "$HOME/.profile" "$path_rule"
            ;;
    esac
    case ":$PATH:" in *":$install_dir:"*) ;; *) export PATH="$install_dir:$PATH" ;; esac
fi

echo "Installed cmrak to $target"
"$target" --version
