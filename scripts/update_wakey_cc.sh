#!/usr/bin/env sh
# Update/install wakey-control-plane bundle on Linux VPS and optionally restart systemd unit.
#
# Expected tarball layout:
#   bin/wakey-control-plane
#   ui/dist/index.html
#   ui/dist/assets/*
#
# Env options:
#   WAKEY_CC_TGZ_URL   Direct URL to bundle tarball (preferred)
#   WAKEY_HOST         Release host (default: git.ldlda.com)
#   WAKEY_OWNER        Repo owner (default: lda)
#   WAKEY_REPO         Repo name (default: wakey)
#   WAKEY_CC_VERSION   Tag, e.g. v0.1.0 (required unless WAKEY_CC_TGZ_URL set)
#   WAKEY_CC_TARGET    Target triple (default from uname: x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu)
#   WAKEY_CC_FILE      Asset filename (default: wakey-cc-${WAKEY_CC_VERSION}-${WAKEY_CC_TARGET}.tgz)
#   WAKEY_CC_ROOT      Install root (default: current directory)
#   WAKEY_CC_SERVICE   systemd unit name (default: wakey-cc.service)
#   WAKEY_CC_NO_RESTART If set, skip systemd restart
#   WAKEY_INSECURE     If set, disable TLS verification
#
# Requires: tar, systemctl, curl or wget

set -eu

log() { printf '[update_wakey_cc] %s\n' "$*"; }
fail() {
    printf '[update_wakey_cc] ERROR: %s\n' "$*" >&2
    exit 1
}

fetch() {
    # fetch <url> <out>
    if command -v curl >/dev/null 2>&1; then
        if [ -n "${WAKEY_INSECURE:-}" ]; then
            curl -fSL -k -o "$2" "$1"
        else
            curl -fSL -o "$2" "$1"
        fi
        return
    fi

    if command -v wget >/dev/null 2>&1; then
        # shellcheck disable=SC2086
        wget ${WAKEY_INSECURE:+--no-check-certificate} -O "$2" "$1"
        return
    fi

    fail 'curl or wget is required'
}

default_target() {
    arch=$(uname -m)
    case "$arch" in
        x86_64)
            printf '%s' 'x86_64-unknown-linux-gnu'
            ;;
        aarch64|arm64)
            printf '%s' 'aarch64-unknown-linux-gnu'
            ;;
        *)
            printf '%s' "$arch"
            ;;
    esac
}

main() {
    ROOT="${WAKEY_CC_ROOT:-$PWD}"
    SERVICE="${WAKEY_CC_SERVICE:-wakey-cc.service}"
    TMPDIR="${TMPDIR:-/tmp}"
    ARCHIVE="$TMPDIR/wakey-cc.$$.tgz"
    STAGING="$TMPDIR/wakey-cc-stage.$$"

    URL="${WAKEY_CC_TGZ_URL:-}"
    if [ -z "$URL" ]; then
        HOST="${WAKEY_HOST:-git.ldlda.com}"
        OWNER="${WAKEY_OWNER:-lda}"
        REPO="${WAKEY_REPO:-wakey}"
        VERSION="${WAKEY_CC_VERSION:-}"
        [ -n "$VERSION" ] || fail 'set WAKEY_CC_TGZ_URL or WAKEY_CC_VERSION'
        TARGET="${WAKEY_CC_TARGET:-$(default_target)}"
        FILE="${WAKEY_CC_FILE:-wakey-cc-${VERSION}-${TARGET}.tgz}"
        URL="https://$HOST/$OWNER/$REPO/releases/download/$VERSION/$FILE"
    fi

    trap 'rm -f "$ARCHIVE"; rm -rf "$STAGING"' EXIT INT TERM

    log "fetching $URL"
    fetch "$URL" "$ARCHIVE" || fail 'download failed'

    rm -rf "$STAGING"
    mkdir -p "$STAGING"
    tar -xzf "$ARCHIVE" -C "$STAGING" || fail 'extract failed'

    [ -f "$STAGING/bin/wakey-control-plane" ] || fail 'bundle missing bin/wakey-control-plane'
    [ -f "$STAGING/ui/dist/index.html" ] || fail 'bundle missing ui/dist/index.html'

    mkdir -p "$ROOT"
    cp -a "$STAGING/." "$ROOT/"

    if [ -z "${WAKEY_CC_NO_RESTART:-}" ] && command -v systemctl >/dev/null 2>&1; then
        if systemctl list-unit-files "$SERVICE" >/dev/null 2>&1; then
            log "restarting $SERVICE"
            systemctl daemon-reload
            systemctl restart "$SERVICE"
            systemctl --no-pager --full status "$SERVICE" | sed -n '1,16p'
        else
            log "service $SERVICE not installed; skipped restart"
        fi
    fi

    log "done: installed into $ROOT"
}

main "$@"
