# Footer and processor menu checks

- Background workspace saves no longer disable the New Graph launcher. Opening
  a local name dialog does not mutate the catalog; its submission remains blocked
  during pending writes or unsaved titles. Mod+N follows the same rule.
- Same deterministic test, 20 busy/idle cycles: before the fix the launcher had
  40 disabled-attribute transitions; afterward it had 0. This counts the UI state
  changes causing the dimming, not GPU frames, rendering throughput or all possible
  sources of flicker. No timing delays or CSS fade concealment were added.
- The footer directly selects the processing buffer, without an information popup
  or a normal-state Apply button. Selection submits once; native request ownership,
  cancellation boundaries, supported-value validation and busy guards remain.
  Same-buffer failures expose retry, and request/session/persistence errors share
  one stacked alert. The previous manual draft/apply path was removed.
- Buffer delay is one block duration (block samples / processing sample rate), not
  measured total input/output latency. The live 48 kHz / 32 configuration displays
  0.67 ms. A tooltip explains the distinction; device clock rates are not substituted
  for the processing format. Invalid rate/size values show an unavailable mark.
- Plug-ins now appear under format submenus AU, AUv3, VST3, CLAP and LV2 when
  discovered, with alphabetically sorted leaves and unchanged insertion identities.
  Built-in compressor/limiter functionality was not removed without a final choice.
- Focused UI suites: 78 tests passed; TypeScript, UI boundary checks and focused
  Biome passed. A read-only reviewer identified and rechecked same-buffer retry and
  overlapping-error issues; regression tests cover both corrections.
- Desktop debug build succeeded. Existing Vite bundle-size warning remains.
  Only the desktop UI was restarted, not the engine/HAL/CoreAudio. Native UI
  inspection confirmed the inline 32 setting, 0.67 ms display and working
  Processor -> AU -> installed plug-in menu. Other format grouping/leaf identities
  were verified in tests; no unavailable plug-ins were installed for this check.
