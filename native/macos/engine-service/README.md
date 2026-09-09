# ToneNari macOS engine service

This is the launchd-activated control-plane companion for the macOS HAL plug-in.
The executable entry point is Rust; the native layer is limited to the named
XPC listener, fixed-layout endpoint transport, and descriptor transfer required
by macOS.

The service creates an unlinked temporary backing file, maps and initializes a
duplex endpoint transport, and responds only to the versioned endpoint request
defined by `TNHALControlProtocol`. No PCM crosses XPC. The HAL receives a
duplicated descriptor and performs its own full layout and channel validation.

After initialization, the Rust worker prepares a stereo virtual-I/O
pass-through graph once, allocates its block buffers once, and processes
128-frame chunks directly between the shared render and capture buffers. The
shared transport publishes an absolute-frame watermark and stream epoch, so the
worker neither guesses the Core Audio cursor nor replays a previous timeline.
Graph lowering resolves node IDs, ports, and channel routes before the loop;
the steady-state block path performs no graph lookup, allocation, or lock.

Run the non-installing smoke test with:

```sh
pnpm smoke:macos-engine-xpc
```

The script registers an ephemeral user LaunchAgent, crosses the real named Mach
service, validates the received transport through the HAL attachment layer, and
removes the service on both success and failure. It does not install the HAL
driver or modify `/Library`.

The current same-user peer check is a development baseline, not final client
authentication. Signed production builds must validate the expected peer
identity before exposing live audio transport.
