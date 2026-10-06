import assert from "node:assert/strict";
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const sourceScript = fileURLToPath(new URL("./build-macos-universal-app.sh", import.meta.url));
const driverLicense = readFileSync(new URL("../LICENSE-Driver.txt", import.meta.url));
const changelog = readFileSync(new URL("../CHANGELOG.md", import.meta.url));

function executable(path, contents) {
  writeFileSync(path, contents);
  chmodSync(path, 0o755);
}

function createApp(path, { architecture = "universal", signed = false } = {}) {
  mkdirSync(join(path, "Contents", "MacOS"), { recursive: true });
  writeFileSync(join(path, "Contents", "MacOS", "MixRelay"), `MACHO ${architecture}\n`);
  if (signed) writeFileSync(join(path, ".tauri-signed"), "signed\n");
}

function fixture(t) {
  const fixtureRoot = fileURLToPath(new URL("../target/test-artifacts/", import.meta.url));
  mkdirSync(fixtureRoot, { recursive: true });
  const root = mkdtempSync(join(fixtureRoot, "mixrelay-universal-build-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  const bin = join(root, "bin");
  const scripts = join(root, "scripts");
  const target = join(root, "target", "universal-apple-darwin", "release", "bundle", "macos");
  const app = join(target, "MixRelay.app");
  const unrelated = join(target, "unrelated.keep");
  const log = join(root, "mock-command.log");
  const sdk = join(root, "mock-sdk");
  const developer = join(root, "MockXcode.app", "Contents", "Developer");
  const script = join(scripts, "build-macos-universal-app.sh");
  mkdirSync(bin, { recursive: true });
  mkdirSync(scripts, { recursive: true });
  mkdirSync(target, { recursive: true });
  mkdirSync(sdk, { recursive: true });
  mkdirSync(developer, { recursive: true });
  mkdirSync(join(root, "apps/desktop/src-tauri/icons"), { recursive: true });
  writeFileSync(join(root, "apps/desktop/src-tauri/icons/icon.icns"), "icon fixture");
  writeFileSync(join(root, "LICENSE-Driver.txt"), driverLicense);
  writeFileSync(join(root, "CHANGELOG.md"), changelog);
  writeFileSync(unrelated, "preserve me\n");
  copyFileSync(sourceScript, script);
  chmodSync(script, 0o755);

  executable(join(bin, "uname"), `#!/bin/sh
if [ "$1" = "-s" ]; then
  printf '%s\\n' Darwin
else
  printf '%s\\n' Darwin
fi
`);
  executable(join(bin, "rustup"), `#!/bin/sh
printf '%s\\n' aarch64-apple-darwin x86_64-apple-darwin
`);
  executable(join(bin, "xcrun"), `#!/bin/sh
if [ "$1" = "--sdk" ] && [ "$3" = "--show-sdk-path" ]; then
  printf '%s\\n' "$MOCK_SDK"
  exit 0
fi
if [ "$1" = "--sdk" ] && [ "$3" = "--find" ]; then
  printf '%s\\n' /usr/bin/clang
  exit 0
fi
if [ "$1" = "lipo" ]; then
  if [ "$2" = "-create" ]; then
    source="$3"
    output=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "-output" ]; then output="$2"; break; fi
      shift
    done
    cp "$source" "$output"
    exit $?
  fi
  candidate=""
  for argument in "$@"; do candidate="$argument"; done
  if grep -q 'SINGLE' "$candidate" 2>/dev/null; then
    printf '%s\\n' arm64
  else
    printf '%s\\n' 'arm64 x86_64'
  fi
  exit 0
fi
printf '%s\\n' 'unexpected xcrun invocation' >&2
exit 2
`);
  executable(join(bin, "cargo"), `#!/bin/sh
printf 'cargo %s\\n' "$*" >> "$MOCK_LOG"
target=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--target" ]; then target="$2"; break; fi
  shift
done
mkdir -p "target/$target/release"
for helper in mixrelay-driver-uninstaller mixrelay-macos-installer mixrelay-macos-payload-check; do
  if [ "\${MOCK_HELPER_SINGLE:-}" = 1 ]; then
    printf '%s\\n' 'MACHO SINGLE' > "target/$target/release/$helper"
  else
    printf '%s\\n' 'MACHO universal' > "target/$target/release/$helper"
  fi
  chmod 755 "target/$target/release/$helper"
done
`);
  executable(join(bin, "plutil"), `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
const path = args.at(-1);
if (args[0] === '-create') fs.writeFileSync(path, '{}');
else if (args[0] === '-insert') {
  const value = JSON.parse(fs.readFileSync(path, 'utf8'));
  value[args[1]] = args[2] === '-bool' ? args[3] === 'YES' : args[3];
  fs.writeFileSync(path, JSON.stringify(value));
} else if (args[0] === '-extract') {
  process.stdout.write(JSON.parse(fs.readFileSync(path, 'utf8'))[args[1]]);
} else process.exit(2);
`);
  executable(join(bin, "file"), `#!/bin/sh
if [ "\${MOCK_FILE_FAIL:-}" = 1 ]; then exit 127; fi
candidate="$2"
if grep -q 'MACHO' "$candidate" 2>/dev/null; then
  printf '%s\\n' 'Mach-O universal binary'
else
  printf '%s\\n' 'ASCII text'
fi
`);
  executable(join(bin, "pnpm"), `#!/bin/sh
printf 'pnpm %s\\n' "$*" >> "$MOCK_LOG"
printf 'APPLE_SIGNING_IDENTITY=%s\\n' "\${APPLE_SIGNING_IDENTITY:-}" >> "$MOCK_LOG"
printf 'MIXRELAY_CODESIGN_IDENTITY=%s\\n' "\${MIXRELAY_CODESIGN_IDENTITY:-}" >> "$MOCK_LOG"
if [ "\${MOCK_PNPM_MODE:-universal}" = missing ]; then exit 0; fi
mkdir -p "$MOCK_APP/Contents/MacOS"
printf '%s\\n' '{"CFBundleShortVersionString":"1.2.3"}' > "$MOCK_APP/Contents/Info.plist"
if [ "\${MOCK_PNPM_MODE:-universal}" = single ]; then
  printf '%s\\n' 'MACHO SINGLE' > "$MOCK_APP/Contents/MacOS/MixRelay"
else
  printf '%s\\n' 'MACHO universal' > "$MOCK_APP/Contents/MacOS/MixRelay"
fi
if [ "\${MOCK_PNPM_MODE:-universal}" = nested ]; then
  mkdir -p "$MOCK_APP/Contents/Frameworks"
  printf '%s\\n' 'MACHO universal' > "$MOCK_APP/Contents/Frameworks/Helper"
fi
if [ -n "\${APPLE_SIGNING_IDENTITY:-}" ] && [ "\${APPLE_SIGNING_IDENTITY}" != - ]; then
  : > "$MOCK_APP/.tauri-signed"
fi
`);
  executable(join(bin, "codesign"), `#!/bin/sh
printf 'codesign %s\\n' "$*" >> "$MOCK_LOG"
if [ "$1" = "--verify" ]; then
  [ "\${MOCK_INVALID_SIGNATURE:-}" != 1 ] || exit 1
  candidate=""
  for argument in "$@"; do candidate="$argument"; done
  [ -f "$candidate/.tauri-signed" ] || [ -f "$candidate/.ad-hoc-signed" ] || grep -q SIGNED "$candidate" 2>/dev/null
  exit $?
fi
if [ "$1" = "--force" ] && [ "$2" = "--sign" ]; then
  candidate=""
  for argument in "$@"; do candidate="$argument"; done
  if [ -d "$candidate" ]; then
    : > "$candidate/.ad-hoc-signed"
  else
    printf '%s\\n' SIGNED >> "$candidate"
  fi
  exit 0
fi
exit 0
`);

  return { root, bin, developer, app, unrelated, log, sdk, script };
}

function runBuild(paths, extraEnv = {}, args = []) {
  const env = { ...process.env };
  for (const key of [
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "MIXRELAY_CODESIGN_IDENTITY",
    "APPLE_SIGNING_IDENTITY",
    "CC_aarch64_apple_darwin",
    "CC_x86_64_apple_darwin",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
    "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER",
    "SDKROOT",
    "MACOSX_DEPLOYMENT_TARGET",
  ]) delete env[key];
  Object.assign(env, {
    PATH: `${paths.bin}:${env.PATH ?? ""}`,
    DEVELOPER_DIR: paths.developer,
    CARGO_BUILD_JOBS: "1",
    MOCK_APP: paths.app,
    MOCK_FILE_FAIL: "",
    MOCK_LOG: paths.log,
    MOCK_PNPM_MODE: "universal",
    MOCK_SDK: paths.sdk,
    APPLE_SIGNING_IDENTITY: "Developer ID Application: MixRelay",
    ...extraEnv,
  });
  return spawnSync("zsh", [paths.script, ...args], { cwd: paths.root, encoding: "utf8", env });
}

function log(paths) {
  return existsSync(paths.log) ? readFileSync(paths.log, "utf8") : "";
}

function assertPreserved(paths) {
  assert.equal(readFileSync(join(paths.app, "Contents", "MacOS", "MixRelay"), "utf8"), "MACHO old\n");
  assert.equal(readFileSync(paths.unrelated, "utf8"), "preserve me\n");
}

test("maps explicit identity before Tauri and does not force-resign valid output", (t) => {
  const paths = fixture(t);
  const result = runBuild(paths, { MIXRELAY_CODESIGN_IDENTITY: "Developer ID Application: MixRelay" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(log(paths), /--target universal-apple-darwin/);
  assert.match(log(paths), /APPLE_SIGNING_IDENTITY=Developer ID Application: MixRelay/);
  assert.doesNotMatch(log(paths), /codesign --force/);
  assert.equal(readFileSync(paths.unrelated, "utf8"), "preserve me\n");
});

test("rejects conflicting signing identities before invoking Tauri", (t) => {
  const paths = fixture(t);
  const result = runBuild(paths, {
    MIXRELAY_CODESIGN_IDENTITY: "one",
    APPLE_SIGNING_IDENTITY: "two",
  });
  assert.notEqual(result.status, 0);
  assert.doesNotMatch(log(paths), /^pnpm /m);
});

test("Driver product builds source helpers and signs its independent removal app before the outer manager", (t) => {
  const paths = fixture(t);
  paths.app = paths.app.replace(/MixRelay\.app$/, "MixRelayDriver.app");
  const identity = "Developer ID Application: MixRelay (AB12C3D4E5)";
  const result = runBuild(paths, { APPLE_SIGNING_IDENTITY: identity }, ["--driver"]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(log(paths), /--config src-tauri\/tauri\.driver\.conf\.json/);
  assert.ok(existsSync(paths.app));
  const commands = log(paths);
  const cargoBuilds = commands.split("\n").filter(line => line.startsWith("cargo "));
  assert.equal(cargoBuilds.length, 2);
  assert.match(cargoBuilds[0], /--release --target aarch64-apple-darwin --bin mixrelay-driver-uninstaller --bin mixrelay-macos-installer --bin mixrelay-macos-payload-check$/);
  assert.match(cargoBuilds[1], /--release --target x86_64-apple-darwin --bin mixrelay-driver-uninstaller --bin mixrelay-macos-installer --bin mixrelay-macos-payload-check$/);
  assert.ok(commands.indexOf(cargoBuilds[1]) < commands.indexOf("pnpm "));
  const uninstaller = join(paths.app, "Contents/Helpers/Uninstall MixRelay Driver.app");
  const info = JSON.parse(readFileSync(join(uninstaller, "Contents/Info.plist"), "utf8"));
  assert.equal(info.CFBundleIdentifier, "io.mixrelay.driver.uninstaller");
  assert.equal(info.CFBundleExecutable, "mixrelay-driver-uninstaller");
  assert.equal(info.MixRelaySigningTeam, "AB12C3D4E5");
  assert.equal(info.CFBundleShortVersionString, "1.2.3");
  const helperOutput = join(paths.root, "target/universal-apple-darwin/release");
  assert.deepEqual(readFileSync(join(uninstaller, "Contents/Resources/mixrelay-macos-installer")), readFileSync(join(helperOutput, "mixrelay-macos-installer")));
  assert.deepEqual(readFileSync(join(uninstaller, "Contents/Resources/LICENSE-Driver.txt")), driverLicense);
  assert.deepEqual(readFileSync(join(uninstaller, "Contents/Resources/CHANGELOG.md")), changelog);
  assert.equal(readFileSync(join(uninstaller, "Contents/Resources/icon.icns"), "utf8"), "icon fixture");
  const helperSign = commands.indexOf(`--identifier io.mixrelay.driver.uninstaller ${uninstaller}`);
  const outerSign = commands.indexOf(`--preserve-metadata=entitlements,flags,runtime --timestamp ${paths.app}`);
  assert.ok(helperSign >= 0 && outerSign > helperSign);
  assert.match(commands, /--identifier mixrelay-macos-installer /);
  assert.match(commands, /--identifier mixrelay-macos-payload-check /);
});

test("rejects an unsigned release identity before invoking Tauri", (t) => {
  const paths = fixture(t);
  const result = runBuild(paths, { APPLE_SIGNING_IDENTITY: "" });
  assert.notEqual(result.status, 0);
  assert.doesNotMatch(log(paths), /^pnpm /m);
});

test("Driver product rejects a missing signing Team ID before building helpers", (t) => {
  const paths = fixture(t);
  const result = runBuild(paths, {}, ["--driver"]);
  assert.notEqual(result.status, 0);
  assert.doesNotMatch(log(paths), /^cargo |^pnpm /m);
});

test("Driver product rejects single-architecture helper output and restores its manager", (t) => {
  const paths = fixture(t);
  paths.app = paths.app.replace(/MixRelay\.app$/, "MixRelayDriver.app");
  createApp(paths.app, { architecture: "old", signed: true });
  const result = runBuild(paths, {
    APPLE_SIGNING_IDENTITY: "Developer ID Application: MixRelay (AB12C3D4E5)",
    MOCK_HELPER_SINGLE: "1",
  }, ["--driver"]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Driver helper must contain exactly arm64 and x86_64/);
  assertPreserved(paths);
  assert.doesNotMatch(log(paths), /^pnpm /m);
});

test("rejects nested unsigned output and restores the old app", (t) => {
  const paths = fixture(t);
  createApp(paths.app, { architecture: "old", signed: true });
  const result = runBuild(paths, { MOCK_PNPM_MODE: "nested", MOCK_INVALID_SIGNATURE: "1" });
  assert.notEqual(result.status, 0);
  assertPreserved(paths);
  assert.doesNotMatch(log(paths), /codesign --force/);
});

test("fails on missing fresh output and restores the old app", (t) => {
  const paths = fixture(t);
  createApp(paths.app, { architecture: "old", signed: true });
  const result = runBuild(paths, { MOCK_PNPM_MODE: "missing" });
  assert.notEqual(result.status, 0);
  assertPreserved(paths);
});

test("fails on a single-architecture output and restores the old app", (t) => {
  const paths = fixture(t);
  createApp(paths.app, { architecture: "old", signed: true });
  const result = runBuild(paths, { MOCK_PNPM_MODE: "single" });
  assert.notEqual(result.status, 0);
  assertPreserved(paths);
});

test("restores the old app when a validation tool is missing", (t) => {
  const paths = fixture(t);
  createApp(paths.app, { architecture: "old", signed: true });
  const result = runBuild(paths, { MOCK_FILE_FAIL: "1" });
  assert.notEqual(result.status, 0);
  assertPreserved(paths);
});

test("rejects a pre-existing app symlink without invoking Tauri", (t) => {
  const paths = fixture(t);
  const linkedApp = join(paths.root, "linked-app");
  createApp(linkedApp, { architecture: "old", signed: true });
  symlinkSync(linkedApp, paths.app);
  const result = runBuild(paths);
  assert.notEqual(result.status, 0);
  assert.equal(lstatSync(paths.app).isSymbolicLink(), true);
  assert.doesNotMatch(log(paths), /^pnpm /m);
});
