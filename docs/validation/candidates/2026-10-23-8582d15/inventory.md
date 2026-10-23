# Candidate qualification

Source: 8582d158455c56006e4288146dcc646100c34bb5

Contract: bb3955a41cc0440074570766c5016c9eb30f18772ed80899153e1ba126bb63d6

| Environment | Artifact SHA-256 | Result |
| --- | --- | --- |
| macos-14-2-arm64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-14-2-arm64: missing report; GitHub-hosted runners cannot select macOS 14.2; macOS 14 Intel is also unavailable. |
| macos-14-2-x64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-14-2-x64: missing report; GitHub-hosted runners cannot select macOS 14.2; macOS 14 Intel is also unavailable. |
| macos-14-arm64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-14-arm64: GRAPH-02: blocked |
| macos-15-arm64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-15-arm64: GRAPH-02: blocked |
| macos-15-x64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-15-x64: GRAPH-02: blocked |
| macos-26-arm64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-26-arm64: GRAPH-02: blocked |
| macos-26-x64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-26-x64: GRAPH-02: blocked |
| macos-27-arm64 | 926023b29a72a6b4e86386d32e1c7b21458cf41464eb01aa7f32c55dd40fea97 | macos-27-arm64: GRAPH-02: blocked |
| debian-13-x64 | 9b84bdaa85884576470fb87d94b2f6fc244819dd021d8cf0848822b329a20c4a | debian-13-x64: APP-01: failed |
| debian-13-arm64 | d46de76461f7f77677ef8ebf3baf84e3dce4ef836b6461865d0e484b1ccc1d12 | debian-13-arm64: APP-01: failed |
| ubuntu-26-04-x64 | 9b84bdaa85884576470fb87d94b2f6fc244819dd021d8cf0848822b329a20c4a | ubuntu-26-04-x64: APP-01: failed |
| ubuntu-26-04-arm64 | d46de76461f7f77677ef8ebf3baf84e3dce4ef836b6461865d0e484b1ccc1d12 | ubuntu-26-04-arm64: missing report |
| kali-rolling-x64 | 9b84bdaa85884576470fb87d94b2f6fc244819dd021d8cf0848822b329a20c4a | kali-rolling-x64: APP-01: failed |
| kali-rolling-arm64 | d46de76461f7f77677ef8ebf3baf84e3dce4ef836b6461865d0e484b1ccc1d12 | kali-rolling-arm64: APP-01: failed |
| arch-rolling-x64 | 03d02a54646802db21f6d4ea87444619a3533656cf9f3ba8cb5a11256386b02d | arch-rolling-x64: APP-01: failed |

## Observed installed environments

| Environment | OS version | CPU | Kernel | PipeWire | WirePlumber | glibc | Run |
| --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-2-arm64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-14-2-x64 | not observed | not observed | not observed | not applicable | not applicable | not applicable | not observed |
| macos-14-arm64 | 14.8.9 | arm64 | 23.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| macos-15-arm64 | 15.7.9 | arm64 | 24.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| macos-15-x64 | 15.7.9 | x64 | 24.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| macos-26-arm64 | 26.6.2 | arm64 | 25.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| macos-26-x64 | 26.6.1 | x64 | 25.6.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| macos-27-arm64 | 27.0.1 | arm64 | 27.0.0 | not applicable | not applicable | not applicable | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| debian-13-x64 | Debian GNU/Linux 13 (trixie) (13.7) | x64 | 6.12.113+deb13-cloud-amd64 | 1.4.2 | 0.5.8 | 2.41 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| debian-13-arm64 | Debian GNU/Linux 13 (trixie) (13.7) | arm64 | 6.12.113+deb13-cloud-arm64 | 1.4.2 | 0.5.8 | 2.41 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| ubuntu-26-04-x64 | Ubuntu 26.04.1 LTS (26.04) | x64 | 7.0.0-38-generic | 1.6.2 | 0.5.13 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| ubuntu-26-04-arm64 | not observed | not observed | not observed | not observed | not observed | not observed | not observed |
| kali-rolling-x64 | Kali GNU/Linux Rolling (2026.1) | x64 | 6.18.12+kali-cloud-amd64 | 1.6.8 | 0.5.15 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| kali-rolling-arm64 | Kali GNU/Linux Rolling (2026.1) | arm64 | 6.18.12+kali-cloud-arm64 | 1.6.8 | 0.5.15 | 2.43 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |
| arch-rolling-x64 | Arch Linux (rolling) | x64 | 7.2.9-arch1-1 | 1.6.10 | 0.5.18 | 2.44 | https://github.com/mixrelay/mixrelay/actions/runs/39945522752 |

