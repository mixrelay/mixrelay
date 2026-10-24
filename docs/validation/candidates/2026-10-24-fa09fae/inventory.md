# Candidate qualification

Source: fa09faedaef98e8fc4b908d663ea6b5b5a6c2f7b

Contract: ce58607aa1cc87b3d2ad4f2e86b3349cccd6f547f10be2a33be51c7504cc9833

| Environment | Artifact SHA-256 | Result |
| --- | --- | --- |
| macos-14-arm64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | passed |
| macos-15-arm64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | macos-15-arm64: AUDIO-01: failed |
| macos-15-x64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | macos-15-x64: AUDIO-01: failed |
| macos-26-arm64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | passed |
| macos-26-x64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | macos-26-x64: AUDIO-01: failed |
| macos-27-arm64 | 8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5 | macos-27-arm64: AUDIO-01: failed |
| debian-13-x64 | be9561f2bd7941bb975567a1ae0e9bcc270fe2a29773cbf8293d4fb03c053af6 | passed |
| debian-13-arm64 | a1dd313d99f414e8d17cc81651ca9ea5856515dee0ab4b0992a93fa36dbc7add | passed |
| ubuntu-26-04-x64 | be9561f2bd7941bb975567a1ae0e9bcc270fe2a29773cbf8293d4fb03c053af6 | passed |
| ubuntu-26-04-arm64 | a1dd313d99f414e8d17cc81651ca9ea5856515dee0ab4b0992a93fa36dbc7add | ubuntu-26-04-arm64: GRAPH-01: failed |
| kali-rolling-x64 | be9561f2bd7941bb975567a1ae0e9bcc270fe2a29773cbf8293d4fb03c053af6 | passed |
| kali-rolling-arm64 | a1dd313d99f414e8d17cc81651ca9ea5856515dee0ab4b0992a93fa36dbc7add | passed |
| arch-rolling-x64 | 9c18442167ba44d4f68f31526d5ed2d4bb21bf8b246771ac0156e7f60d7a10e5 | passed |

## Observed installed environments

| Environment | OS version | CPU | Kernel | PipeWire | WirePlumber | glibc | Run |
| --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-arm64 | 14.8.9 | arm64 | 23.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| macos-15-arm64 | 15.7.9 | arm64 | 24.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| macos-15-x64 | 15.7.9 | x64 | 24.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| macos-26-arm64 | 26.6.2 | arm64 | 25.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| macos-26-x64 | 26.6.1 | x64 | 25.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| macos-27-arm64 | 27.0.1 | arm64 | 27.0.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| debian-13-x64 | Debian GNU/Linux 13 (trixie) (13.7) | x64 | 6.12.113+deb13-cloud-amd64 | 1.4.2 | 0.5.8 | 2.41 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| debian-13-arm64 | Debian GNU/Linux 13 (trixie) (13.7) | arm64 | 6.12.113+deb13-cloud-arm64 | 1.4.2 | 0.5.8 | 2.41 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| ubuntu-26-04-x64 | Ubuntu 26.04.1 LTS (26.04) | x64 | 7.0.0-38-generic | 1.6.2 | 0.5.13 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| ubuntu-26-04-arm64 | Ubuntu 26.04.1 LTS (26.04) | arm64 | 7.0.0-38-generic | 1.6.2 | 0.5.13 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| kali-rolling-x64 | Kali GNU/Linux Rolling (2026.1) | x64 | 6.18.12+kali-cloud-amd64 | 1.6.8 | 0.5.15 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| kali-rolling-arm64 | Kali GNU/Linux Rolling (2026.1) | arm64 | 6.18.12+kali-cloud-arm64 | 1.6.8 | 0.5.15 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |
| arch-rolling-x64 | Arch Linux (rolling) | x64 | 7.2.9-arch1-1 | 1.6.10 | 0.5.18 | 2.44 | https://github.com/mixrelay/mixrelay/actions/runs/40095387842 |

## Scenario results

