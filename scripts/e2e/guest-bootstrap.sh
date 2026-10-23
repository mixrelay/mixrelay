#!/usr/bin/env bash
set -euo pipefail

# Run only inside the disposable cloud guest, before installing the candidate.
source /etc/os-release
if [[ "$ID" == arch ]]; then
  pacman -Syu --noconfirm --needed xorg-server-xvfb dbus pipewire pipewire-audio \
    pipewire-pulse wireplumber gnome-keyring libsecret libarchive openbox sudo
else
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends xvfb dbus-x11 dbus-user-session pipewire pipewire-bin \
    pipewire-pulse wireplumber gnome-keyring libsecret-tools libarchive-tools openbox sudo
fi
loginctl enable-linger qa
install -d -m 0700 -o qa -g qa /home/qa/.config /home/qa/.config/systemd /home/qa/.config/systemd/user
sudo -u qa test -w /home/qa/.config
stat -c '%U:%G %a %n' /home/qa/.config /home/qa/.config/systemd /home/qa/.config/systemd/user
cat > /home/qa/.config/systemd/user/mixrelay-qa-display.service <<'UNIT'
[Unit]
Description=Disposable qualification X11 display
[Service]
ExecStart=/usr/bin/Xvfb :99 -screen 0 1440x900x24 -nolisten tcp
[Install]
WantedBy=default.target
UNIT
chown qa:qa /home/qa/.config/systemd/user/mixrelay-qa-display.service
