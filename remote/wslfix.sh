#!/usr/bin/env bash
set -euo pipefail

# sometimes it breaks, run to fix

sudo sh -c 'echo :WSLInterop:M::MZ::/init:PF > /usr/lib/binfmt.d/WSLInterop.conf'
sudo systemctl restart systemd-binfmt
