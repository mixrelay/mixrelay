# Candidate qualification

Source: 42088b93c5e05119b52b939cf484ae8168ee49d6

Contract: ce58607aa1cc87b3d2ad4f2e86b3349cccd6f547f10be2a33be51c7504cc9833

| Environment | Artifact SHA-256 | Result |
| --- | --- | --- |
| macos-14-arm64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-14-arm64: missing report |
| macos-15-arm64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-15-arm64: missing report |
| macos-15-x64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-15-x64: missing report |
| macos-26-arm64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-26-arm64: missing report |
| macos-26-x64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-26-x64: missing report |
| macos-27-arm64 | 40393463521640c68c274064654c9435d9a65e5cd1503d056016c5754401a9cb | macos-27-arm64: missing report |
| debian-13-x64 | 11e17aa3fab010f2b6cfad00e49f6e7ef24b046ab12a71c544ce27b95cbd8ed1 | debian-13-x64: missing report |
| debian-13-arm64 | 7759cb111d19c79f183309d127f28dd8530d5208187fbedd673b912c4ce65581 | debian-13-arm64: missing report |
| ubuntu-26-04-x64 | 11e17aa3fab010f2b6cfad00e49f6e7ef24b046ab12a71c544ce27b95cbd8ed1 | ubuntu-26-04-x64: missing report |
| ubuntu-26-04-arm64 | 7759cb111d19c79f183309d127f28dd8530d5208187fbedd673b912c4ce65581 | ubuntu-26-04-arm64: GRAPH-01: failed |
| kali-rolling-x64 | 11e17aa3fab010f2b6cfad00e49f6e7ef24b046ab12a71c544ce27b95cbd8ed1 | kali-rolling-x64: missing report |
| kali-rolling-arm64 | 7759cb111d19c79f183309d127f28dd8530d5208187fbedd673b912c4ce65581 | kali-rolling-arm64: missing report |
| arch-rolling-x64 | a5e09b93c5a8a1a1c23a57a4b2820ae375698023a11f0593a518c3640a28eca2 | arch-rolling-x64: missing report |

## Observed installed environments

| Environment | OS version | CPU | Kernel | PipeWire | WirePlumber | glibc | Run |
| --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-arm64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-15-arm64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-15-x64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-26-arm64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-26-x64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-27-arm64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| debian-13-x64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| debian-13-arm64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| ubuntu-26-04-x64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| ubuntu-26-04-arm64 | Ubuntu 26.04.1 LTS (26.04) | arm64 | 7.0.0-38-generic | 1.6.2 | 0.5.13 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/40104329016 |
| kali-rolling-x64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| kali-rolling-arm64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| arch-rolling-x64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |

## Scenario results

| Environment | PKG-01 | APP-01 | GRAPH-02 | AUDIO-01 | REC-02 | GRAPH-01 | AUDIO-11 | UPGRADE-01 | PKG-02 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-arm64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| macos-15-arm64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| macos-15-x64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| macos-26-arm64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| macos-26-x64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| macos-27-arm64 | not run | not run | not run | not run | not run | not run | not run | not run | not run |
| debian-13-x64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |
| debian-13-arm64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |
| ubuntu-26-04-x64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |
| ubuntu-26-04-arm64 | passed | passed | passed | passed | passed | failed | not applicable | failed | passed |
| kali-rolling-x64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |
| kali-rolling-arm64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |
| arch-rolling-x64 | not run | not run | not run | not run | not run | not run | not applicable | not run | not run |

## Failure and blocking diagnostics

Full diagnostics remain in the per-target qualification JSON and raw Actions evidence.

- ubuntu-26-04-arm64 / GRAPH-01 (failed): Installed UI runner failed: Observed installed menu expansion: Add node Observed installed submenu expansion: Processor (1 Right Arrow submissions) Submitted installed UI control: Compressor (16.86s) Observed installed menu expansion: Add node Observed installed submenu expansion: Output (1 Right Arrow submissions) Traceback (most recent call last):   File "/home/qa/workspace/scripts/e2e/linux-ui.py", line 302, in <module>     main(folder)     ~~~~^^^^^^^^   File "/home/qa/workspace/scripts/e2e/linux-ui.py", line 277, in main     activate("Recorder")     ~~~~~~~~^^^^^^^^^^^^   File "/home/qa/workspace/scripts/e2e/linux-ui.py", line 165, in activate    …
- ubuntu-26-04-arm64 / UPGRADE-01 (failed): Installed UI runner failed: Observed installed menu expansion: Add node Observed installed submenu expansion: Processor (1 Right Arrow submissions) Submitted installed UI control: Compressor (15.00s) Observed installed menu expansion: Add node Observed installed submenu expansion: Output (1 Right Arrow submissions) Submitted installed UI control: Recorder (3.71s) Submitted installed UI control: Connect channels… (13.58s) Submitted installed UI control: Destination (11.44s) Submitted installed UI control: Recorder (2.81s) Submitted installed UI control: Connect (12.43s) Traceback (most recent call last):   File "/home/qa/workspace/scripts/e2e/linux-ui…

## VM execution diagnostics

| Environment | Accelerator | CPU model | Result | Pinned image checksum |
| --- | --- | --- | --- | --- |
| ubuntu-26-04-arm64 | tcg | cortex-a72 | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 8eb876c2ccc4b6ff89e4a65d511815923b285e04503ba465809bf587f763da24 |
