import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { inspectCss, inspectScript } from './ui-boundary-rules.mjs';

const sourceRoot = path.resolve(import.meta.dirname, '../src');
const extensions = new Set(['.ts', '.tsx', '.css']);

async function collect(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await collect(filename)));
    else if (extensions.has(path.extname(filename))) files.push(filename);
  }
  return files;
}

const files = await collect(sourceRoot);
const cssFiles = new Map();
const layouts = new Map();
const violations = [];
function report(filename, items) {
  for (const item of items)
    violations.push(
      `${path.relative(sourceRoot, filename)}:${item.line} [${item.code}] ${item.message}`,
    );
}

for (const filename of files) {
  const source = await readFile(filename, 'utf8');
  const relative = path.relative(sourceRoot, filename);
  if (filename.endsWith('.css')) {
    cssFiles.set(filename, source);
    continue;
  }
  const result = inspectScript(relative, source);
  report(filename, result.violations);
  for (const binding of result.layoutBindings) {
    const cssFile = path.resolve(path.dirname(filename), binding.source);
    if (!layouts.has(cssFile)) layouts.set(cssFile, new Set());
    layouts.get(cssFile).add(binding.name);
  }
}
for (const [filename, source] of cssFiles) {
  report(filename, inspectCss(path.relative(sourceRoot, filename), source, layouts.get(filename)));
}
if (violations.length) {
  console.error('UI system boundary check failed. Update tokens / theme / Core before consumers:');
  for (const violation of violations) console.error(`- ${violation}`);
  process.exitCode = 1;
} else {
  console.log(
    'UI system boundary check passed: shared appearance is owned by tokens / theme / Core.',
  );
}
