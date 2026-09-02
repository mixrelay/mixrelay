# Integration validation

This directory is reserved for behavior that cannot be represented by a unit
test: installed HAL plug-ins, real Core Audio devices, OBS fixtures, process-tap
permissions, hot-plug behavior, and independent hardware clocks.

All deterministic domain, compiler, protocol, DSP, and adapter-state behavior
belongs in unit tests beside its owner. Rust library coverage is gated at 100%
for lines, functions, and regions by `pnpm coverage:rust`.

Long-running validation records underruns, overruns, CPU use, memory use, and
measured latency under identical conditions.