## Scenario results

| Environment | PKG-01 | APP-01 | GRAPH-02 | AUDIO-01 | REC-02 | GRAPH-01 | AUDIO-11 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| macos-14-2-arm64 | not run | not run | not run | not run | not run | not run | not run |
| macos-14-2-x64 | not run | not run | not run | not run | not run | not run | not run |
| macos-14-arm64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| macos-15-arm64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| macos-15-x64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| macos-26-arm64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| macos-26-x64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| macos-27-arm64 | passed | passed | blocked | failed | blocked | blocked | blocked |
| debian-13-x64 | passed | failed | failed | blocked | blocked | blocked | not applicable |
| debian-13-arm64 | passed | failed | passed | passed | passed | blocked | not applicable |
| ubuntu-26-04-x64 | passed | failed | passed | passed | passed | blocked | not applicable |
| ubuntu-26-04-arm64 | not run | not run | not run | not run | not run | not run | not applicable |
| kali-rolling-x64 | passed | failed | blocked | blocked | blocked | blocked | not applicable |
| kali-rolling-arm64 | passed | failed | passed | failed | failed | blocked | not applicable |
| arch-rolling-x64 | passed | failed | passed | passed | passed | blocked | not applicable |

## Failure and blocking diagnostics

Full diagnostics remain in the per-target qualification JSON and raw Actions evidence.

