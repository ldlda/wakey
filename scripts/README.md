# Scripts

This folder contains helper scripts for building, packaging, releasing, and deploying `wakey`.

## Overview

- `act_runner.ps1` — Starts your local Gitea runner as a background daemon on Windows.
- `cross_build.ps1` — Cross-compiles the project using `cross` for one or more Linux targets.
- `package.ps1` — Builds a developer-friendly tarball/zip containing binaries and init scripts.
- `package_rootfs.ps1` — Builds a router-ready rootfs tarball that can be installed via `wget | tar`.
- `release.ps1` — One-shot helper to cross build and package.
- `publish.ps1` — Optional: bump version in Cargo.toml, build, `cargo publish`, and tag the repo.
- `init/openwrt/*` — OpenWrt init scripts (procd) to run `wakey` and other helpers.
- `systemd/wakey.service` — A basic systemd unit for Linux hosts.

## act_runner.ps1

Starts the Gitea `act_runner` if it isn't already running.

- Default paths assume `~/Documents/gitea/act_runner.exe` and config at `~/Documents/gitea/.runner`.
- Usage:

```powershell
./scripts/act_runner.ps1
# or specify custom paths
./scripts/act_runner.ps1 -RunnerPath "C:/Users/Admin/Documents/gitea/act_runner.exe" -Config "C:/Users/Admin/Documents/gitea/.runner"
```

## cross_build.ps1

Cross-compile with `cross` for the targets you need.

- Requires `cross` installed (`cargo install cross`).
- Usage:

```powershell
./scripts/cross_build.ps1 -Release -Targets armv7-unknown-linux-musleabihf
```

## package.ps1

Create a developer bundle with binaries and init scripts. Doesn’t layout files for `/` directly.

```powershell
./scripts/package.ps1 -Version 0.1.0 -Targets armv7-unknown-linux-musleabihf
```

Artifacts in `dist/wakey-<version>.(tgz|zip)`.

## package_rootfs.ps1

Create a router-ready rootfs tarball that installs with a one-liner.

```powershell
./scripts/package_rootfs.ps1 -Version 0.1.0 -Target armv7-unknown-linux-musleabihf
```

This writes `dist/wakey-rootfs-<version>-<target>.tgz` containing:

- `root/.bin/wakey`
- `etc/init.d/wakey` (and any other scripts under `scripts/init/openwrt`)

Install on OpenWrt:

```sh
wget -O- <URL>/wakey-rootfs-<version>-<target>.tgz | tar -xz -C /
chmod +x /etc/init.d/wakey
/etc/init.d/wakey enable
/etc/init.d/wakey start
```

## release.ps1

One-shot:

```powershell
./scripts/release.ps1 -Version 0.1.0 -Targets armv7-unknown-linux-musleabihf
```

## publish.ps1

Optional helper to bump the version, build release, publish the crate, and tag the repo.

```powershell
./scripts/publish.ps1 -Version 0.1.0 -Tag
```

- If you’re not publishing to crates.io, the `cargo publish` step will warn and continue.
- Uses a simple regex replace to update the `version = "…"` line in `Cargo.toml`.

## CI workflow (Gitea)

A workflow in `.gitea/workflows/release.yml` is provided:

- Triggers on tag pushes (v\*) and manual dispatch.
- Builds with `cross` on your self-hosted Windows runner.
- Packages a rootfs tarball.
- Publishes a Gitea release via API, attaching the tarball (requires `secrets.GITEA_TOKEN`).

Once published, you can install on the router with:

```sh
wget -O- <release-asset-URL> | tar -xz -C /
```

Adjust paths and targets as needed for your environment.
