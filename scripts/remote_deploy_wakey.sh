#!/bin/sh
# Usage: remote_deploy_wakey.sh <bin_tmp> <dest_path> [restart_flag]
# restart_flag: 1 (default) to stop/start, 0 to only replace file.
# Or stream over SSH: sh -s -- --batch [--binary <tmp> <dest> <restart> |
#                                     --install <tmp> <dest>]...

set -e

# A subshell keeps per-binary variables and exit statements local, including
# when several replacements are requested by one dev_push invocation.
deploy_binary() (
    BIN_TMP="$1"
    DEST="$2"
    RESTART="${3:-1}"
    INIT="/etc/init.d/wakey"
    KILL="/root/.bin/kill_wakey.sh"

    if [ -z "$BIN_TMP" ] || [ -z "$DEST" ]; then
        echo "usage: $0 <bin_tmp> <dest_path> [restart_flag]" >&2
        exit 2
    fi

    [ -f "$BIN_TMP" ] || {
        echo "tmp binary not found: $BIN_TMP" >&2
        exit 1
    }
    chmod +x "$BIN_TMP" || true

    if [ "$RESTART" = "1" ]; then
        if [ -x "$INIT" ]; then
            "$INIT" stop || true
        elif [ -x "$KILL" ]; then
            sh "$KILL" || true
        fi
    fi

    mv -f "$BIN_TMP" "$DEST"

    if [ "$RESTART" = "1" ]; then
        if [ -x "$INIT" ]; then
            "$INIT" start || "$INIT" restart || true
        else
            nohup "$DEST" >/dev/null 2>&1 &
        fi
    fi

    exit 0
)

install_support_file() {
    sed -i 's/\r$//' "$1" 2>/dev/null || true
    mkdir -p "$(dirname "$2")"
    cp -f "$1" "$2"
    chmod 0755 "$2"
}

if [ "${1:-}" != '--batch' ]; then
    deploy_binary "$@"
    exit
fi

shift
while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 4 ] || { echo '--binary requires tmp, destination, restart' >&2; exit 2; }
            deploy_binary "$2" "$3" "$4"
            shift 4
            ;;
        --install)
            [ "$#" -ge 3 ] || { echo '--install requires tmp and destination' >&2; exit 2; }
            install_support_file "$2" "$3"
            shift 3
            ;;
        *)
            echo "unknown deployment operation: $1" >&2
            exit 2
            ;;
    esac
done
