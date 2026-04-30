#!/usr/bin/env sh
# Build and install wakey-control-plane from repository source on the target host.
# This avoids glibc baseline mismatches from prebuilt artifacts by building locally,
# then packaging the same bundle format and installing it.
#
# Simplest use:
#   ./scripts/update_wakey_cc_from_repo.sh
#
# Env options:
#   WAKEY_CC_REPO_DIR    Existing checkout to update/build instead of cloning
#   WAKEY_CC_REPO_URL    Git URL (default: https://git.ldlda.com/lda/wakey.git)
#   WAKEY_CC_REF         Git ref/branch/tag (default: main)
#   WAKEY_CC_TARGET      Rust target triple (default from uname)
#   WAKEY_CC_ROOT        Install root (default: current directory)
#   WAKEY_CC_SERVICE     systemd unit name (default: wakey-cc.service)
#   WAKEY_CC_USER        Owner for installed files/temp dirs (default: current user)
#   WAKEY_CC_VERSION     Bundle version label (default: v0.0.0-local-<short_sha>)
#   WAKEY_CC_NO_RESTART  If set, skip systemd restart
#   WAKEY_CC_KEEP_TMP    If set, keep temp checkout/build dir for debugging
#   SUDO                 Privilege escalation command/path for install steps (default: sudo)
#
# Requires: git, cargo, tar, pnpm, systemctl (optional for restart)

# Common existing-checkout use:
: <<'USE CASE'
WAKEY_CC_REPO_DIR="$HOME/dev/wakey" \
WAKEY_CC_USER=lda \
WAKEY_CC_KEEP_TMP=1 \
scripts/update_wakey_cc_from_repo.sh
USE CASE
set -eu

