#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Linux && $(id -u) != 0 ]] || exit 2
workspace=${1:?Expected isolated source directory}
graph_rate=${TONENARI_LAB_GRAPH_RATE:-48000}
[[ "$graph_rate" == 44100 || "$graph_rate" == 48000 ]] || exit 2
[[ "$workspace" == /*/.lab/tonenari-integration-* && -f "$workspace/Cargo.toml" ]] || exit 2
wireplumber --version | grep -q '0\.4\.' || { echo 'This fixture config requires WirePlumber 0.4'; exit 2; }
mkdir -p "$workspace/target/native-tests"
lab=$(mktemp -d "$workspace/target/native-tests/application-lab-XXXXXX")
printf 'isolated_lab=%s\n' "$lab"
mkdir -p "$lab/runtime" "$lab/config" "$lab/state" "$lab/data"
chmod 700 "$lab/runtime"
cp -R /usr/share/wireplumber "$lab/config/wireplumber"
fixture=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cp "$fixture/application-lab-wireplumber.conf" "$lab/config/wireplumber/wireplumber.conf"
cp "$fixture/85-tonenari-lab.lua" "$lab/config/wireplumber/main.lua.d/85-tonenari-lab.lua"
cp "$fixture/application-lab-pipewire.conf" "$lab/pipewire.conf"
sed -i "s/default.clock.rate = 48000/default.clock.rate = $graph_rate/" "$lab/pipewire.conf"
export XDG_RUNTIME_DIR="$lab/runtime" XDG_CONFIG_HOME="$lab/config" XDG_STATE_HOME="$lab/state" XDG_DATA_HOME="$lab/data"
export WIREPLUMBER_CONFIG_DIR="$lab/config/wireplumber"
export PATH="/home/state303/.cargo/bin:$PATH" CARGO_TARGET_DIR=/home/state303/tonenari/target
unset PIPEWIRE_REMOTE PIPEWIRE_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS
cd "$workspace"
cc -Wall -Wextra -Werror "$fixture/TNApplicationFixture.c" $(pkg-config --cflags --libs libpipewire-0.3) -lm -o "$lab/application-fixture"
pipewire -c "$lab/pipewire.conf" >"$lab/pipewire.log" 2>&1 &
pipewire_pid=$!
policy_pid=''
bus_pid=''
cleanup() {
    if [[ -n "$policy_pid" ]]; then kill -TERM "$policy_pid" 2>/dev/null || true; wait "$policy_pid" || true; fi
    if [[ -n "$bus_pid" ]]; then kill -TERM "$bus_pid" 2>/dev/null || true; wait "$bus_pid" || true; fi
    kill -TERM "$pipewire_pid" 2>/dev/null || true
    wait "$pipewire_pid" || true
    # Preserve exact owned temporary config/logs as evidence; no broad cleanup.
    printf 'owned_processes_reaped; evidence=%s\n' "$lab"
}
trap cleanup EXIT
for _ in {1..100}; do [[ -S "$lab/runtime/pipewire-0" ]] && break; sleep 0.02; done
[[ -S "$lab/runtime/pipewire-0" ]] || exit 1
dbus-daemon --session --nofork --print-address=1 >"$lab/dbus-address" 2>"$lab/dbus.log" &
bus_pid=$!
for _ in {1..100}; do [[ -s "$lab/dbus-address" ]] && break; sleep 0.02; done
IFS= read -r DBUS_SESSION_BUS_ADDRESS < "$lab/dbus-address"
export DBUS_SESSION_BUS_ADDRESS
wireplumber >"$lab/wireplumber.log" 2>&1 &
policy_pid=$!
sleep 1
pw-dump >"$lab/before.json"
test_name=${TONENARI_LAB_TEST_NAME:-native_application_stream_relaunch_recovers_pcm_without_graph_reapply}
if [[ ${TONENARI_LAB_LIFECYCLE_TEST:-0} == 1 ]]; then
    cc -O2 -std=c11 -D_GNU_SOURCE -Wall -Wextra -Werror -pthread \
        $(pkg-config --cflags libpipewire-0.3) -I "$workspace/native/linux/pipewire" \
        "$workspace/native/linux/pipewire/tests/TNLifecycleFixture.c" \
        "$workspace/native/linux/pipewire/TNPcmRing.c" \
        $(pkg-config --libs libpipewire-0.3) -lm -o "$lab/lifecycle-fixture"
    "$lab/lifecycle-fixture"
else
    TONENARI_LAB_APPLICATION_FIXTURE="$lab/application-fixture" cargo test --locked -p tonenari-engine-service --bin tonenari-engine-service "$test_name" -- --ignored --nocapture
fi
pw-dump >"$lab/after.json"
