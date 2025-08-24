#!/bin/sh
# Update/install wakey on OpenWrt by fetching a rootfs tarball and extracting to /
# Supports either a direct URL or auto-detecting the latest release from a Gitea instance.
#
# Env overrides:
#   WAKEY_TGZ_URL      Direct URL to wakey-rootfs-<ver>-<arch>.tgz
#   WAKEY_HOST         Gitea host, e.g. git.ldlda.com
#   WAKEY_OWNER        Repo owner, e.g. lda
#   WAKEY_REPO         Repo name, e.g. wakey
#   WAKEY_ARCH         Target triplet in filename (default: armv7-unknown-linux-musleabihf)
#   WAKEY_VERSION      Tag name (e.g., v0.1.0). If omitted, fetch latest via API.
#   WAKEY_INSECURE     If set (non-empty), pass --no-check-certificate to wget
#
# Requires: wget (or uclient-fetch), tar

set -eu

ARCH=${WAKEY_ARCH:-armv7-unknown-linux-musleabihf}
TMPFILE="/tmp/wakey-rootfs.$$.$ARCH.tgz"
API_TOKEN="${WAKEY_TOKEN:-}"

log() { echo "[update_wakey] $*"; }
fail() { echo "[update_wakey] ERROR: $*" >&2; exit 1; }

fetch() {
	# fetch <url> <out>
	if command -v uclient-fetch >/dev/null 2>&1; then
		uclient-fetch -O "$2" "$1" || return 1
	elif command -v wget >/dev/null 2>&1; then
		# shellcheck disable=SC2086
		wget ${WAKEY_INSECURE:+--no-check-certificate} -O "$2" "$1" || return 1
	else
		return 2
	fi
}

latest_asset_url() {
	# prints URL to stdout, returns 0 on success
	HOST=$1 OWNER=$2 REPO=$3 ARCH=$4 TOKEN=$5
	API="https://$HOST/api/v1/repos/$OWNER/$REPO/releases/latest"
		if command -v curl >/dev/null 2>&1; then
			if [ -n "$TOKEN" ]; then
				RESP=$(curl -fsSL -H "Authorization: token $TOKEN" "$API") || return 1
			else
				RESP=$(curl -fsSL "$API") || return 1
			fi
		elif command -v wget >/dev/null 2>&1; then
		# shellcheck disable=SC2086
		RESP=$(wget ${WAKEY_INSECURE:+--no-check-certificate} -qO- "$API") || return 1
	else
		return 2
	fi
	# crude parse for browser_download_url matching arch; avoids jq dependency
	echo "$RESP" | sed -n "s/.*\"browser_download_url\"\s*:\s*\"\([^\"]*${ARCH}\.tgz\)\".*/\1/p" | head -n1
}

main() {
	URL="${WAKEY_TGZ_URL:-}"
	if [ -z "$URL" ]; then
		HOST=${WAKEY_HOST:-git.ldlda.com}
		OWNER=${WAKEY_OWNER:-lda}
		REPO=${WAKEY_REPO:-wakey}
		if [ -n "${WAKEY_VERSION:-}" ]; then
			FILE="wakey-rootfs-${WAKEY_VERSION}-${ARCH}.tgz"
			URL="https://$HOST/$OWNER/$REPO/releases/download/${WAKEY_VERSION}/$FILE"
		else
			URL=$(latest_asset_url "$HOST" "$OWNER" "$REPO" "$ARCH" "$API_TOKEN") || fail "unable to resolve latest asset URL"
			[ -n "$URL" ] || fail "no asset URL found for arch $ARCH"
		fi
	fi

	log "fetching $URL"
	if ! fetch "$URL" "$TMPFILE"; then
		fail "download failed"
	fi

	log "installing"
	tar -xz -f "$TMPFILE" -C / || fail "extract failed"
	rm -f "$TMPFILE"

	if [ -f /etc/init.d/wakey ]; then
		chmod +x /etc/init.d/wakey || true
		/etc/init.d/wakey enable || true
		/etc/init.d/wakey restart || /etc/init.d/wakey start || true
	fi
	log "done"
}

main "$@"
