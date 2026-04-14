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
#   WAKEY_CC_VERSION   Optional tag, e.g. v0.1.0 (if omitted, latest release is used)
#   WAKEY_CC_TARGET    Target triple (default from uname: x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu)
#   WAKEY_CC_FILE      Asset filename (default: wakey-cc-${WAKEY_CC_VERSION}-${WAKEY_CC_TARGET}.tgz)
#   WAKEY_CC_ROOT      Install root (default: current directory)
#   WAKEY_CC_SERVICE   systemd unit name (default: wakey-cc.service)
#   WAKEY_CC_NO_RESTART If set, skip systemd restart
#   WAKEY_INSECURE     If set, disable TLS verification
#
# Requires: tar, systemctl, jq, curl or wget

set -eu

log() { printf '[update_wakey_cc] %s\n' "$*"; }
fail() {
    printf '[update_wakey_cc] ERROR: %s\n' "$*" >&2
    exit 1
}

http_get() {
    # http_get <url>
    if command -v curl >/dev/null 2>&1; then
        if [ -n "${WAKEY_INSECURE:-}" ]; then
            curl -fsSL -k "$1"
        else
            curl -fsSL "$1"
        fi
        return
    fi

    if command -v wget >/dev/null 2>&1; then
        # shellcheck disable=SC2086
        wget ${WAKEY_INSECURE:+--no-check-certificate} -qO- "$1"
        return
    fi

    fail 'curl or wget is required'
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

latest_asset_url() {
    # latest_asset_url <host> <owner> <repo> <target> [file_hint]
    HOST="$1"
    OWNER="$2"
    REPO="$3"
    TARGET="$4"
    FILE_HINT="${5:-}"
    API="https://$HOST/api/v1/repos/$OWNER/$REPO/releases/latest"

    command -v jq >/dev/null 2>&1 || fail 'jq is required for latest release resolution'

    RESP=$(http_get "$API") || return 1

    if [ -n "$FILE_HINT" ]; then
        echo "$RESP" |
            jq -r --arg file "$FILE_HINT" '
                .assets[]?
                | select(
                    (.name // "") == $file
                    or ((.browser_download_url // "") | endswith("/" + $file) or endswith($file))
                )
                | .browser_download_url
            ' |
            head -n1
        return
    fi

    echo "$RESP" |
        jq -r --arg target "$TARGET" '
            .assets[]?
            | select(
                ((.name // "") | startswith("wakey-cc-"))
                and ((.name // "") | endswith("-" + $target + ".tgz"))
            )
            | .browser_download_url
        ' |
        head -n1
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
        TARGET="${WAKEY_CC_TARGET:-$(default_target)}"
        VERSION="${WAKEY_CC_VERSION:-}"
        FILE="${WAKEY_CC_FILE:-}"

        if [ -n "$VERSION" ]; then
            if [ -z "$FILE" ]; then
                FILE="wakey-cc-${VERSION}-${TARGET}.tgz"
            fi
            URL="https://$HOST/$OWNER/$REPO/releases/download/$VERSION/$FILE"
        else
            URL=$(latest_asset_url "$HOST" "$OWNER" "$REPO" "$TARGET" "$FILE") ||
                fail 'unable to resolve latest release asset URL'
            [ -n "$URL" ] ||
                fail "no latest control-plane asset found for target $TARGET"
        fi
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
