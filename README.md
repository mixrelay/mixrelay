# MixRelay

MixRelay is an audio-routing application for streamers, audio artists, and independent
broadcasters. The current release supports macOS 15.0 or later (Apple Silicon, and
Apple-supported Intel models through macOS 26) and Linux x86_64/arm64 with PipeWire 1.2
or later and WirePlumber 0.5 or later. Its portable graph
and DSP engine is shared across platforms, while native discovery, capture, and
virtual-device lifecycles remain isolated behind host-audio interfaces.

MixRelay ships the editor and virtual-audio components in one installer, with one
version and update catalog. There is one public MixRelay application and no Driver
selection. One installation supports multiple independently named virtual devices;
channel counts do not require separate installations. Virtual devices support
1–64 channels per direction on macOS/Linux.

## Project status

- [Product and implementation status](docs/product.md): free driver, free/paid app policy, behavior, implementation boundaries and the single work checklist.
- [Design system](docs/design-system.md): Core tokens and contracts, with Foundations, Components, Patterns, Pages and Review documentation in Storybook (`pnpm storybook`).
- [Pause and resume manual](docs/manual/playback.md): shared playback controls and how paused intervals affect recordings, in the 11 supported languages.
- [Manual publication policy](docs/manual/README.md): language support checks and translation scope for Rawsector manuals.
- [Product website draft](apps/website/README.md): static product page and HTML guide; local preview only, with no checkout or downloads yet.

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

macOS release binaries are Universal 2 (Apple Silicon `arm64` and Intel `x86_64`).
`pnpm desktop:build` selects the checked universal app build on macOS; Linux
uses its native Tauri build. Development builds remain host-native.
Build the separate macOS components with:

```sh
pnpm desktop:build
pnpm engine:build:macos-universal
pnpm hal:build:macos-universal
```

The app and engine are written under `target/universal-apple-darwin/release`;
the HAL bundle is written under `target/macos-hal`. Each build verifies both
architecture slices. These commands do not install components or produce a
notarized installer. Universal cross-build success is not native Intel audio
qualification.

The [cross-OS development lab](infra/lab/README.md) separates local macOS validation,
Debian/Ubuntu/Kali/Arch Docker builds, and Linux desktop VM verification.
`pnpm lab:build` and `pnpm lab:test` run inside the actual target OS and record their scope;
they do not imply that native drivers or live audio have passed qualification.

For focused Rust checks that include the desktop, use the same code-check entry:

```sh
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-desktop --lib
```

Code checks do not require installer sidecars or signed driver files. Release builds
use `pnpm desktop:build`, which retains those payload and signature requirements.
Do not use code-check artifacts as installed-product evidence.

The UI dependency direction is `React Aria / React Flow -> Core -> Feature -> Screen`.
Shared components and visual contracts belong to `apps/control-ui/src/core`.

Product decisions and completion status live in [one product document](docs/product.md).
[Performance measurement procedures](docs/performance/audio-engine-render-baseline.md)
describe the native harnesses and comparison conditions.

## Publishing release installers

Support targets, architectures, dependency floors and required qualification scenarios
are defined in [`support/environments.json`](support/environments.json). Run
`pnpm support:check` to detect drift in generated CI inputs and packaging minima.

Follow [release qualification](docs/release-qualification.md) to build once per CPU,
qualify the exact final files on every required native environment, and prepare the
publication manifest. The manifest command requires the candidate and successful
qualification evidence. Missing environments or scenarios block publication.

`publish-updates.yml` verifies the successful native workflow, source commit and
installer hashes before requesting central catalog publication. It uses evidence
downloaded directly from that Actions run. Keep installers unchanged between
qualification and publication.

R2 retains the latest patch installer for each product's major/minor series.
After a new patch is published and verified, previous patch files leave R2 and
the download catalog. GitHub Releases, tags and changelogs are preserved.

## Distribution and licensing

This repository is private. Product installers are prepared for website distribution.
The Stream Deck plug-in will be available only through Elgato Marketplace once its listing
is approved and published. The qualified candidate file is for the owner to upload to
Maker Console.
Free driver, indefinite limited app trial and paid-app policies are defined only in
the [product document](docs/product.md#제품-구성). These are product requirements,
not a claim that the new licensing policy or standalone driver is implemented.
The store and complete licensing flow are not live yet.

MixRelay Driver is proprietary freeware, not an open-source grant. Ordinary personal or business
use follows the free/paid product policy. Incorporating MixRelay into another product or service,
including OEM, bundling, resale or white-label distribution, requires prior written permission
and a separate paid agreement: `support@rawsector.com`. Buying the app does not grant redistribution
rights. Third-party components retain their own licences and notices. See the
[app terms](LICENSE.txt), [Driver terms](LICENSE-Driver.txt) and
[usage and integration policy](docs/product.md#이용-허가와-제품-편입). Both terms are drafts
pending legal review.
