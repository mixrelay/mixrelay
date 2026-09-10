# Cross-OS development lab

macOS is tested on the existing Apple Silicon host. There is **no macOS VM requirement**.
Linux distributions are separate compatibility targets, not aliases for one Ubuntu result.
See [the actual validation snapshot](VALIDATION.md) for passed, blocked, and unrun checks.

| Target | Build environment on this Mac | Desktop/audio environment | Not proved here |
| --- | --- | --- | --- |
| macOS arm64 | Local Xcode + Rust | Local logged-in session | Other Macs, release signing |
| Debian 13 | Debian Docker image, arm64 | Debian arm64 UTM desktop VM | Physical device timing |
| Ubuntu 24.04 LTS | Ubuntu Docker image, arm64 | Ubuntu arm64 UTM desktop VM | Physical device timing |
| Kali rolling | Kali Docker image, arm64 | Kali arm64 UTM desktop VM | Every future rolling update |
| Arch rolling | Official Arch Docker image, **amd64 emulated** | Official Arch x64 UTM **emulation**, then x64 host/VM | Native timing on this Mac |
| Windows 11 ARM | Native toolchain inside Windows ARM UTM VM | Same interactive Windows guest | x64 kernel drivers |
| Windows x64 | x64 Windows host/VM or native CI runner | x64 Windows host/VM | Physical device timing in CI |

Arch Linux ARM is a different distribution and must not be labelled an official Arch x64 test.
UTM uses Mac virtualization for matching architectures; x64 guests on this ARM Mac need emulation.
Docker Desktop's Linux guest is useful for builds but is not a substitute for a distribution's
desktop login, PipeWire/WirePlumber session, kernel, or Windows driver stack.

## Linux build containers

From the repository root:

```sh
docker compose -f infra/lab/compose.yaml build debian
docker compose -f infra/lab/compose.yaml run --rm debian build
# Repeat with ubuntu, kali, arch. Use test instead of build to also run unit tests.
```

Build one image at a time on a memory/disk-constrained host. Source is copied into the image:
rebuild after source changes. Host node_modules/target, credentials, .git, and local agent context
are excluded. Images contain private product source: do not push them to a public registry.
Containers run without root/capabilities, host device/socket mounts, or exposed ports.

Rust 1.96.0, Node 24.19.0, pnpm 11.9.0 and base image digests are pinned. Apt/pacman repositories
still move: image digests alone do **not** make installed distro packages reproducible. Archive
the resulting private image digest and package inventory with each qualified test run; requalify
rolling updates. To extract a failed or successful build report, retain a named container:

```sh
docker compose -f infra/lab/compose.yaml run --name mixrelay-debian-check debian test
docker cp mixrelay-debian-check:/workspace/.lab/reports .lab/debian-reports
# Remove only this stopped, disposable container after collecting its evidence:
docker rm mixrelay-debian-check
```

Ensure `.lab` exists first. The CLI returns nonzero for failure; do not hide it with `|| true` in CI.
No container result counts as a live audio or plugin-window pass.

On this Apple Silicon host, official Arch's pacman download sandbox currently fails under
Docker's amd64 emulation (`error restricting syscalls via seccomp: 22`). Do not disable
container isolation/signature checking to label it successful. Use the native x64 CI lane
or a real x64 Linux Docker host; the full-system Arch VM remains a separate check.

The workflow `.github/workflows/native-lab.yml` covers native macOS, Windows x64/ARM64,
and Debian/Ubuntu/Kali x64/ARM64 plus Arch x64. Workflow presence is not a completed CI run.

## Desktop VMs on this Mac

