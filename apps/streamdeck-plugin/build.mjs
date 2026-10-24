import { copyFile, cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { build } from 'esbuild';

const output = 'dist/io.mixrelay.streamdeck.sdPlugin';
await rm(output, { recursive: true, force: true });
await mkdir(`${output}/images`, { recursive: true });
for (const action of ['power', 'mute', 'volume', 'recording']) {
  await copyFile(`images/${action}.svg`, `${output}/images/${action}.svg`);
  const icon = (await readFile(`images/${action}.svg`, 'utf8'))
    .replace('<svg ', '<svg x="26" y="3" ')
    .replace('width="24" height="24"', 'width="20" height="20"');
  await writeFile(
    `${output}/images/key-${action}.svg`,
    `<svg xmlns="http://www.w3.org/2000/svg" width="72" height="72" viewBox="0 0 72 72"><rect width="72" height="72" fill="#191e20"/>${icon}</svg>`,
  );
}
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
await copyFile('ko.json', `${output}/ko.json`);
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
