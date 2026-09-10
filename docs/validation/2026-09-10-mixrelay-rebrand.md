# MixRelay identity cutover

## Scope

- Workspace: `/Volumes/dev/MixRelay`.
- Repository: `https://github.com/mixrelay/mixrelay`, private repository ID `1354366829`.
- Repository transfer preserved its identity and history. The empty previous organization
  was deleted after checking repositories, packages, projects, teams, app installations,
  Actions secret/variable names, and webhooks. Deletion was accepted and the subsequent
  organization lookup returned 404; the destination repository remained accessible.
- Product, Cargo/npm names, environment variables, native symbols, transport identifiers,
  bundle IDs, launch services, build/install scripts, tests, and documentation use MixRelay.
- No runtime compatibility aliases or old-identity fallback paths were added. This is an
  identity change, not a DSP optimization; no performance improvement is claimed.

## Validation

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- Workspace documentation tests: 4 passed; `cargo doc --workspace --no-deps` passed.
- UI Biome check and dependency-boundary check: passed.
- UI typecheck and production build: passed, with the existing bundle-size warning.
- UI tests: 57 files / 690 tests passed. The test command still exits unsuccessfully on
  the 100% coverage gate: lines 90.09%, functions 88.19%, statements 87.40%, branches 80.36%.
  The threshold was not reduced.
- Workspace library run: audio-engine 36, audio-graph 26, audio-io 58, audio-recorder 32,
  control-protocol 7, control-server 17, engine-service 219, and HAL core 49 tests passed.
  macOS test-support passed 15 tests and failed one native-device fixture because
  `MIXRELAY_TEST_ENDPOINT_UID` was not supplied. The run stopped at that crate.
- Separately executed library tests: PCM ring 10, process tap 23, processor host 60, and
  macOS transport 136 tests passed.
- The five Windows transport-contract tests passed within engine-service's library run.
  A binary-target filter matched zero tests and is not counted as validation.
- Windows bridge script checks: 7 passed, 2 failed on stale fixed-rate source assertions.
  Both the pre-change HEAD and current probe use `sampleRate` / `sampleRate / 2`; the
  assertions still expect 48000 / 24000. No Windows runtime qualification is claimed.
- Independent read-only review found and verified corrections for one underscored Windows
  transport magic and remaining PascalCase native prefixes. The graph fixture's expected
  deterministic ordering was updated for the renamed node ID.
- `git diff --check`: passed. Agent-only instructions remain ignored and unstaged.

## Local data and installation

- Graphs, audio settings, endpoint settings, preferences, WebKit data, caches, and logs
  were moved to the new identity's locations while the app and engine were stopped.
- Graph file SHA-256 before and immediately after the move:
  `87de6495e6abf3c9ecd469ffcd4be6f55b5e930355aff5d05c61ce29e1b4ac09`.
- Audio-session file SHA-256 before and after the move:
  `021fc14bb72de190606c9d1bd2d1e0743914a34ac50882a444d76d5976ee15e3`.
- The default endpoint label was changed to `MixRelay IO`; custom graph labels and
  recording destinations were preserved. The inspected app WebKit stores contained
  no custom processor-path localStorage entry requiring a key rewrite.
- Built the debug desktop app and engine, plus the arm64/x86_64 universal HAL bundle.
  HAL factory/COM probe passed. App and driver signatures passed local ad-hoc verification.
  This does not establish distribution signing or notarization.
- Installed `/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver` and
  `/Library/LaunchDaemons/io.mixrelay.audio.engine.plist`. The old active app bundle,
  HAL bundle, and launch plist were moved to Trash, not irreversibly erased.
- Built and installed HAL executable SHA-256 matched:
  `c47e79702be2a71f1dd6b200cbb588773f4acf9052424ba4defd902b9c9fb9b7`.
- Running app: `/Volumes/dev/MixRelay/target/debug/bundle/macos/MixRelay.app`, bundle ID
  `io.mixrelay.desktop`; service binary is `target/debug/mixrelay-engine-service`.
- Native UI showed all three existing graphs and the recorder. Engine status reported
  ready/running at 48 kHz / 64 frames, no underruns or overruns. Microphone PCM reached both
  recorder channels (observed peaks 0.0707713), while the unplugged headphone node remained
  offline. No headphone listening test or recording-file write was performed.
- Startup logs showed format admission gating before 48 kHz activation, then repeated
  five-second windows with zero missed callbacks and zero over-budget callbacks.

## Limits

Linux and Windows builds/runtime were not exercised on their target OS. Native tests
requiring explicitly selected physical/test devices remain unqualified; existing coverage
and Windows script-test failures remain visible above. Git history, previous diagnostic
logs, and recoverable build/installation artifacts were not rewritten as product source.
