# MixRelay for Stream Deck

macOS adapter for the existing MixRelay node controls. No public HTTP service,
account, network configuration, or duplicate audio engine is required.
Requires macOS 14.2+, MixRelay running, and Stream Deck 6.6+.

## Build and install

From the repository root:

```sh
pnpm install
pnpm --filter @mixrelay/streamdeck-plugin build
pnpm --filter @mixrelay/streamdeck-plugin test
pnpm --package=@elgato/cli dlx streamdeck validate apps/streamdeck-plugin/dist/io.mixrelay.streamdeck.sdPlugin
pnpm --package=@elgato/cli dlx streamdeck pack apps/streamdeck-plugin/dist/io.mixrelay.streamdeck.sdPlugin --output apps/streamdeck-plugin/dist
```

Open the resulting `.streamDeckPlugin` package with Stream Deck. Add **MixRelay →
Node control**, choose a graph and node, then a supported action. Record, pause and
stop appear only for recorder nodes. For a new recording, enter an existing
absolute destination folder. Each take gets a unique filename, follows the input
sample rate and uses the app's PCM24 default. Resume retains the original take.

Stream Deck + dials list only nodes with gain: rotate changes gain in 0.5 dB steps;
press mutes. Output nodes without gain are not falsely presented as volume controls.

## Control contract

The macOS desktop owns `~/Library/Application Support/io.mixrelay.desktop/control/control.sock`.
Directory permissions are `0700`, socket permissions `0600`; peer user identity is checked.
This socket does not accept URLs, arbitrary engine routes, file reads, scripts or shell commands.
Its typed node command shares the desktop's application-capture preparation and engine
publication path. The existing internal engine HTTP transport is not replaced or newly exposed.

One newline-delimited JSON request per connection, at most 16 KiB. At most eight
connections are admitted. Read/write timeouts are two seconds and responses are
bounded to 4 MiB. Snapshot requests can be cancelled; accepted changes are drained
before releasing engine ownership on app exit. Idle clients cannot hold shutdown open.

```json
{"id":1,"command":{"type":"snapshot"}}
```

The response is `{id,status,body}`. A successful snapshot body contains
`protocolVersion:1`, a desktop `session`, engine `revision`, `enginePower`, and
`graphs:[{id,name,nodes}]`. Nodes contain `nodeId`, `label`, `kind`,
`control:{enabled,muted}`, `capabilities:{power,mute,recorder}`, optional `gainDb`,
and recorder status only for recorder nodes.

```json
{"id":2,"command":{"type":"node","session":"observed-session","target":{"graphId":"graph-1","nodeId":"gain-1"},"request":{"expectedRevision":42,"enabled":false}}}
```

Node updates accept explicit `enabled`, `muted`, and supported `gainDb` fields.
HTTP-style status `202` means accepted publication, not confirmed active audio;
the plugin refreshes a coherent snapshot before displaying the resulting state.
Recorder commands additionally require `expectedRecording:{state,path}` so a
late stop cannot stop a newer take at the same graph revision.

No offline queue, automatic write retry, optimistic state, or fallback target
selection is used. OFF nodes permit power-on, but other hardware commands fail
locally; removed nodes remain unavailable until explicitly reassigned. The engine
validates revisions and capabilities again. App restart invalidates prior sessions.

The UI rebases controls-only changes onto layout and Undo/Redo history. Structural
conflicts require explicit recovery instead of silently discarding edits.

## Validation boundary

Automated tests exercise protocol failures, OFF/deleted targets, take fences and
the real bundled SDK against a simulated Stream Deck host. These do not prove
physical key/dial behavior. Real Stream Deck installation and hardware validation
remain required before release.

Built with the [official Elgato SDK](https://docs.elgato.com/streamdeck/sdk/introduction/plugin-environment/)
and validated with the [official CLI](https://docs.elgato.com/streamdeck/cli/commands/validate/).
