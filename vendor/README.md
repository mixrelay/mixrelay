# Temporary plugin-host ABI patches

`truce-rack-vst3`, `truce-rack-clap`, and `truce-rack-lv2` are copied from the crates.io **1.1.5** packages.
Their recorded upstream commit is `233f56c7788291b2f24e3f2aba3a767330f72cb7` in
https://github.com/truce-audio/truce-rack. Root MIT and Apache licenses are preserved here.
Normalized Cargo manifests and upstream READMEs are retained. Registry caches are not modified.

An actual Debian 13/aarch64 workspace build exposed 9 VST3 and 2 CLAP type errors:
the bindings use target-dependent `c_char`, whereas these adapters hard-coded `i8`.
Use VST3 `char8`/`TUID`/`FIDString` and CLAP `c_char` without changing byte layout, DSP or ownership.
TUID and UTF-8 tests include bytes above 127 so signedness changes cannot silently corrupt IDs/names.

The root `[patch.crates-io]` applies this fix consistently on all platforms. These are excluded
from workspace membership so product lints do not accidentally rewrite upstream source.
Run their regression tests explicitly:

```sh
cargo test --locked -p truce-rack-vst3 -p truce-rack-clap --lib
```

Replace the patch with a released upstream fix only after both signed-c_char (macOS/x64) and
unsigned-c_char (Linux ARM64) builds and these tests pass. This is not a plugin runtime certification.

## LV2 state patch

The LV2 adapter's unimplemented state methods now call the standard LV2 State interface directly,
preserving native save/restore failures. Lilv's convenience state helpers otherwise discard native
save failures into a parameter-only snapshot and do not return native restore status. The format
stores a version, exact plugin URI, byte order, input control values by symbol, and owned opaque
properties keyed/typed by URI. `atom:URID` values are remapped across instances. State capture is
limited to portable POD properties (1 MiB total, 4096 entries) and a 4 MiB serialized envelope.

File/path assets, nested URID containers, nonportable properties and a mismatched byte order are
explicit errors, not silently discarded state. These remain unsupported qualification cases.
Restoration validates the complete envelope/control layout before native mutation; the ToneNari
host restores only a prepared worker and discards it on any vendor restore failure.

The new `state.rs`, tests and `fixtures/state.lv2` fixture are ToneNari-authored under the same
MIT OR Apache-2.0 terms. Existing vendor licenses above remain applicable. Sources:
[LV2 state contract](https://lv2plug.in/ns/ext/state) and
[Lilv implementation](https://github.com/lv2/lilv/blob/master/src/state.c).

Linux fixture validation uses a temporary copy of `fixtures/state.lv2`, not installed plugins:

```sh
cc -shared -fPIC -Wall -Wextra -Werror "$fixture_dir/state.lv2/state.c" -o "$fixture_dir/state.lv2/state.so"
LV2_PATH="$fixture_dir" cargo test --locked -p truce-rack-lv2 --lib
LV2_PATH="$fixture_dir" cargo test --locked -p truce-rack-lv2 --lib native_state_fixture -- --ignored
```

On macOS the workspace intentionally does not enable LV2, so use the vendor manifest for a
standalone compile check; native linking/tests require Lilv. Do not label such a check as Linux
runtime verification.
