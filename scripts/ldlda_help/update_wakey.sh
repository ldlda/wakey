#!/bin/sh
# Update/install wakey on OpenWrt by fetching a rootfs tarball and extracting to /
# Simple defaults, BusyBox-friendly.
#
# Env options:
#   WAKEY_TGZ_URL   Direct URL to wakey-rootfs-<ver>-<arch>.tgz (preferred if known)
#   WAKEY_HOST      Gitea host (default: git.ldlda.com)
#   WAKEY_OWNER     Repo owner (default: lda)
#   WAKEY_REPO      Repo name  (default: wakey)
#   WAKEY_ARCH      Target triplet in filename (default: armv7-unknown-linux-musleabihf)
#   WAKEY_VERSION   Tag name (e.g., v0.1.0). If omitted, resolve latest via API.
#   WAKEY_INSECURE  If set, disable TLS verify for wget/curl
#
# Requires: uclient-fetch or wget (curl optional), tar

set -eu

ARCH=${WAKEY_ARCH:-armv7-unknown-linux-musleabihf}
TMPDIR="/var/tmp"
TMPFILE="$TMPDIR/wakey-rootfs.$$.$ARCH.tgz"
STAGING="$TMPDIR/wakey-rootfs.$$.$ARCH"

log() { echo "[update_wakey] $*"; }
fail() { echo "[update_wakey] ERROR: $*" >&2; exit 1; }

http_get() {
	# http_get <url>
	if command -v uclient-fetch >/dev/null 2>&1; then
		# uclient-fetch supports -O -, write to stdout
		uclient-fetch -O - "$1" 2>/dev/null || return 1
	elif command -v wget >/dev/null 2>&1; then
		# shellcheck disable=SC2086
		wget ${WAKEY_INSECURE:+--no-check-certificate} -qO- "$1" || return 1
	elif command -v curl >/dev/null 2>&1; then
		if [ -n "${WAKEY_INSECURE:-}" ]; then
			curl -fsSL -k "$1" || return 1
		else
			curl -fsSL "$1" || return 1
		fi
	else
		return 2
	fi
}

fetch() {
	# fetch <url> <out>
	if command -v uclient-fetch >/dev/null 2>&1; then
		uclient-fetch -O "$2" "$1" || return 1
	elif command -v wget >/dev/null 2>&1; then
		wget ${WAKEY_INSECURE:+--no-check-certificate} -O "$2" "$1" || return 1
	elif command -v curl >/dev/null 2>&1; then
		if [ -n "${WAKEY_INSECURE:-}" ]; then
			curl -fSL -k -o "$2" "$1" || return 1
		else
			curl -fSL -o "$2" "$1" || return 1
		fi
	else
		return 2
	fi
}

latest_asset_url() {
	# prints URL to stdout, returns 0 on success (public release only)
	HOST=$1 OWNER=$2 REPO=$3 ARCH=$4
	API="https://$HOST/api/v1/repos/$OWNER/$REPO/releases/latest"
	RESP=$(http_get "$API") || return 1
	# Extract the first browser_download_url ending with our ARCH .tgz (BusyBox-friendly)
	echo "$RESP" \
	| grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*'"$ARCH"'\.tgz"' \
	| head -n1 \
	| cut -d '"' -f 4
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
			URL=$(latest_asset_url "$HOST" "$OWNER" "$REPO" "$ARCH") || fail "unable to resolve latest asset URL"
			[ -n "$URL" ] || fail "no asset URL found for arch $ARCH"
		fi
	fi

	log "fetching $URL"
	if ! fetch "$URL" "$TMPFILE"; then
		fail "download failed"
	fi

	log "installing"
	mkdir -p "$TMPDIR" "$STAGING"
	tar -xz -f "$TMPFILE" -C "$STAGING" || fail "extract failed"

	# Ensure execute bits on staged files we know should be executable
	for f in \
		"$STAGING/etc/init.d/"* \
		"$STAGING/etc/ldlda_help/"*.sh \
		"$STAGING/root/.bin/wakey" \
		"$STAGING/root/.bin/kill_wakey.sh"; do
		[ -e "$f" ] && chmod +x "$f" 2>/dev/null || true
	done

	# Normalize line endings for shell scripts (avoid CRLF issues on OpenWrt)
	for f in \
		"$STAGING/etc/init.d/"* \
		"$STAGING/etc/ldlda_help/"*.sh; do
		[ -f "$f" ] && sed -i 's/\r$//' "$f" 2>/dev/null || true
	done

	# Copy staged tree into /
	tar -C "$STAGING" -cf - . | tar -C / -xpf - || fail "install copy failed"

	rm -f "$TMPFILE"
	rm -rf "$STAGING"

	if [ -f /etc/init.d/wakey ]; then
		/etc/init.d/wakey enable || true
		/etc/init.d/wakey restart || /etc/init.d/wakey start || true
	fi
	log "done"
}

main "$@"
