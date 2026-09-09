# ToneNari Windows WaveRT driver boundary

This directory owns an experimental ToneNari PortCls/WaveRT miniport and the version 2
kernel/user transport contract. It is **not yet qualified for installation or distribution**.
Build on Windows with Visual Studio, the Windows SDK, and WDK; production distribution
still requires Microsoft signing and native audio/HLK qualification.

The accepted direction is to replace this experimental generic miniport framework
with the upstream SysVAD framework, retaining ToneNari-specific transport work.
The original sample now enumerates and streams in the same ARM64 VM. A separate
diagnostic overlay also passes actual shared-PCM routing with this transport, but
an opt-in Rust LAB adapter now consumes that mapping. Product configuration is
still unqualified. See the
[reuse decision](../../../docs/adr/0011-windows-sysvad-reuse.md) and
[bridge results](../../../docs/performance/2026-09-07-windows-sysvad-bridge.md).

Microsoft's SysVAD sample is the reference for PortCls/WaveRT miniport structure, but it is not a
drop-in product driver: its capture path generates a tone and its endpoint identities and formats
are samples. This product-owned render/capture bridge currently:

- validates discrete F32 stream formats with 1–64 channels at 44.1/48/96 kHz;
- copies render DMA frames into the bounded transport consumed by the engine;
- copies engine capture frames into the capture DMA buffer, zero-filling underflow;
- validates all IOCTL sizes and counters;
- drains DPC references before unmapping on file cleanup or owner-process exit;
- marks transport offline on PnP stop/removal without revoking a live user's pointer;
- performs no allocation, waiting, or user callback from the WaveRT position/update path.

`include/ToneNariTransport.h` defines the IOCTL handshake. The Rust virtual-endpoint
adapter consumes this v2 mapping **only through the separate SysVAD LAB control
device**, not this experimental driver. Do not report the product virtual device
usable merely because the driver builds. `ToneNariAudio.inf` declares only ARM64 for now.
Channel/name reconfiguration and full format negotiation are also unfinished: the
current data-range intersection selects a stereo/48 kHz default, even though direct
stream validation supports more formats.

The shared mapping contains a fixed-size header followed by render and capture rings.
The driver keeps addresses, lengths, format, and its own counters in private kernel
storage. User-writable header fields never determine a kernel memory-copy address or
length. User event handles allow waiting, not signaling. One controller mapping and
one host stream per direction are supported; a conflicting attachment fails explicitly.

`scripts/lab/guest-windows-driver-check.cmd <unique-job-id>` builds with `/kernel`,
`/W4 /WX`, stack protection, CFG and integrity checking, validates the INF, and executes
the shared PCM-copy tests. This command does not sign, install, or change boot policy.
The WDK is pinned by `scripts/lab/prepare-windows-wdk.cmd`.

Portable ASan/UBSan check (from the repository root):

```sh
mkdir -p target/native-tests
clang++ -std=c++17 -O1 -Wall -Wextra -Werror -fsanitize=address,undefined \
  -I native/windows/virtual-audio-driver/include \
  native/windows/virtual-audio-driver/tests/ring_test.cpp -o target/native-tests/driver-ring-test
target/native-tests/driver-ring-test
```

This checks production copy arithmetic, not the kernel API lifecycle. The authorized
Windows ARM64 VM now loads test-signed version 0.1.0.2: 192 native controller
attach/reject/cleanup cycles pass, and four live KS interfaces answer pin queries.
However, AudioEndpointBuilder still creates no ToneNari MMDevice endpoints. This is
a failing integration check, not usable virtual audio; see the
[native validation record](../../../docs/performance/2026-09-07-windows-wavert-preflight.md).
Process death with duplicated handles, PnP stop/remove/re-enable, WaveRT PCM,
Driver Verifier, sustained CPU/latency measurements and release signing remain open.