| Environment | PKG-01 | APP-01 | GRAPH-02 | AUDIO-01 | REC-02 | GRAPH-01 | AUDIO-11 | UPGRADE-01 | PKG-02 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-arm64 | passed | passed | passed | passed | passed | passed | passed | passed | passed |
| macos-15-arm64 | passed | passed | passed | failed | passed | passed | passed | failed | passed |
| macos-15-x64 | passed | passed | passed | failed | passed | passed | passed | passed | passed |
| macos-26-arm64 | passed | passed | passed | passed | passed | passed | passed | passed | passed |
| macos-26-x64 | passed | passed | passed | failed | passed | passed | passed | failed | passed |
| macos-27-arm64 | passed | passed | passed | failed | passed | passed | passed | failed | passed |
| debian-13-x64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |
| debian-13-arm64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |
| ubuntu-26-04-x64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |
| ubuntu-26-04-arm64 | passed | passed | passed | passed | passed | failed | not applicable | passed | passed |
| kali-rolling-x64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |
| kali-rolling-arm64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |
| arch-rolling-x64 | passed | passed | passed | passed | passed | passed | not applicable | passed | passed |

## Failure and blocking diagnostics

Full diagnostics remain in the per-target qualification JSON and raw Actions evidence.

- ubuntu-26-04-arm64 / GRAPH-01 (failed): Installed UI runner failed: Observed installed menu expansion: Add node Observed installed submenu expansion: Processor (1 Right Arrow submissions) Submitted installed UI control: Compressor (15.07s) Observed installed menu expansion: Add node Observed installed submenu expansion: Output (1 Right Arrow submissions) Submitted installed UI control: Recorder (17.07s) Submitted installed UI control: Connect channels… (38.84s) Submitted installed UI control: Destination (13.35s) Submitted installed UI control: Recorder (7.06s) Submitted installed UI control: Connect (8.16s) Traceback (most recent call last):   File "/home/qa/workspace/scripts/e2e/linux-ui.p…
- macos-15-arm64 / AUDIO-01 (failed): missingFrames  488 !== 0
- macos-15-arm64 / UPGRADE-01 (failed): missingFrames  1056 !== 0
- macos-15-x64 / AUDIO-01 (failed): missingFrames  832 !== 0
- macos-26-x64 / AUDIO-01 (failed): missingFrames  3640 !== 0
- macos-26-x64 / UPGRADE-01 (failed): missingFrames  10232 !== 0
- macos-27-arm64 / AUDIO-01 (failed): missingFrames  1412 !== 0
- macos-27-arm64 / UPGRADE-01 (failed): missingFrames  896 !== 0

## VM execution diagnostics

| Environment | Accelerator | CPU model | Result | Pinned image checksum |
| --- | --- | --- | --- | --- |
| arch-rolling-x64 | kvm | host | guest suite completed | 08754e28d91b1680086c6211ef4df13559e766fff7cdb4006489b14b5d281f08 |
| debian-13-arm64 | tcg | cortex-a72 | guest suite completed | 0199ea72dd82e84264fda5068c5756412c778f395fca6748a186f2d0a34f524fe406e87339bda0b7f71aa9bab066b9850cde44cd1650116f389dc3f786cb232b |
| debian-13-x64 | kvm | host | guest suite completed | ec584ccabfef7120b4c4f7482adf0e615894d9292cedec446b64194b951ebe5dd97c336b5dcbae458333764b0edd516334506017f33c4f2020ef8d38b569ffe3 |
| kali-rolling-arm64 | tcg | cortex-a72 | guest suite completed | 9ab19c28d049fdc6f1d5d30f6cc93b8b01997f11c89f6992f690c07b16b7b7e4 |
| kali-rolling-x64 | kvm | host | guest suite completed | 3f4ca86ae1eca0dc2bf2092f065a959508e186c6b44f85758813a8e9a0604f3e |
| ubuntu-26-04-arm64 | tcg | cortex-a72 | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 8eb876c2ccc4b6ff89e4a65d511815923b285e04503ba465809bf587f763da24 |
| ubuntu-26-04-x64 | kvm | host | guest suite completed | 5645d0318ad21f643ebb1ac4567514620e9893380b07b0201fda163baa5b7eb5 |
