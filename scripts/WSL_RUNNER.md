# Fedora / WSL Gitea Runner

This repo now expects Linux/router release builds to run on a Fedora 43 runner
inside WSL, not on the Windows host runner.

The goal is simple:

- build `wakey` and `wakey-agent` in a Linux-native environment
- stop fighting Windows-host ARM/musl/TLS toolchain issues
- keep PowerShell on Windows as the operator front door

## Runner layout

Recommended WSL location:

```text
/mnt/c/Users/Admin/Documents/realshit/wakey/ar_data/wsl_runner
```

This matches the default used by `act_runner_wsl.ps1` and
`act_runner_fedora.sh`, so WSL runner config is isolated at
`ar_data/wsl_runner/config.yaml` in this repo.

Recommended labels:

```text
self-hosted,linux,fedora,wsl,release:host
```

The Windows runner can stay registered during migration as fallback.

## Fedora setup

Install the basics in Fedora 43:

```bash
sudo dnf install -y git curl tar gzip findutils python3
sudo dnf install -y powershell
```

Install Rust and the router target:

```bash
rustup target add armv7-unknown-linux-musleabihf
```

Install the native cross toolchain pieces you need for Linux/ARM router builds.
The exact package names can vary over time, so confirm the current Fedora names
for:

- ARM Linux GCC
- musl development/toolchain support
- OpenSSL/ring/TLS build prerequisites if needed by your dependency graph

## Runner lifecycle

From Windows PowerShell, using the WSL wrapper:

```powershell
./scripts/act_runner_wsl.ps1 -Action update
./scripts/act_runner_wsl.ps1 -Action register -ServerUrl https://git.ldlda.com/ -Token <runner-token>
./scripts/act_runner_wsl.ps1 -Action start -Attach
./scripts/act_runner_wsl.ps1 -Action start -Once
./scripts/act_runner_wsl.ps1 -Action status
./scripts/act_runner_wsl.ps1 -Action stop
```

Path control (binary and config are independent):

```powershell
./scripts/act_runner_wsl.ps1 -Action status `
	-RunnerBin /home/lda/gitea-runner/act_runner `
	-ConfigPath /mnt/c/Users/Admin/Documents/realshit/wakey/ar_data/wsl_runner/config.yaml
```

If your distro name is not `FedoraLinux-43`, set it explicitly:

```powershell
./scripts/act_runner_wsl.ps1 -Distro FedoraLinux-43 -Action status
```

Or set:

```powershell
$env:WAKEY_WSL_DISTRO = "FedoraLinux-43"
```

Inside Fedora directly, you can also use:

```bash
./scripts/act_runner_fedora.sh update
./scripts/act_runner_fedora.sh register --server-url https://git.ldlda.com/ --token <runner-token>
./scripts/act_runner_fedora.sh start --attach
./scripts/act_runner_fedora.sh start --once
./scripts/act_runner_fedora.sh status
./scripts/act_runner_fedora.sh stop
```

## Release flow

The release workflow is expected to run on the Fedora/WSL runner labels:

```text
self-hosted + linux + fedora + wsl + release
```

It builds:

- `wakey`
- `wakey-agent`

and packages both into the rootfs tarball.

From Windows, the intended operator flow remains:

```powershell
./scripts/publish.ps1 -Tag
```

That script should trigger the WSL runner in `--once` mode by default.

## Rollback

If the Fedora runner path is broken:

1. stop using the WSL runner labels in the workflow
2. temporarily point the workflow back to the Windows runner labels
3. use the existing Windows runner as fallback while fixing the Fedora path

Do not delete the Windows runner until the Fedora path has already produced at
least one successful full release and one successful router install.
