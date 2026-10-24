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

Open the resulting `.streamDeckPlugin` package with Stream Deck. MixRelay offers
**Power, Mute, Volume, and Recording** actions (localized to Korean when selected
in Stream Deck). Choose the graph and a supported node for each action. Configured
power keys retain their targets through the stable Power action identity.
Recording only lists recorder nodes, with Record/Resume, Pause, and Stop choices.
Recording uses the folder and format selected in MixRelay's recorder settings.
The property inspector shows these linked settings read-only; no folder typing or
duplicate recording preferences are needed. The shared default follows the input
sample rate and uses PCM16. MixRelay creates each take's filename. Resume retains
the original take and its format.

Volume works on buttons and Stream Deck + dials. Choose an increase/decrease button
and a 0.5, 1, 2, or 3 dB step; the key displays its direction, step, and current gain.
Dial rotation uses the same step and all ticks in each event; pressing the dial mutes
when the target supports mute. Output nodes without gain are not listed as volume targets.

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
`protocolVersion:1`, a desktop `session`, engine `revision`, `activeRevision`,
`structureGeneration`, `enginePower`, and
`graphs:[{id,name,nodes}]`. Nodes contain `nodeId`, `label`, `kind`,
`control:{enabled,muted}`, `capabilities:{power,mute,recorder}`, optional `gainDb`,
and recorder status only for recorder nodes.

Recorder nodes also include desktop-owned
`recordingSettings:{folder,sampleRate,encoding}`. A null sample rate means follow
input. The plugin sends only `{action}` for record/pause/stop, with the existing
observed-take fence; MixRelay resolves the shared destination and format.

```json
{"id":2,"command":{"type":"node","session":"observed-session","target":{"graphId":"graph-1","nodeId":"gain-1"},"request":{"expectedRevision":42,"enabled":false}}}
```

Node updates accept explicit `enabled`, `muted`, and supported `gainDb` fields.
HTTP-style status `202` means accepted publication, not confirmed active audio;
the plugin waits for the coherent snapshot's `activeRevision` to acknowledge the
command before displaying its resulting state. Ordinary controls update the
current render plan without reloading the graph. A control acknowledgement also
includes `requiresGraphReload`, allowing UI clients to read reconciled metadata
only when first-time native preparation changed the canonical document.
Recorder commands additionally require `expectedRecording:{state,path}` so a
late stop cannot stop a newer take at the same graph revision.

No offline queue, automatic write retry, optimistic state, or fallback target
selection is used. OFF nodes permit power-on, but other hardware commands fail
locally; removed nodes remain unavailable until explicitly reassigned. The engine
validates revisions and capabilities again. App restart invalidates prior sessions.

Rapid volume input coalesces into one bounded desired gain while the current write
finishes. Each input event clamps before the next, preserving reverse movement at a
limit. Trailing writes require the exact acknowledged revision, session, topology,
and enabled target. Silence or a single stalled write beyond 750 ms, rejection,
disconnect, settings changes, or external edits discard the remainder; failed writes
are never replayed. The displayed gain remains confirmed state, not desired state.

The UI rebases controls-only changes onto layout and Undo/Redo history. Structural
conflicts require explicit recovery instead of silently discarding edits.

## Validation boundary

Automated tests exercise protocol failures, OFF/deleted targets, take fences and
the real bundled SDK against a simulated Stream Deck host. These do not prove
physical key/dial behavior. Real Stream Deck installation and hardware validation
remain required before release.

Built with the [official Elgato SDK](https://docs.elgato.com/streamdeck/sdk/introduction/plugin-environment/)
and validated with the [official CLI](https://docs.elgato.com/streamdeck/cli/commands/validate/).
