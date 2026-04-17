# Scripts

This folder has small helpers for build, CI, and router install. Keep it simple; use only what you need.

## Quick start (recommended)

1. Start your Fedora/WSL Gitea runner (recommended release path):

   ```powershell
   ./scripts/act_runner_wsl.ps1 -Action start -Attach
   ```

   If first-time, use the runbook in [`WSL_RUNNER.md`](./WSL_RUNNER.md) to update and register the Fedora runner first.

2. Publish, tag, and push (recommended):

   ```powershell
   ./scripts/publish.ps1 -Tag
   ```

   This will bump the version (if you specify one), build locally, tag, push, and start the Fedora/WSL runner in `--once` mode by default. To also publish to the registry, add `-Publish`.

3. Install on the router

   Use the updater on the router to fetch the latest automatically:

   ```sh
   WAKEY_HOST=git.ldlda.com WAKEY_OWNER=lda WAKEY_REPO=wakey sh /etc/ldlda_help/update_wakey.sh
   ```

   Alternatively, use the release asset URL from your Gitea Release page:

   ```sh
   wget -O- https://<gitea>/<owner>/<repo>/releases/download/v0.1.0/wakey-rootfs-v0.1.0-armv7-unknown-linux-musleabihf.tgz | tar -xz -C /
   chmod +x /etc/init.d/wakey && /etc/init.d/wakey enable && /etc/init.d/wakey restart
   ```

## Scripts overview

- `act_runner.ps1` — Start/seed the local Gitea runner. Use `-Attach` to see logs, `-ForceConfigure` to register non-interactively.
- `act_runner_wsl.ps1` — Windows PowerShell wrapper that controls the Fedora/WSL runner.
- `act_runner_fedora.sh` — Linux/Fedora helper used inside WSL for update/register/start/stop/status.
- `dev_push.ps1` — Fast dev loop: build + upload to router. Usage:
  - `./scripts/dev_push.ps1 -Pass <password> [-HostName <ip>] [-RemotePath </root/.bin/wakey>] [-Restart] [-Quiet]`
  - Uploads to `<RemotePath>.tmp` then atomically moves into place; `-Restart` restarts the service; `-Quiet` silences MOTD.
- `package_rootfs.ps1` — Produces `dist/wakey-rootfs-<version>-<target>.tgz` with `/root/.bin/wakey` and `/etc/init.d/*`.
- `update_wakey_cc.sh` — Linux VPS updater for control-plane bundles; extracts into the selected directory (default: current directory) and restarts `wakey-cc.service`.
- `update_wakey_cc_from_repo.sh` — Linux VPS source updater; clones repo to temp dir, builds UI + control-plane locally, packages bundle tgz, installs into selected directory, and restarts `wakey-cc.service`.
- `package_wakey_cc_bundle.sh` — Produces `dist/wakey-cc-<version>-<target>.tgz` with `bin/wakey-control-plane`, `ui/dist/*`, updater script, and systemd template.
- `publish.ps1` — Optional version bump + build + tag (and `cargo publish` only if you pass `-Publish`).
- `test_remote.ps1` — Build ARM Linux test binaries locally, upload them to the router, and run them there.

## VPS updater (control-plane)

```sh
chmod +x ./scripts/update_wakey_cc.sh
cd /opt/wakey
sudo -E ./scripts/update_wakey_cc.sh

# optional: pin version/target
WAKEY_CC_VERSION=v0.2.0 WAKEY_CC_TARGET=x86_64-unknown-linux-gnu \
  sudo -E ./scripts/update_wakey_cc.sh
```

## VPS source updater (control-plane)

Build-and-install locally on the VPS (avoids glibc mismatch from prebuilt binaries):

```sh
chmod +x ./scripts/update_wakey_cc_from_repo.sh
cd /opt/wakey
sudo -E /path/to/repo/scripts/update_wakey_cc_from_repo.sh

# optional: pin ref/target and skip restart
WAKEY_CC_REF=main WAKEY_CC_TARGET=x86_64-unknown-linux-gnu WAKEY_CC_NO_RESTART=1 \
  sudo -E /path/to/repo/scripts/update_wakey_cc_from_repo.sh
```

Expected tarball layout:

- `bin/wakey-control-plane`
- `ui/dist/index.html`
- `ui/dist/assets/*`
- `scripts/update_wakey_cc.sh`
- `deploy/systemd/wakey-cc.service`

