import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

function run(program, args, options = {}) {
  const result = spawnSync(program, args, { encoding: 'utf8', timeout: 30000, ...options });
  assert.ifError(result.error);
  assert.equal(result.signal, null, result.stderr);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

function buildPolicy(t) {
  const directory = mkdtempSync(join(tmpdir(), 'mixrelay-dbus-link-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const source = readFileSync(new URL('../apps/desktop/src-tauri/build.rs', import.meta.url), 'utf8');
  const main = source.slice(0, source.indexOf('fn build_macos_audio('));
  assert.match(main, /fn main\(\)/);
  // Run the actual build-script entry point; the native macOS and Tauri hooks
  // do not participate in the ELF link policy exercised here.
  writeFileSync(join(directory, 'build.rs'), `${main}
fn build_macos_audio(_: &str) {}
mod tauri_build { pub fn build() {} }
`);
  run('rustc', ['--edition=2024', join(directory, 'build.rs'), '-o', join(directory, 'build')]);
  const args = (os) => run(join(directory, 'build'), [], {
    env: {
      ...process.env,
      CARGO_CFG_TARGET_OS: os,
      CARGO_MANIFEST_DIR: fileURLToPath(new URL('../apps/desktop/src-tauri/', import.meta.url)),
    },
  }).split('\n').filter((line) => line.startsWith('cargo:rustc-link-arg='))
    .map((line) => line.slice('cargo:rustc-link-arg='.length));
  return { directory, args };
}

test('desktop build isolates private D-Bus symbols only for ELF targets', (t) => {
  const policy = buildPolicy(t);
  assert.deepEqual(policy.args('linux'), [
    `-Wl,--version-script=${fileURLToPath(new URL('../apps/desktop/src-tauri/dbus-symbols.map', import.meta.url))}`,
  ]);
  assert.deepEqual(policy.args('macos'), []);
});

test('private D-Bus cannot interpose a dynamically loaded accessibility library', {
  skip: process.platform !== 'linux' && 'Requires the Linux ELF linker/loader; exercised on Linux CI',
}, (t) => {
  const { directory, args } = buildPolicy(t);
  writeFileSync(join(directory, 'private.c'), `
int dbus_connection_get_unix_fd(void) { return 11; }
int _dbus_private_state(void) { return 13; }
`);
  writeFileSync(join(directory, 'system.c'), `
int dbus_connection_get_unix_fd(void) { return 22; }
int _dbus_private_state(void) { return 24; }
int accessibility_value(void) {
    return dbus_connection_get_unix_fd() == 22 && _dbus_private_state() == 24;
}
`);
  writeFileSync(join(directory, 'app.c'), `
#include <dlfcn.h>
int dbus_connection_get_unix_fd(void);
int _dbus_private_state(void);
int main(void) {
    if (dbus_connection_get_unix_fd() != 11 || _dbus_private_state() != 13) return 2;
    void *library = dlopen("./system.so", RTLD_NOW | RTLD_LOCAL);
    if (!library) return 3;
    int (*value)(void) = dlsym(library, "accessibility_value");
    if (!value) return 4;
    return value() ? 0 : 1;
}
`);
  const options = { cwd: directory };
  run('cc', ['-c', 'private.c', '-o', 'private.o'], options);
  // libdbus-sys bundles libdbus.a's objects inside its Rust archive.
  run('ar', ['rcs', 'liblibdbus_sys-fixture.rlib', 'private.o'], options);
  run('cc', ['-fPIC', '-shared', 'system.c', '-o', 'system.so'], options);
  const link = ['-fuse-ld=bfd', '-rdynamic', 'app.c', 'liblibdbus_sys-fixture.rlib', '-ldl'];
  run('cc', [...link, '-o', 'unisolated'], options);
  const broken = spawnSync('./unisolated', [], options);
  assert.ifError(broken.error);
  assert.equal(broken.signal, null);
  assert.equal(broken.status, 1, 'The control must reproduce cross-library D-Bus interposition');
  run('cc', [...link, ...args('linux'), '-o', 'isolated'], options);
  run('./isolated', [], options);
});
