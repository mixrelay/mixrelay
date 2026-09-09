# ToneNari

ToneNari is a cross-platform audio-routing application for streamers, audio artists,
and independent broadcasters. Its portable graph and DSP engine is shared across macOS,
Linux/PipeWire, and Windows/WASAPI, while native discovery, capture, and virtual-device lifecycles
remain isolated behind host-audio interfaces.

## Repository layout

- `apps/control-ui`: responsive React control surface shared by desktop and LAN clients
- `apps/desktop`: Tauri application wrapper and desktop tray lifecycle
- `crates/audio-graph`: versioned graph contract and deterministic execution-plan compiler
- `crates/audio-engine`: real-time engine policy and domain state
- `crates/processor-host`: isolated AU/VST3/CLAP/LV2 discovery, DSP workers, and native editor broker
- `crates/control-protocol`: stable machine-readable control contract
- `crates/control-server`: local HTTP control service; loopback-only by default
- `apps/engine-service`: platform audio backend, graph lifecycle, and control service process
- `native/macos/hal-driver`: isolated AudioServerPlugIn implementation boundary
- `packaging`: platform-specific service, driver, and installer boundaries

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

The [cross-OS development lab](infra/lab/README.md) separates local macOS validation,
Debian/Ubuntu/Kali/Arch Docker builds, and Linux/Windows desktop VM verification.
`pnpm lab:build` and `pnpm lab:test` run inside the actual target OS and record their scope;
they do not imply that native drivers or live audio have passed qualification.

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
