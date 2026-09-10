#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Linux && $(id -u) != 0 ]] || exit 2
workspace=${1:?Expected immutable source directory}
fixture=${2:?Expected owned fixture directory}
ppm=${3:?Expected deliberate sender ppm}
seconds=${4:-90}
disable=${5:-false}
quantum=${MIXRELAY_LAB_QUANTUM:-128}
[[ "$quantum" == 128 || "$quantum" == 1024 ]] || exit 2
rt_module=''
if [[ ${MIXRELAY_LAB_RT_ENABLED:-false} == true ]]; then
    rt_module='{ name = libpipewire-module-rt args = { rt.prio = 20 rt.time.soft = 200000 rt.time.hard = 2000000 } flags = [ ifexists nofail ] }'
fi
[[ "$workspace" == /*/.lab/mixrelay-integration-* && -f "$workspace/Cargo.toml" ]] || exit 2
[[ "$fixture" == /*/.lab/mixrelay-integration-* && -f "$fixture/MRDriftSender.c" ]] || exit 2
[[ "$ppm" =~ ^-?[0-9]+$ && "$seconds" =~ ^[0-9]+$ ]] || exit 2
(( ppm >= -10000 && ppm <= 10000 && seconds >= 5 && seconds <= 600 )) || exit 2
[[ "$disable" == true || "$disable" == false ]] || exit 2
wireplumber --version | grep -q '0\.4\.' || exit 2
port=49178
[[ -z $(ss -H -lun "sport = :$port") ]] || { echo 'Owned RTP port is occupied'; exit 2; }
mkdir -p "$workspace/target/native-tests"
lab=$(mktemp -d "$workspace/target/native-tests/drift-lab-XXXXXX")
printf 'isolated_lab=%s ppm=%s seconds=%s resample.disable=%s quantum=%s\n' "$lab" "$ppm" "$seconds" "$disable" "$quantum"
mkdir -p "$lab/runtime" "$lab/config" "$lab/state" "$lab/data"
chmod 700 "$lab/runtime"
cp -R /usr/share/wireplumber "$lab/config/wireplumber"
cp "$workspace/native/linux/pipewire/tests/application-lab-wireplumber.conf" "$lab/config/wireplumber/wireplumber.conf"
cp "$workspace/native/linux/pipewire/tests/85-mixrelay-lab.lua" "$lab/config/wireplumber/main.lua.d/85-mixrelay-lab.lua"
sed -e "s/@PORT@/$port/" -e "s/@DISABLE@/$disable/" -e "s/@QUANTUM@/$quantum/g" -e "s/@RT_MODULE@/$rt_module/" "$fixture/drift-lab-pipewire.conf" > "$lab/pipewire.conf"
export XDG_RUNTIME_DIR="$lab/runtime" XDG_CONFIG_HOME="$lab/config" XDG_STATE_HOME="$lab/state" XDG_DATA_HOME="$lab/data"
export WIREPLUMBER_CONFIG_DIR="$lab/config/wireplumber"
unset PIPEWIRE_REMOTE PIPEWIRE_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS
cc -O2 -Wall -Wextra -Werror "$fixture/MRDriftSender.c" -lm -o "$lab/drift-sender"
format_api=()
if grep -q 'uint32_t sample_rate, uint32_t block_frames' "$workspace/native/linux/pipewire/MRPipeWire.h"; then format_api=(-DTN_DRIFT_FORMAT_API); fi
native_source="$workspace/native/linux/pipewire/MRPipeWire.c"
if [[ ${MIXRELAY_LAB_TRACE_ENABLED:-false} == true ]]; then
    format_api+=(-DTN_DRIFT_TRACE)
    native_source="$fixture/MRDriftTrace.c"
    export MIXRELAY_LAB_DRIFT_TRACE="$lab/native-trace.csv"
fi
cargo build --manifest-path "$workspace/Cargo.toml" -p mixrelay-pcm-ring --release --target-dir "$workspace/target"
cc -O2 -Wall -Wextra -Werror -D_GNU_SOURCE "${format_api[@]}" -I"$workspace/native/linux/pipewire" -I"$workspace/native/pcm-ring/include" \
    "$fixture/MRDriftProbe.c" "$native_source" "$workspace/target/release/libmixrelay_pcm_ring.a" -ldl \
    $(pkg-config --cflags --libs libpipewire-0.3) -lm -pthread -o "$lab/drift-probe"
pipewire_pid='' policy_pid='' bus_pid='' sender_pid='' probe_pid=''
cleanup() {
    for owned_pid in "$probe_pid" "$sender_pid" "$policy_pid" "$bus_pid" "$pipewire_pid"; do
        if [[ -n "$owned_pid" ]]; then kill -TERM "$owned_pid" 2>/dev/null || true; wait "$owned_pid" 2>/dev/null || true; fi
    done
    printf 'owned_processes_reaped; evidence=%s\n' "$lab"
}
trap cleanup EXIT
PIPEWIRE_DEBUG='2,default:4,mod.rtp-source:4' pipewire -c "$lab/pipewire.conf" >"$lab/pipewire.log" 2>&1 &
pipewire_pid=$!
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
serial=$(python3 -c 'import json,sys; a=json.load(sys.stdin); print(next(n["info"]["props"]["object.serial"] for n in a if n.get("type")=="PipeWire:Interface:Node" and n.get("info",{}).get("props",{}).get("node.name")=="io.mixrelay.lab-drift-rtp"))' < "$lab/before.json")
warmup=40
(( seconds <= 25 )) && warmup=1
"$lab/drift-probe" "$serial" "$seconds" "$warmup" >"$lab/probe.json" 2>"$lab/probe.log" &
probe_pid=$!
"$lab/drift-sender" "$port" "$ppm" "$seconds" >"$lab/sender.json" 2>"$lab/sender.log" &
sender_pid=$!
ps -L -p "$pipewire_pid,$probe_pid" -o pid,tid,cls,rtprio,comm >"$lab/thread-policy.txt"
probe_result=0
wait "$probe_pid" || probe_result=$?
probe_pid=''
wait "$sender_pid"; sender_pid=''
pw-dump >"$lab/after.json"
python3 -c 'import json,sys; print(json.dumps({"sender":json.load(open(sys.argv[1])),"probe":json.load(open(sys.argv[2]))},sort_keys=True))' "$lab/sender.json" "$lab/probe.json"
printf 'probe_exit_code=%s\n' "$probe_result"
exit "$probe_result"