## Remote test runner

`test_remote.ps1` is the main way to run Linux/router-specific tests that do not
make sense on Windows.

Basic examples:

```powershell
./scripts/test_remote.ps1 -Package wakey
./scripts/test_remote.ps1 -Package wakey -BinaryFilter integration_live_services -Ignored -NoCapture
./scripts/test_remote.ps1 -Package wakey -BinaryFilter integration_live_services -Filter inventory_real_router_default_query_returns_rows_or_empty_cleanly -Ignored -NoCapture
./scripts/test_remote.ps1 -AllPackages
```

Important semantics:

- `-BinaryFilter`
  - selects which compiled Rust test executable(s) to run
- `-Filter`
  - filters test functions inside a selected Rust test binary
- `-Ignored`
  - runs `#[ignore]` tests
- `-IncludeIgnored`
  - includes ignored tests in addition to normal ones
- `-List`
  - lists tests inside the remote binary instead of executing them

Useful flags:

- `-BuildProfile debug|release`
- `-Exact`
- `-NoCapture`
- `-ShowOutput`
- `-Threads <n>`
- `-RemoteHost root@<router-ip>`
- `-RemoteTestPath /tmp/tmp/wakey-test`

Implementation notes:

- test executables are discovered via Cargo JSON messages, not regexing human output
- binaries are batch-uploaded per package run
- each package run gets a unique remote temp directory for safer concurrent use
- packages with no test executables are skipped instead of treated as failures

## Release checklist (tag push)

1. **Version:** bump crate versions / changelog as you prefer; tag `v*` and push.
2. **Runner:** Fedora/WSL self-hosted labels `self-hosted`, `linux`, `fedora`, `wsl`, `release` online (see [`WSL_RUNNER.md`](./WSL_RUNNER.md)).
3. **Control-plane bundle:** release job builds `ui/dist`, then `wakey-control-plane` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. With **zig** + **cargo-zigbuild** on the runner, gnu builds use workflow env `WAKEY_CC_GLIBC_VERSION` (default `2.28`) for a predictable glibc floor; without them, binaries match the host glibc.
4. **Artifacts:** expect `dist/wakey-rootfs-*-armv7-unknown-linux-musleabihf.tgz` and `dist/wakey-cc-*-unknown-linux-gnu.tgz` attached to the Gitea release.
5. **Secret:** `GITEA_TOKEN` must be set for the publish step.

## CI (Gitea)

- Defined in `.gitea/workflows/release.yml` (tag / manual dispatch): router rootfs + control-plane bundles and Gitea release attach.
- Release job targets your Fedora/WSL runner labels: `self-hosted`, `linux`, `fedora`, `wsl`, `release`.
- Requires `secrets.GITEA_TOKEN` in the repo to publish the release.

## CI (GitHub)

- Defined in `.github/workflows/ci.yml` on pushes/PRs to `main` or `master`:
  - **`rust` job:** `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` on `ubuntu-latest`.
  - **`ui` job:** under `ui/`, `pnpm install --frozen-lockfile`, then `pnpm run typecheck`, `format:check`, and `build` (mirrors what you need before packaging the control-plane bundle). GitHub sets `CI=true` for the runner; pnpm uses lockfile `ui/pnpm-lock.yaml` for cache.

## Runner migration

- Fedora/WSL is now the intended release-builder path.
- The Windows runner can stay registered in parallel during migration and fallback.
- The detailed operator runbook is in [`WSL_RUNNER.md`](./WSL_RUNNER.md).

## Local build/install (manual path)

```powershell
# Build
cargo build --release --target armv7-unknown-linux-musleabihf

# Package rootfs
./scripts/package_rootfs.ps1 -Version v0.1.0 -Target armv7-unknown-linux-musleabihf

# Copy to router (PowerShell)
scp -O .\dist\wakey-rootfs-v0.1.0-armv7-unknown-linux-musleabihf.tgz root@<router-ip>:/tmp/wakey.tgz
```

```sh
# On the router
tar -xz -f /tmp/wakey.tgz -C /
chmod +x /etc/init.d/wakey && /etc/init.d/wakey enable && /etc/init.d/wakey restart
```

Or you can use the script:

```powershell
./scripts/dev_push.ps1 -Pass <password>
```

That’s it. Keep the flow: start Fedora/WSL runner → tag push → install.
