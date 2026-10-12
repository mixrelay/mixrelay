import assert from 'node:assert/strict';
import { readFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { prepareDriverPackage } from './prepare-driver-package.mjs';
import { validatePackage } from './check-linux-package.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = path => readFileSync(resolve(root, path), 'utf8');
const config = JSON.parse(read('scripts/fixtures/legacy-driver-linux.json'));

test('Driver Linux package owns one manager, service and user-session activation contract', () => {
  assert.equal(config.mainBinaryName, 'mixrelay-driver-manager');
  assert.deepEqual(config.bundle.externalBin, ['binaries/mixrelay-driver-service', 'binaries/mixrelay-engine-service']);
  for (const format of ['deb', 'rpm']) {
    const files = config.bundle.linux[format].files;
    assert.ok(files['/usr/lib/systemd/user/mixrelay-driver.service']);
    assert.ok(files['/etc/xdg/autostart/mixrelay-driver.desktop']);
    assert.ok(files['/usr/share/mixrelay-driver/version.json']);
    assert.ok(!Object.keys(files).some(path => path.includes('io.mixrelay.desktop') || path.includes('mixrelay-engine')));
    for (const source of Object.values(files)) {
      if (source === null) continue;
      if (source.includes('target/driver-package')) continue;
      assert.ok(existsSync(resolve(root, 'apps/desktop/src-tauri', source)), source);
    }
  }
  const unit = read('packaging/linux/mixrelay-driver.service');
  assert.match(unit, /^ExecStart=\/usr\/bin\/mixrelay-driver-service$/m);
  assert.doesNotMatch(unit, /User=root|WantedBy=multi-user|mixrelay-engine/);
  assert.match(read('packaging/linux/session-start.sh'), /systemctl --user start mixrelay-driver.service/);
  assert.match(read('packaging/linux/mixrelay-driver-uninstall.desktop'), /^Terminal=true$/m);
  assert.ok(config.bundle.linux.deb.depends.includes('pkexec'));
  assert.ok(config.bundle.linux.deb.depends.includes('xterm | x-terminal-emulator'));
  assert.ok(!config.bundle.linux.deb.depends.includes('polkit-1-auth-agent | lxpolkit'));
  assert.ok(config.bundle.linux.rpm.depends.includes('polkit'));
  assert.ok(config.bundle.linux.rpm.depends.includes('xterm'));
  assert.ok(config.bundle.linux.rpm.depends.includes('libc.so.6(GLIBC_2.39)(64bit)'));
  assert.ok(!config.bundle.linux.rpm.depends.some(dependency => /\s[<>=]/.test(dependency)));
  assert.ok(!config.bundle.linux.rpm.depends.includes('PolicyKit-authentication-agent'));
  assert.match(read('packaging/linux/driver-uninstall.sh'), /FALSE "\$delete_label"/);
  assert.doesNotMatch(read('packaging/linux/driver-uninstall.sh'), /clear-local-license|graphs\.json|rm -rf/);
});

test('Driver removal retries partial installs and preserves settings on absence or authentication failure', t => {
  mkdirSync(resolve(root, 'target'), { recursive: true });
  const directory = mkdtempSync(resolve(root, 'target/linux-driver-uninstall-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const [packageFormat, packageState] of [
    ['deb', 'installed'], ['rpm', 'installed'], ['arch', 'installed'],
    ...['unpacked', 'half-configured', 'half-installed', 'triggers-awaited', 'triggers-pending',
      'not-installed', 'config-files'].map(state => ['deb', state]),
  ]) {
    const removable = packageState !== 'not-installed' && packageState !== 'config-files';
    for (const language of ['en', 'ko']) {
      for (const status of removable ? [0, 1, 100, 126, 127] : [0]) {
        const scenario = `${packageFormat}-${packageState}-${language}-${status}`;
        const fixtureDirectory = join(directory, scenario);
        const userDirectory = join(fixtureDirectory, 'home');
        const settings = join(userDirectory, '.config/mixrelay-driver/drivers.json');
        mkdirSync(dirname(settings), { recursive: true });
        writeFileSync(settings, 'saved Driver settings');
        writeFileSync(`${settings}.stopped`, 'stopped');
        const log = join(fixtureDirectory, 'calls');
        const commands = {
          id: 'printf "1000\\n"',
          'dpkg-query': '[ "$TEST_PACKAGE" = deb ] || exit 1\n[ "$1" = -W ] && [ "$2" = \'-f=${db:Status-Status}\' ] && [ "$3" = mix-relay-driver ] || exit 1\nprintf "%s" "$TEST_PACKAGE_STATE"',
          rpm: '[ "$TEST_PACKAGE" = rpm ]',
          pacman: '[ "$TEST_PACKAGE" = arch ] && [ "$1" = -Q ] && [ "$2" = mix-relay-driver ]',
          zenity: `printf '%s\\n' "$@" >> "$TEST_LOG"
if [ "$1" = --list ]; then
  for value do last=$value; done
  printf '%s\\n' "$last"
fi`,
          pkexec: `printf 'pkexec\\n' >> "$TEST_LOG"
printf '%s\\n' "$@" >> "$TEST_LOG"
case "$TEST_STATUS" in 126|127) exit "$TEST_STATUS" ;; esac
printf 'package-remove\\n' >> "$TEST_LOG"
exit "$TEST_STATUS"`,
        };
        for (const [command, body] of Object.entries(commands)) {
          writeFileSync(join(fixtureDirectory, command), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
        }
        const script = join(fixtureDirectory, 'uninstall.sh');
        writeFileSync(script, read('packaging/linux/driver-uninstall.sh'));
        const result = spawnSync('sh', [script], { encoding: 'utf8', env: {
          ...process.env,
          PATH: `${fixtureDirectory}:${process.env.PATH}`,
          HOME: userDirectory,
          XDG_CONFIG_HOME: '',
          LC_ALL: language === 'ko' ? '' : 'C',
          LC_MESSAGES: '',
          LANG: language === 'ko' ? 'ko_KR.UTF-8' : 'C',
          TEST_LOG: log,
          TEST_PACKAGE: packageFormat,
          TEST_PACKAGE_STATE: packageState,
          TEST_STATUS: String(status),
        } });
        assert.equal(result.status, !removable || status === 100 ? 1 : status, `${scenario}: ${result.stderr}`);
        assert.equal(existsSync(settings), !removable || status !== 0, scenario);
        assert.equal(existsSync(`${settings}.stopped`), !removable || status !== 0, scenario);
        const calls = readFileSync(log, 'utf8');
        assert.match(calls, /FALSE\n/);
        if (!removable) {
          assert.doesNotMatch(calls, /pkexec|package-remove/);
        } else if (packageFormat === 'deb') {
          assert.match(calls, /pkexec\n\/usr\/bin\/apt-get\n-y\nremove\n--\nmix-relay-driver\n/);
        } else if (packageFormat === 'rpm') {
          assert.match(calls, /pkexec\n\/usr\/bin\/dnf\n-y\n--setopt=clean_requirements_on_remove=False\nremove\nmix-relay-driver\n/);
        } else {
          assert.match(calls, /pkexec\n\/usr\/bin\/pacman\n--noconfirm\n-R\n--\nmix-relay-driver\n/);
        }
        if (removable && (status === 126 || status === 127)) {
          assert.doesNotMatch(calls, /package-remove|Quit MixRelay|앱과 Driver 관리자를 종료/);
          assert.match(calls, language === 'ko'
            ? /패키지를 제거하지 않았습니다.*설정은 삭제하지 않았습니다/
            : /package was not removed and saved settings were not deleted/);
          assert.ok(calls.includes(status === 126 ? '--info\n' : '--error\n'), scenario);
          assert.match(calls, status === 126
            ? /Authentication cancelled|인증이 취소/
            : /Authorization unavailable or authentication failed|승인 불가 또는 인증 오류/);
        } else if (!removable || status !== 0) {
          assert.match(calls, language === 'ko'
            ? /MixRelay와 MixRelay Driver를 종료한 뒤 다시 시도/ : /Quit MixRelay and MixRelay Driver, then retry/);
        } else {
          assert.doesNotMatch(calls, /--error|--info/);
        }
      }
    }
  }
});

test('Driver preparation publishes actual matching service/version inputs without graph binaries', () => {
  const copies = [], writes = [], commands = [];
  const fsApi = {
    readFileSync(path) {
      return path.endsWith('Cargo.toml') ? '[workspace.package]\nversion = "1.2.3"\n' : '{"version":"1.2.3"}';
    },
    mkdirSync() {},
    copyFileSync(from, to) { copies.push([from, to]); },
    chmodSync() {},
    writeFileSync(path, text) { writes.push([path, text]); },
  };
  const result = prepareDriverPackage({
    args: ['--target', 'aarch64-unknown-linux-gnu'], rootDir: '/project', env: {}, fsApi,
    run(command, args) { commands.push([command, args]); return { status: 0 }; },
  });
  assert.equal(result.version, '1.2.3');
  assert.deepEqual(JSON.parse(writes[0][1]), { version: '1.2.3' });
  assert.match(copies[0][0], /release\/mixrelay-driver-service$/);
  assert.match(copies[0][1], /binaries\/mixrelay-driver-service-aarch64-unknown-linux-gnu$/);
  assert.ok(commands[0][1].includes('--locked'));
  assert.ok(commands[0][1].includes('mixrelay-driver-service'));
  assert.ok(!commands[0][1].includes('mixrelay-engine-service'));
  fsApi.readFileSync = path => path.endsWith('Cargo.toml') ? '[workspace.package]\nversion = "1.2.4"\n' : '{"version":"1.2.3"}';
  assert.throws(() => prepareDriverPackage({ args: ['--target', 'aarch64-unknown-linux-gnu'], rootDir: '/project', env: {}, fsApi }), /versions must match/);
});

test('Linux service packaging makes a restrictive build output executable for ordinary users', t => {
  const directory = mkdtempSync(join(root, 'target/driver-sidecar-permissions-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  mkdirSync(join(directory, 'apps/desktop/src-tauri'), { recursive: true });
  writeFileSync(join(directory, 'Cargo.toml'), '[workspace.package]\nversion = "1.2.3"\n');
  writeFileSync(join(directory, 'apps/desktop/src-tauri/tauri.conf.json'), '{"version":"1.2.3"}');
  const input = join(directory, 'target/x86_64-unknown-linux-gnu/release/mixrelay-driver-service');
  const bytes = '#!/bin/sh\nexit 0\n';
  mkdirSync(dirname(input), { recursive: true });
  writeFileSync(input, bytes, { mode: 0o700 });
  prepareDriverPackage({ args: ['--target', 'x86_64-unknown-linux-gnu'], rootDir: directory,
    env: {}, run: () => ({ status: 0 }) });
  const sidecar = join(directory, 'apps/desktop/src-tauri/binaries/mixrelay-driver-service-x86_64-unknown-linux-gnu');
  assert.equal(readFileSync(sidecar, 'utf8'), bytes);
  assert.equal(statSync(sidecar).mode & 0o777, 0o755);
  assert.equal(statSync(input).mode & 0o777, 0o700);
});

function driverFixture() {
  const elf = Buffer.alloc(64);
  elf.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]);
  elf[16] = 2; elf[18] = 183; elf[52] = 64;
  const executables = ['usr/bin/mixrelay-driver-manager', 'usr/bin/mixrelay-driver-service', 'usr/bin/mixrelay-engine-service'];
  const files = Object.entries(config.bundle.linux.deb.files).filter(([, source]) => source !== null).map(([path]) => path.slice(1));
  return {
    packageName: 'mix-relay-driver', version: '1.2.3', versionReceipt: '{"version":"1.2.3"}',
    architecture: 'arm64',
    depends: `${config.bundle.linux.deb.depends.join(', ')}, libwebkit2gtk-4.1-0, libgtk-3-0t64`,
    contents: [...executables.map(path => `-rwxr-xr-x 0/0 64 2026-10-06 12:00 ./${path}`),
      ...files.map(path => `-rw-r--r-- 0/0 64 2026-10-06 12:00 ./${path}`)].join('\n'),
    elfHeaders: Object.fromEntries(executables.map(path => [path, elf])),
  };
}

test('Driver checker requires a terminal and runtime and rejects editor ownership and mismatched installed-version receipts', () => {
  const fixture = driverFixture();
  assert.equal(validatePackage(fixture).executables.length, 3);
  assert.equal(validatePackage({ ...fixture,
    depends: fixture.depends.replace('xterm | x-terminal-emulator', 'x-terminal-emulator'),
  }).executables.length, 3);
  assert.throws(() => validatePackage({ ...fixture,
    depends: fixture.depends.replace('xterm | x-terminal-emulator, ', ''),
  }), /Missing required dependency: xterm or x-terminal-emulator/);
  assert.throws(() => validatePackage({ ...fixture,
    depends: fixture.depends.replace('xterm | x-terminal-emulator', 'xterm | unrelated'),
  }), /Missing required dependency: xterm or x-terminal-emulator/);
  assert.throws(() => validatePackage({ ...fixture, versionReceipt: '{"version":"1.2.2"}' }), /receipt/);
  assert.throws(() => validatePackage({ ...fixture, contents: `${fixture.contents}\n-rwxr-xr-x 0/0 64 2026-10-06 12:00 ./usr/bin/mixrelay-desktop` }), /must not own/);
  assert.throws(() => validatePackage({ ...fixture, contents: fixture.contents.replace('usr/share/mixrelay-driver/version.json', 'usr/share/mixrelay-driver/missing.json') }), /documentation/);
});

test('macOS Driver-only ownership, autostart and optional cleanup preserve App boundaries', () => {
  const contract = read('native/macos/installer/src/contract.rs');
  assert.match(contract, /\/Applications\/MixRelay Driver\.app/);
  assert.match(contract, /driver-manager\.owner/);
  assert.match(contract, /pub const APP_PATH: &str = "\/Applications\/MixRelay\.app"/);
  assert.match(contract, /bin\/mixrelay-engine-service/);
  const launchd = read('packaging/macos/io.mixrelay.audio.driver.plist.in');
  assert.match(launchd, /<key>KeepAlive<\/key><true\/>/);
  assert.match(launchd, /<key>RunAtLoad<\/key><true\/>/);
  assert.match(launchd, /<key>UserName<\/key><string>__MIXRELAY_SERVICE_USER__/);
  const cleanup = read('native/macos/installer/src/native/user_data.rs').split('#[cfg(test)]')[0];
  assert.match(cleanup, /"Library\/Caches", c"io\.mixrelay\.driver\.desktop"/);
  assert.match(cleanup, /"Library\/WebKit", c"io\.mixrelay\.driver\.desktop"/);
  assert.match(cleanup, /"Library\/Saved Application State",\s*c"io\.mixrelay\.driver\.desktop\.savedState"/);
  assert.match(cleanup, /if !delete_settings \{\s*return Ok\(\(\)\);\s*\}[\s\S]*remove_file\(&parent, c"drivers\.json"\)/);
  assert.doesNotMatch(cleanup, /mixrelay_licensing|graphs\.json|recording-settings\.json/);
});
