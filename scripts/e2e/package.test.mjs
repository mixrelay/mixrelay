import assert from 'node:assert/strict';
import test from 'node:test';
import { installedPaths, requireDisposableHost } from './package.mjs';

test('installation refuses an unacknowledged host before any installer or privilege command runs', () => {
  assert.throws(() => requireDisposableHost('macos', false), /disposable-environment/);
  assert.throws(() => requireDisposableHost('linux', false), /disposable-environment/);
  assert.ok(installedPaths('macos').every(path => path.startsWith('/')));
  assert.ok(installedPaths('linux').every(path => path.startsWith('/usr/bin/')));
});
