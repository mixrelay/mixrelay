#!/bin/zsh
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "MixRelay dynamic graph smoke only runs on macOS"
  exit 1
fi

repo_root="${0:A:h:h}"
control_origin="http://127.0.0.1:19341"
gain_active=false
restore_payload=""
restore_revision=""

json_revision() {
  node -e 'let value=""; process.stdin.on("data", chunk => value += chunk); process.stdin.on("end", () => process.stdout.write(String(JSON.parse(value).revision)));'
}

runtime_active_revision() {
  curl -fsS "$control_origin/api/v1/graph/status" |
    node -e 'let value=""; process.stdin.on("data", chunk => value += chunk); process.stdin.on("end", () => process.stdout.write(String(JSON.parse(value).activeRevision)));'
}

wait_for_revision() {
  local expected="$1"
  for attempt in {1..40}; do
    if [[ "$(runtime_active_revision)" == "$expected" ]]; then
      return 0
    fi
    sleep 0.05
  done
  print -u2 "Graph revision $expected did not become active"
  return 1
}

graph_with_gain() {
  local revision="$1"
  node -e '
    let value = "";
    process.stdin.on("data", chunk => value += chunk);
    process.stdin.on("end", () => {
      const graph = JSON.parse(value);
      const endpoint = graph.nodes.find(node => node.id === "default-virtual-io");
      if (!endpoint) process.exit(2);
      graph.revision = Number(process.argv[1]);
      graph.nodes = [endpoint, {
        id: "output-gain",
        label: "Output gain",
        type: "gain",
        config: { channels: 2, gainDb: -6.0206003 }
      }];
      graph.edges = [
        {
          id: "from-os-to-gain",
          source: { nodeId: endpoint.id, portId: "from_os" },
          destination: { nodeId: "output-gain", portId: "audio" },
          channelMapping: { mode: "auto" }
        },
        {
          id: "gain-to-os",
          source: { nodeId: "output-gain", portId: "audio" },
          destination: { nodeId: endpoint.id, portId: "to_os" },
          channelMapping: { mode: "auto" }
        }
      ];
      process.stdout.write(JSON.stringify(graph));
    });
  ' "$revision"
}

graph_direct() {
  local revision="$1"
  node -e '
    let value = "";
    process.stdin.on("data", chunk => value += chunk);
    process.stdin.on("end", () => {
      const graph = JSON.parse(value);
      const endpoint = graph.nodes.find(node => node.id === "default-virtual-io");
      if (!endpoint) process.exit(2);
      graph.revision = Number(process.argv[1]);
      graph.nodes = [endpoint];
      graph.edges = [{
        id: "default-pass-through",
        source: { nodeId: endpoint.id, portId: "from_os" },
        destination: { nodeId: endpoint.id, portId: "to_os" },
        channelMapping: { mode: "auto" }
      }];
      process.stdout.write(JSON.stringify(graph));
    });
  ' "$revision"
}

apply_graph() {
  curl -fsS \
    -X PUT \
    -H 'content-type: application/json' \
    --data-binary "$1" \
    "$control_origin/api/v1/graph" >/dev/null
}

restore_direct() {
  if [[ "$gain_active" == true && -n "$restore_payload" ]]; then
    wait_for_revision "$((restore_revision - 1))" || true
    apply_graph "$restore_payload" || true
    wait_for_revision "$restore_revision" || true
  fi
}
trap restore_direct EXIT

current_graph="$(curl -fsS "$control_origin/api/v1/graph")"
current_revision="$(print -r -- "$current_graph" | json_revision)"
gain_revision="$((current_revision + 1))"
restore_revision="$((current_revision + 2))"
gain_payload="$(print -r -- "$current_graph" | graph_with_gain "$gain_revision")"
restore_payload="$(print -r -- "$current_graph" | graph_direct "$restore_revision")"

apply_graph "$gain_payload"
gain_active=true
wait_for_revision "$gain_revision"
"$repo_root/scripts/smoke-macos-installed-duplex.sh" --half-gain

apply_graph "$restore_payload"
wait_for_revision "$restore_revision"
gain_active=false
"$repo_root/scripts/smoke-macos-installed-duplex.sh"

print "MixRelay dynamic graph smoke passed: direct -> -6.0206 dB -> direct"