Use [UTM](https://docs.getutm.app/) for the foreign desktop guests. Installation:

```sh
brew install --cask utm
pnpm lab:vm list
pnpm lab:vm start <UUID>
pnpm lab:vm status <UUID>
pnpm lab:vm shutdown <UUID>
```

`shutdown` requests a clean guest shutdown; it never force-kills the VM. macOS may ask the user
to allow automation of UTM. OS installation/account setup is deliberately interactive: the lab
does not accept licenses, bypass Windows requirements, embed passwords, or enable test signing.

Create one VM per target in UTM, keeping its disk on a volume with adequate space:

| Guest name | Wizard | Initial resource budget | Installation media |
| --- | --- | --- | --- |
| MixRelay Debian | Virtualize / Linux, ARM64 | 4 vCPU, 6 GiB RAM, 64 GiB disk | Official Debian arm64 installer |
| MixRelay Ubuntu | Virtualize / Linux, ARM64 | 4 vCPU, 6 GiB RAM, 64 GiB disk | Official Ubuntu arm64 installer; install a desktop if using server media |
| MixRelay Kali | Virtualize / Linux, ARM64 | 4 vCPU, 6 GiB RAM, 64 GiB disk | Official Kali ARM64 installer, not an x64 appliance |
| MixRelay Arch | Emulate / Linux, x86_64 | 4 vCPU, 6 GiB RAM, 64 GiB disk | Official Arch x86_64 ISO |
| MixRelay Windows | Virtualize / Windows, ARM64 | 4 vCPU, 8 GiB RAM, 96 GiB disk | Official Windows 11 ARM64 ISO, valid license |

These are development budgets, not OS minimums. Sparse disks still grow; do not overcommit the
current host's free space. Run guests sequentially initially. Follow the official
[Windows wizard/guest-tools guide](https://docs.getutm.app/guides/windows/), including UEFI/TPM
requirements. Use NAT networking, no public RDP/SSH forwarding, and no shared host home directory.
Never attach the host's active audio interface to a guest without explicitly releasing it first.

Record the installer version and verified SHA-256, UTM version, guest architecture, allocated
resources and guest OS build in the evidence report. Use official installer checksum/signature
instructions. A VM merely starting does not qualify its audio backend.

`images.json` records the Linux media URLs and SHA-256 values checked for this lab.
`scripts/lab/create-linux-vm.applescript` creates/exports an **unbooted staging** VM using UTM's
API; it does not install an OS. Validate the exported disks, then remove only that newly created
internal staging copy and open the external bundle in place. Do **not** use UTM's scripting
`import` for this step: it copies the bundle back into internal Documents. Attach the verified
installer ISO again after registration; removable-media bookmarks are not part of config.plist.
Before booting, enable HDA sound and disable clipboard/directory sharing in the final VM settings.

### Linux guest bootstrap

The optional `scripts/lab/bootstrap-linux-toolchains.sh` installs pinned toolchains under the
existing non-root guest account, verifying Node/rustup SHA-256 values before execution. It does
not create accounts, change passwords, or edit shell profiles. `guest-first-build.sh` expects a
source-only archive and refuses to replace an existing `~/mixrelay` guest checkout.

For long UTM guest-agent commands, wrap execution with `scripts/lab/guest-job.sh <unique-id>
<command> ...`. UTM's immediate command return is **not** a completion signal. Read both
`<checkout>/.lab/jobs/mixrelay-lab-<unique-id>/output.log` and its atomic `exit-code` file; a missing marker
means incomplete, and a nonzero marker means failed. Jobs never replace earlier evidence.

`guest-desktop-build.sh` enables `tauri/custom-protocol` to embed the built frontend. A plain
Cargo debug build otherwise expects the development URL. After starting an isolated guest
engine, `node scripts/lab/pipewire-smoke.mjs --run` checks the default virtual endpoint's
stereo signal, meters, and internal silent gaps. It requires explicit existing virtual targets,
never changes default devices, and does not certify latency, hardware or endurance.

Install a desktop and log into it. Clone the repository onto the **guest's disk** (do not reuse
macOS node_modules/target). Install the repository's Rust/Node/pnpm versions. Then:

```sh
sudo bash infra/lab/install-linux-deps.sh
pnpm install --frozen-lockfile
pnpm lab:test
systemctl --user status pipewire wireplumber
pw-cli info 0
wpctl status
```

If the distribution is using PulseAudio instead, explicitly configure its PipeWire desktop
session following distro instructions; dependency installation alone does not migrate a running
session. Re-login as needed. Do not run MixRelay as root or start a second server over an existing
PipeWire socket. Use both X11 and Wayland sessions when available for native plugin editors.
After this preflight, the existing Linux development audio installer is an **explicit** separate
step (`pnpm install:linux-dev-audio`), because it changes the guest's user service.

### Windows guest bootstrap

Install Git, Node 24, pnpm 11.9.0, Rust 1.96.0 (MSVC), Visual Studio C++ Build Tools with the
matching ARM64 or x64 tools, Windows SDK and WebView2. Use a Developer PowerShell with the target
toolchain enabled. Clone onto the guest's NTFS disk, then run:

```powershell
pnpm install --frozen-lockfile
rustc --version --verbose
pnpm lab:test
pnpm desktop:dev
```

The Rust host triple must match the claimed target. WDK and a separately signed driver package
are needed for virtual endpoints. **The repository currently lacks a complete WaveRT miniport
and INF; the installer script is not a driver implementation.** Do not fabricate a successful
driver result or use an x64 kernel driver in an ARM64 VM.

## Acceptance evidence

`lab:build` compiles UI and the complete Rust workspace; `lab:test` adds UI/Rust unit tests and
the existing C sanitizer checks on macOS. Each writes `.lab/reports/*.json` including OS, CPU,
kernel, commands, timings, failures, and an explicit list of things it did not verify.

For **each guest**, separately record actual results for:

- Empty graph startup/reconnect, device discovery, hotplug and per-application capture.
- Virtual endpoint creation/removal; 2/8/16/32/64 channels and channel 13/33 routing.
- 44.1/48/96 kHz transitions, mixed-rate devices, clock drift and underruns.
- Audible output and measured per-channel input/processor/output meters, not just HTTP success.
- VST3/CLAP editors opening, resizing, closing/reopening; LV2 on Linux; AU on local macOS.
- Plugin crash isolation, repeated graph/stream/editor teardown and memory endurance.
- Native driver install/uninstall/signing and recovery after sleep/reboot.

Record `not-run`, `blocked`, `failed`, or `passed` with evidence. VM latency is diagnostic data,
not a product hardware latency claim. Physical Windows/Linux audio hardware remains a release
gate. Current Linux FIFO/timer transport must still be replaced/qualified against the native
PipeWire callback boundary; this lab does not claim that engine work is already implemented.

References: [Docker Desktop VM boundary](https://docs.docker.com/desktop/features/vmm/),
[UTM automation](https://docs.getutm.app/scripting/scripting/),
[Tauri native prerequisites](https://v2.tauri.app/start/prerequisites/).
