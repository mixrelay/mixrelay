import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const sourceRoot = path.resolve(import.meta.dirname, '../src');
const coreRoot = path.join(sourceRoot, 'core');
const sourceExtensions = new Set(['.ts', '.tsx']);
const violations = [];

async function visit(directory) {
  const entries = await readdir(directory, { withFileTypes: true });

  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);

    if (entry.isDirectory()) {
      await visit(entryPath);
      continue;
    }

    if (!sourceExtensions.has(path.extname(entry.name)) || entryPath.startsWith(coreRoot)) {
      continue;
    }

    const source = await readFile(entryPath, 'utf8');
    if (/from\s+['"]@mantine\//u.test(source) || /import\s+['"]@mantine\//u.test(source)) {
      violations.push(path.relative(sourceRoot, entryPath));
    }
  }
}

await visit(sourceRoot);

if (violations.length > 0) {
  console.error('Mantine imports are only allowed under src/core:');
  for (const violation of violations) console.error(`- ${violation}`);
  process.exitCode = 1;
} else {
  console.log('UI boundary check passed: Mantine is isolated behind Core.');
}
