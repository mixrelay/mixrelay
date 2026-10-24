import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

// The static site ships independently. Its font assets are generated from Core.
const root = path.resolve(import.meta.dirname, '..');
const core = path.join(root, 'apps/control-ui/src/core/theme');
const output = path.join(root, 'apps/website/assets');
await mkdir(path.join(output, 'fonts'), { recursive: true });
for (const name of await readdir(path.join(core, 'fonts'))) {
  await copyFile(path.join(core, 'fonts', name), path.join(output, 'fonts', name));
}
const tokens = await readFile(path.join(core, 'tokens.ts'), 'utf8');
const family = (name) => tokens.match(new RegExp(`export const ${name} =\\s*'([^']+)'`))[1];
await writeFile(
  path.join(output, 'Fonts.css'),
  `${await readFile(path.join(core, 'Fonts.css'), 'utf8')}\n:root { --font-sans: ${family('fontFamily')}; --font-mono: ${family('monoFontFamily')}; }\n`,
);
console.log('Website font assets synced from Core.');
