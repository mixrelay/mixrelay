/** `Mod` is the primary application modifier, not a literal platform key. */
export function formatPlatformShortcut(shortcut: string, platform = navigator.platform): string {
  const modifier = /Mac|iPhone|iPad|iPod/i.test(platform) ? '⌘' : 'Ctrl+';
  return shortcut.replace(/\bMod\+/g, modifier);
}
