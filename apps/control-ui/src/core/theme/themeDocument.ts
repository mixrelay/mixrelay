import { themeCss } from './theme';
import { themeStorageKey } from './themePreference';

/** Runs before first paint: applies the saved preference so a choice that differs from
 * the OS appearance never flashes the OS theme while the bundle loads. */
export const themeBootstrapScript = `(()=>{let t='system';try{const v=localStorage.getItem(${JSON.stringify(
  themeStorageKey,
)});if(v==='light'||v==='dark')t=v}catch{}document.documentElement.setAttribute('data-theme',t)})()`;

/** Tags for the built document <head>: the token CSS and the bootstrap, so the first
 * paint already has the theme. MixRelayProvider keeps owning later changes. */
export function themeDocumentTags() {
  return [
    {
      tag: 'style',
      attrs: { 'data-mixrelay-theme': 'document' },
      children: themeCss,
      injectTo: 'head-prepend' as const,
    },
    { tag: 'script', children: themeBootstrapScript, injectTo: 'head-prepend' as const },
  ];
}
