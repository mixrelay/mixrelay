import { copyFile, cp, mkdir, rm, writeFile } from 'node:fs/promises';
import { build } from 'esbuild';

const output = 'dist/io.mixrelay.streamdeck.sdPlugin';
await rm(output, { recursive: true, force: true });
await mkdir(`${output}/images`, { recursive: true });
await build({
  entryPoints: ['src/plugin.mjs'],
  outfile: `${output}/bin/plugin.js`,
  bundle: true,
  platform: 'node',
  target: 'node20',
  format: 'esm',
  banner: {
    js: "import { createRequire } from 'node:module'; const require = createRequire(import.meta.url);",
  },
});
await copyFile('manifest.json', `${output}/manifest.json`);
await copyFile('.sdignore', `${output}/.sdignore`);
await writeFile(`${output}/package.json`, '{"type":"module"}\n');
await cp('ui', `${output}/ui`, { recursive: true });
await build({
  entryPoints: ['ui/inspector.js'],
  outfile: `${output}/ui/inspector.js`,
  bundle: true,
  platform: 'browser',
  format: 'iife',
});
await copyFile('../../branding/tray-white.svg', `${output}/images/icon.svg`);
await copyFile('../desktop/src-tauri/icons/128x128.png', `${output}/images/app.png`);
await copyFile('../desktop/src-tauri/icons/128x128@2x.png', `${output}/images/app@2x.png`);
