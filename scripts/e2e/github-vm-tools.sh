#!/usr/bin/env bash
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]]
case "$1" in
  x64) packages=(qemu-system-x86 ovmf) ;;
  arm64) packages=(qemu-system-arm qemu-efi-aarch64) ;;
  *) exit 2 ;;
esac
sudo apt-get update
sudo apt-get install -y --no-install-recommends "${packages[@]}" qemu-utils cloud-image-utils acl
# Some hosted images expose KVM without granting the runner user access.
if [[ -c /dev/kvm ]]; then sudo setfacl -m "u:$(id -u):rw" /dev/kvm; fi