log() { printf '[update_wakey_cc_from_repo] %s\n' "$*"; }
fail() {
    printf '[update_wakey_cc_from_repo] ERROR: %s\n' "$*" >&2
    exit 1
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
            fail "unsupported arch for default target mapping: $arch (set WAKEY_CC_TARGET explicitly)"
            ;;
    esac
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

sudo_cmd() {
    if [ "$(id -u)" -eq 0 ]; then
        return 0
    fi

    if [ -n "${SUDO:-}" ]; then
        printf '%s' "$SUDO"
        return 0
    fi

    if command -v sudo >/dev/null 2>&1; then
        printf '%s' sudo
        return 0
    fi

    fail 'root privileges are required for install/restart steps; set SUDO or run as root'
}

priv() {
    if [ "$(id -u)" -ne 0 ]; then
        $SUDO_BIN "$@"
    else
        "$@"
    fi
}

restore_label() {
    path=$1
    if command -v restorecon >/dev/null 2>&1; then
        priv restorecon "$path" || true
    fi
}

chown_if_requested() {
    [ -n "${OWNER:-}" ] || return 0
    priv chown -R "$OWNER:$OWNER" "$@"
}

chmod_user_rw_if_requested() {
    [ -n "${OWNER:-}" ] || return 0
    priv chmod -R u+rwX "$@"
}

update_existing_repo() {
    repo_dir=$1
    ref=$2

    [ -d "$repo_dir/.git" ] || fail "WAKEY_CC_REPO_DIR is not a git checkout: $repo_dir"

    log "using existing checkout $repo_dir (ref: $ref)"
    git -C "$repo_dir" fetch --prune origin "$ref" || fail 'git fetch failed'
    git -C "$repo_dir" checkout "$ref" || fail "git checkout failed: $ref"
    git -C "$repo_dir" pull --ff-only origin "$ref" || fail 'git pull --ff-only failed'
}

install_executable_force() {
    src=$1
    dst=$2
    dir=$(dirname "$dst")
    tmp="$dst.new.$$"
    old="$dst.old.$$"

    priv mkdir -p "$dir"
    priv rm -f "$tmp"
    priv cp -f "$src" "$tmp"
    priv chmod 0755 "$tmp"
    chown_if_requested "$tmp"
    if [ -e "$dst" ]; then
        priv mv -f "$dst" "$old" || true
    fi
    priv mv -f "$tmp" "$dst"
    priv rm -f "$old"
    restore_label "$dst"
}

main() {
    REPO_DIR="${WAKEY_CC_REPO_DIR:-}"
    REPO_URL="${WAKEY_CC_REPO_URL:-https://git.ldlda.com/lda/wakey.git}"
    REF="${WAKEY_CC_REF:-main}"
    TARGET="${WAKEY_CC_TARGET:-$(default_target)}"
    ROOT="${WAKEY_CC_ROOT:-$PWD}"
    SERVICE="${WAKEY_CC_SERVICE:-wakey-cc.service}"
    if [ -n "${WAKEY_CC_USER:-}" ]; then
        OWNER="$WAKEY_CC_USER"
    elif [ "$(id -u)" -eq 0 ]; then
        OWNER=
    else
        OWNER=$(id -un)
    fi
    SUDO_BIN=$(sudo_cmd)

    if [ -n "$OWNER" ] && ! id "$OWNER" >/dev/null 2>&1; then
        fail "WAKEY_CC_USER does not exist: $OWNER"
    fi

    require_cmd git
    require_cmd cargo
    require_cmd tar
    require_cmd pnpm

    TMPBASE="${TMPDIR:-/tmp}"
    WORKDIR="$TMPBASE/wakey-cc-src.$$"
    STAGING="$TMPBASE/wakey-cc-stage.$$"
    CLONED_WORKDIR=1

    cleanup() {
        if [ -z "${WAKEY_CC_KEEP_TMP:-}" ]; then
            [ "$CLONED_WORKDIR" = "0" ] || rm -rf "$WORKDIR"
            rm -rf "$STAGING"
        else
            if [ "$CLONED_WORKDIR" != "0" ]; then
                chown_if_requested "$WORKDIR"
                chmod_user_rw_if_requested "$WORKDIR"
                log "keeping temp dir: $WORKDIR"
            fi
            chown_if_requested "$STAGING"
            chmod_user_rw_if_requested "$STAGING"
            log "keeping temp dir: $STAGING"
        fi
    }
    trap cleanup EXIT INT TERM

    if [ -n "$REPO_DIR" ]; then
        WORKDIR=$REPO_DIR
        CLONED_WORKDIR=0
        update_existing_repo "$WORKDIR" "$REF"
    else
        log "cloning $REPO_URL (ref: $REF)"
        git clone --depth 1 --branch "$REF" "$REPO_URL" "$WORKDIR" ||
            fail 'git clone failed'
    fi

    cd "$WORKDIR"

    log 'building UI dist'
    (
        cd ui
        pnpm install --frozen-lockfile
        pnpm build
    )

    log "building control-plane target $TARGET"
    cargo build --release --target "$TARGET" -p wakey-control-plane ||
        fail 'cargo build failed'

    SHA=$(git rev-parse --short HEAD)
    VERSION="${WAKEY_CC_VERSION:-v0.0.0-local-$SHA}"

    log "packaging bundle version $VERSION"
    ./scripts/package_wakey_cc_bundle.sh --version "$VERSION" --target "$TARGET" ||
        fail 'bundle packaging failed'

    PKG="dist/wakey-cc-$VERSION-$TARGET.tgz"
    [ -f "$PKG" ] || fail "bundle not found: $PKG"

    rm -rf "$STAGING"
    mkdir -p "$STAGING"
    tar -xzf "$PKG" -C "$STAGING" || fail 'bundle extract failed'

    [ -f "$STAGING/bin/wakey-control-plane" ] || fail 'bundle missing bin/wakey-control-plane'
    [ -f "$STAGING/ui/dist/index.html" ] || fail 'bundle missing ui/dist/index.html'

    priv mkdir -p "$ROOT"
    install_executable_force \
        "$STAGING/bin/wakey-control-plane" \
        "$ROOT/bin/wakey-control-plane"
    priv cp -a "$STAGING/ui" "$ROOT/"
    priv cp -a "$STAGING/scripts" "$ROOT/"
    priv cp -a "$STAGING/deploy" "$ROOT/"
    chown_if_requested \
        "$ROOT/bin/wakey-control-plane" \
        "$ROOT/ui" \
        "$ROOT/scripts" \
        "$ROOT/deploy"
    chmod_user_rw_if_requested \
        "$ROOT/ui" \
        "$ROOT/scripts" \
        "$ROOT/deploy"

    if [ -z "${WAKEY_CC_NO_RESTART:-}" ] && command -v systemctl >/dev/null 2>&1; then
        if systemctl list-unit-files "$SERVICE" >/dev/null 2>&1; then
            log "restarting $SERVICE"
            priv systemctl daemon-reload
            priv systemctl restart "$SERVICE"
            priv systemctl --no-pager --full status "$SERVICE" | sed -n '1,16p'
        else
            log "service $SERVICE not installed; skipped restart"
        fi
    fi

    log "done: installed into $ROOT"
}

main "$@"
