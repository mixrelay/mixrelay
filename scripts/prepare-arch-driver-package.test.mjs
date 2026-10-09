import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { renderArchRecipe } from './prepare-arch-driver-package.mjs';

test('Arch recipe pins the checked payload and keeps the release binaries intact', () => {
  for (const [architecture, arch] of [['amd64', 'x86_64'], ['arm64', 'aarch64']]) {
    const recipe = renderArchRecipe({ version: '0.1.0', architecture, checksum: 'a'.repeat(64) });
    assert.ok(recipe.includes(`arch=('${arch}')`));
    assert.ok(recipe.includes(`sha256sums=('${'a'.repeat(64)}')`));
    assert.match(recipe, /options=\('!strip' '!debug'\)/);
    assert.match(recipe, /glibc>=2\.39/);
    assert.doesNotMatch(recipe, /@VERSION@|@ARCH@|@SHA256@/);
  }
});

test('Arch recipe rejects malformed substitutions before shell execution', () => {
  const valid = { version: '0.1.0', architecture: 'amd64', checksum: 'a'.repeat(64) };
  for (const invalid of [
    { version: "0.1.0'; touch injected; '" },
    { version: '../0.1.0' }, { architecture: 'i686' }, { checksum: 'SKIP' },
  ]) assert.throws(() => renderArchRecipe({ ...valid, ...invalid }));
});

test('Arch aborts before replacing/removing an active Driver and scopes the hook to its package', () => {
  const hook = readFileSync(new URL('../packaging/linux/arch/mixrelay-driver.hook', import.meta.url), 'utf8');
  assert.match(hook, /^Operation = Upgrade$/m);
  assert.match(hook, /^Operation = Remove$/m);
  assert.match(hook, /^Target = mix-relay-driver$/m);
  assert.match(hook, /^When = PreTransaction$/m);
  assert.match(hook, /^AbortOnFail$/m);
  assert.doesNotMatch(hook, /^Operation = Install$/m);
});
