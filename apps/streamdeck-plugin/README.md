# MixRelay for Stream Deck

macOS adapter for the existing MixRelay node controls. No public HTTP service,
account, network configuration, or duplicate audio engine is required.
Requires macOS 15.0+, MixRelay running, and Stream Deck 7.1+ (Node.js SDK 3.x requires 7.1; the DRM baseline is 6.9).

## Build and install

From the repository root:

```sh
pnpm install
pnpm --filter @mixrelay/streamdeck-plugin build
pnpm --filter @mixrelay/streamdeck-plugin test
pnpm --filter @mixrelay/streamdeck-plugin validate
pnpm --filter @mixrelay/streamdeck-plugin run pack
node scripts/streamdeck-artifact.mjs build --output /absolute/empty-output-directory
```

The last command builds, validates and packs
`MixRelay-StreamDeck-<product version>.streamDeckPlugin`, then checks the unpacked
archive again and reports its size and SHA-256. It refuses to overwrite an existing file.
The package uses the Cargo product version, with a fourth manifest component of `0`.
`validate` checks the built directory; `run pack` rebuilds, validates and packs in one command.

Download the package from the [MixRelay product page](https://rawsector.com/products/mixrelay)
and open it with Stream Deck. Once the Elgato Marketplace listing is published, you
can also search for MixRelay in Marketplace and install it from the product page. MixRelay offers
**Power, Mute, Volume, and Recording** actions (localised to Korean when selected
in Stream Deck). Choose the graph and a supported node for each action. The plugin UUID is `com.rawsector.mixrelay`; action UUIDs end in `.power`, `.mute`,
`.volume` and `.recording`. These identities are fixed before first publication.
Recording only lists recorder nodes, with Record/Resume, Pause, and Stop choices.
Recording uses the folder and format selected in MixRelay's recorder settings.
The property inspector shows these linked settings read-only; no folder typing or
duplicate recording preferences are needed. The shared default follows the input
sample rate and uses PCM16. MixRelay creates each take's filename. Resume retains
the original take and its format.

Volume works on buttons and Stream Deck + dials. Choose an increase/decrease button
and a 0.5, 1, 2, or 3 dB step; the key displays its direction and current gain.
Dial rotation uses the same step and all ticks in each event; pressing the dial mutes
when the target supports mute. Output nodes without gain are not listed as volume targets.

## Release and Marketplace

`pnpm release:macos` builds this separate companion download alongside the macOS PKG.
The plugin is not installed by the PKG: Stream Deck owns its installation.
`support/environments.json` requires the package in `mix-relay.candidate.json` with
its size and SHA-256. Both candidate workflows validate the actual unpacked archive
with the pinned Elgato CLI, check identity, SDK 3, Stream Deck 7.1, Node.js 24 and
product version, and preserve hash-bound evidence. Publication verifies the same
candidate files and evidence. Attach all five candidate artefacts to both releases.

The app's `mix-relay.release.json` contains only desktop installers. Stream Deck
updates and installs its own plugin; the app updater must never select the companion
package as an app installer. The candidate still requires and verifies the plugin.
See the Korean [Marketplace listing kit](../../docs/streamdeck-marketplace.md).

`icons.mjs` renders 256/512 px plugin PNGs from `branding/identity/app-icon.svg`
and derives the 28 px white category SVG from the approved symbol. Action list
icons are transparent white 20 px SVGs; keys use 72 px SVGs and dynamic 144 px SVGs.
The volume dial uses Stream Deck's built-in `$B1` layout within its 200 × 100 area.
Display changes coalesce at 100 ms intervals and unchanged key/dial feedback is
not resent. Failures show an alert on both keys and dials. Settings save on change.
Plugin code uses embedded action identities and never reads manifest metadata at
runtime. The SDK handles its own protected manifest support; distributed resources
are immutable. Runtime logs are excluded from the package through `.sdignore`.

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
the real bundled SDK against a simulated Stream Deck host with a protected,
unreadable manifest. They also check failure alerts, automatic settings persistence,
display throttling, generated icon sizes and archive validation. These do not prove
physical key/dial behaviour. Real Stream Deck installation and hardware validation
remain required before release.

Built with the [official Elgato SDK](https://docs.elgato.com/streamdeck/sdk/introduction/plugin-environment/)
and validated with the [official CLI](https://docs.elgato.com/streamdeck/cli/commands/validate/).
