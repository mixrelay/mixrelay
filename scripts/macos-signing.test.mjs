import assert from 'node:assert/strict';
import test from 'node:test';
import { RETIRED_SIGNING_KEYCHAIN, DEFAULT_SIGNING_CONFIG, usableKeychainSearchList } from './macos-signing.mjs';
import { dirname, join } from 'node:path';

test('retired release lookup is removed without removing other identities or restored keys', () => {
  const login = '/Library/Keychains/unrelated-login.keychain-db';
  const restored = join(dirname(DEFAULT_SIGNING_CONFIG), 'release.keychain-db');
  const unrelatedRelease = '/another/driver-20261008/release.keychain-db';
  const original = [login, RETIRED_SIGNING_KEYCHAIN, restored, unrelatedRelease];
  assert.deepEqual(usableKeychainSearchList(original), [login, restored, unrelatedRelease]);
  assert.equal(original.length, 4);
});

test('a clean search list stays stable across subsequent builds', () => {
  const original = ['/Library/Keychains/login.keychain-db'];
  assert.deepEqual(usableKeychainSearchList(usableKeychainSearchList(original)), original);
});
