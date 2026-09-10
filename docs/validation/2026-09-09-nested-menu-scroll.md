# Nested menu scroll boundary

## Defect and change

The Source list exceeds the menu height limit. Its next-level virtual-device
dropdown was physically inside that scrolling dropdown. The live app exposed
`New virtual device` and `MixRelay Stereo` to accessibility/keyboard navigation
even when the user screenshot showed no child panel: this was not an empty
inventory or missing creation action.

Each menu now has a scrolling list and a sibling submenu portal host. Child
panels remain DOM descendants of their owning menu (Mantine outside-click and
keyboard scope), but are outside every ancestor scrolling list. The shared
component applies to context menus and the Add button, including processor
format submenus. Item sizing/styles and stable selection IDs remain shared.
The Korean category is now `가상 장치`; actual device names and IDs are unchanged.

## Validation

- 42 tests passed across HierarchicalMenu, processorCatalogMenu, and
  RoutingWorkspace; no skipped tests. Nested mouse/keyboard tests use a long
  30-application list and assert both escape from scroll ancestors and continued
  containment by the owning menu, then exactly one leaf selection and closure.
- TypeScript, UI dependency boundaries, and changed-file Biome checks passed.
- Desktop debug app build succeeded and was launched locally. Existing Vite
  >500 kB chunk warning remains; this is not a warning-free build claim.
- Native app: Source → 가상 장치 → 새 가상 장치 clicked through to the creation
  dialog. The existing MixRelay Stereo reference was also present.
- Native app: repeated from Output near the bottom-right window edge; creation
  dialog opened. Already-connected MixRelay Stereo was disabled as expected.
- Native app: Add button → Source → 가상 장치 navigated using arrow keys and
  Return opened the creation dialog. Dialogs were cancelled without creating
  devices. Engine, 32-sample setting, and user graphs were not changed.
- Independent read-only source review found no additional defect in portal
  ownership, disposal, keyboard scoping, or selection handling.

The native automation exposed open menus through accessibility but returned
`Screenshot unavailable` for a direct open-menu capture. Dialog screenshots and
actual click/keyboard actions were verified; no pixel capture proof of the full
hover corridor or exhaustive scrolling/viewport combinations is claimed.
This is a correctness/layout change, not a latency or throughput optimization.