- arch-rolling-x64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"virtualEndpointSettings":{"canCreate":false,"canRename":false,"renderChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64],"captureChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,1…
- arch-rolling-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- debian-13-arm64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"virtualEndpointSettings":{"canCreate":false,"canRename":false,"renderChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64],"captureChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17…
- debian-13-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- debian-13-x64: /api/v1/graphs/graph-1/recorders/qa-recorder: HTTP 409 {"code":"graph_exchange_busy","parameters":{"reason":"graph mutation in progress"}}
- debian-13-x64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- debian-13-x64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"virtualEndpointSettings":{"canCreate":false,"canRename":false,"renderChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64],"captureChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,1…
- debian-13-x64 / GRAPH-02 (failed): /api/v1/graph: HTTP 422 {"code":"graph_invalid","parameters":{"reason":"host audio backend failure: Driver HTTP: error sending request for url (http://mixrelay-driver/api/v1/driver)"}}
- debian-13-x64 / AUDIO-01 (blocked): A prerequisite failed
- debian-13-x64 / REC-02 (blocked): A prerequisite failed
- debian-13-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- kali-rolling-arm64 cleanup: /api/v1/graphs/graph-1/recorders/qa-recorder: HTTP 409 {"code":"graph_exchange_busy","parameters":{"reason":"graph mutation in progress"}}
- kali-rolling-arm64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"virtualEndpointSettings":{"canCreate":false,"canRename":false,"renderChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64],"captureChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16…
- kali-rolling-arm64 / AUDIO-01 (failed): Native loopback changed channel order or level
- kali-rolling-arm64 / REC-02 (failed): /api/v1/graphs/graph-1/recorders/qa-recorder: HTTP 409 {"code":"graph_exchange_busy","parameters":{"reason":"graph mutation in progress"}}
- kali-rolling-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- kali-rolling-x64: /api/v1/driver: HTTP 503 {"error":"Driver unavailable: Driver control queue is full"}
- kali-rolling-x64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"processingState":"running","discontinuities":0,"format":{"sampleRate":48000,"blockFrames":128},"preferredFormat":null,"clockAuthority":"audio_server","observedDeviceSampleRate":null,"issue":null,"transportLatencyFrames":null,"capabilities":{"backend":"pipe_wire","supportedSampleRates":[44100,48000,88200,96000,176400,19200…
- kali-rolling-x64 / GRAPH-02 (blocked): A prerequisite failed
- kali-rolling-x64 / AUDIO-01 (blocked): A prerequisite failed
- kali-rolling-x64 / REC-02 (blocked): A prerequisite failed
- kali-rolling-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-14-arm64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-14-arm64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- macos-14-arm64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-14-arm64 / GRAPH-02 (blocked): A prerequisite failed
- macos-14-arm64 / REC-02 (blocked): A prerequisite failed
- macos-14-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-14-arm64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- macos-15-arm64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-15-arm64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- macos-15-arm64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-15-arm64 / GRAPH-02 (blocked): A prerequisite failed
- macos-15-arm64 / REC-02 (blocked): A prerequisite failed
- macos-15-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-15-arm64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- macos-15-x64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-15-x64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- macos-15-x64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-15-x64 / GRAPH-02 (blocked): A prerequisite failed
- macos-15-x64 / REC-02 (blocked): A prerequisite failed
- macos-15-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-15-x64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- macos-26-arm64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-26-arm64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- macos-26-arm64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-26-arm64 / GRAPH-02 (blocked): A prerequisite failed
- macos-26-arm64 / REC-02 (blocked): A prerequisite failed
- macos-26-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-26-arm64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- macos-26-x64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-26-x64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false  Command failed: /Applications/MixRelay/Uninstall MixRelay.app/Contents/Helpers/mixrelay-macos-installer quit-mixrelay-runtime ERROR MixRelay installation failed: quit-mixrelay-runtime did not complete Quit (pid 7608)
- macos-26-x64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-26-x64 / GRAPH-02 (blocked): A prerequisite failed
- macos-26-x64 / REC-02 (blocked): A prerequisite failed
- macos-26-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-26-x64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- macos-27-arm64: /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-27-arm64 cleanup: Unconfirmed graph publication; reset the QA session  true !== false
- macos-27-arm64 / AUDIO-01 (failed): /api/v1/graph: HTTP 503 {"code":"audio_device_disconnected","parameters":{"reason":"host audio backend failure: CoreAudio device initialization failed: Err(Timeout)"}}
- macos-27-arm64 / GRAPH-02 (blocked): A prerequisite failed
- macos-27-arm64 / REC-02 (blocked): A prerequisite failed
- macos-27-arm64 / GRAPH-01 (blocked): Installed UI automation is not implemented
- macos-27-arm64 / AUDIO-11 (blocked): Whole-system capture adapter and native acceptance are not implemented
- ubuntu-26-04-x64 / APP-01 (failed): Installed engine/Driver not ready: {"enginePower":{"enabled":true,"transitioning":false},"protocolVersion":1,"driverState":"not_installed","sampleRate":48000,"bufferFrames":128,"underruns":0,"overruns":0,"sampleFaults":{"sourceSamples":0,"inputSamples":0,"outputSamples":0,"blocks":0},"audioSession":{"endpointPreferencesUnavailable":false,"virtualEndpointSettings":{"canCreate":false,"canRename":false,"renderChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64],"captureChannels":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,1…
- ubuntu-26-04-x64 / GRAPH-01 (blocked): Installed UI automation is not implemented

## VM execution diagnostics

| Environment | Accelerator | CPU model | Result | Pinned image checksum |
| --- | --- | --- | --- | --- |
| arch-rolling-x64 | kvm | host | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 08754e28d91b1680086c6211ef4df13559e766fff7cdb4006489b14b5d281f08 |
| debian-13-arm64 | tcg | cortex-a72 | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 0199ea72dd82e84264fda5068c5756412c778f395fca6748a186f2d0a34f524fe406e87339bda0b7f71aa9bab066b9850cde44cd1650116f389dc3f786cb232b |
| debian-13-x64 | kvm | host | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | ec584ccabfef7120b4c4f7482adf0e615894d9292cedec446b64194b951ebe5dd97c336b5dcbae458333764b0edd516334506017f33c4f2020ef8d38b569ffe3 |
| kali-rolling-arm64 | tcg | cortex-a72 | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 9ab19c28d049fdc6f1d5d30f6cc93b8b01997f11c89f6992f690c07b16b7b7e4 |
| kali-rolling-x64 | kvm | host | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 3f4ca86ae1eca0dc2bf2092f065a959508e186c6b44f85758813a8e9a0604f3e |
| ubuntu-26-04-arm64 | tcg | cortex-a72 | ssh exited null; see /home/runner/work/mixrelay/mixrelay/evidence/guest-bootstrap.log | 8eb876c2ccc4b6ff89e4a65d511815923b285e04503ba465809bf587f763da24 |
| ubuntu-26-04-x64 | kvm | host | Guest suite failed; see guest-suite.log and qualification report  1 !== 0  | 5645d0318ad21f643ebb1ac4567514620e9893380b07b0201fda163baa5b7eb5 |
