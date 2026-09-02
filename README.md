# ToneNari

ToneNari is a macOS audio-routing application for streamers, audio artists,
and independent broadcasters. The first product goal is a dependable virtual
Capture-to-Stream path that works naturally with OBS.

## Repository layout

- `apps/control-ui`: responsive React control surface shared by desktop and LAN clients
- `apps/desktop`: Tauri application wrapper and macOS menu-bar lifecycle
- `crates/audio-graph`: versioned graph contract and deterministic execution-plan compiler
- `crates/audio-engine`: real-time engine policy and domain state
- `crates/control-protocol`: stable machine-readable control contract
- `crates/control-server`: local HTTP control service; loopback-only by default
- `native/macos/hal-driver`: isolated AudioServerPlugIn implementation boundary
- `packaging/macos`: signed and notarized package work

## Development

```sh
pnpm install
pnpm check
pnpm coverage:rust
pnpm storybook
pnpm desktop:dev
```

The complete macOS build requires full Xcode. Command Line Tools alone are not
sufficient for the HAL driver, signing, notarization, and final bundle checks.

The UI dependency direction is `Mantine -> Core -> Feature -> Screen`. Direct
Mantine imports outside `apps/control-ui/src/core` fail the boundary check.

The product capability floor and local code policy are recorded in
[`ADR 0005`](docs/adr/0005-product-capability-baseline.md) and
[`ADR 0006`](docs/adr/0006-code-quality-policy.md).

## Distribution and licensing

ToneNari is currently developed in a private repository. No open-source license
is granted at this stage. Freeware availability and future commercial licensing
are product distribution decisions and do not imply permission to copy,
redistribute, or create derivative source code.
