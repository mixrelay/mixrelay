import { createHash } from "node:crypto";
import {
  chmodSync,
  closeSync,
  copyFileSync,
  existsSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const LANGUAGES = JSON.parse(readFileSync(new URL("../packaging/localization/languages.json", import.meta.url), "utf8"));
const LOCALIZED_NOTICES = JSON.parse(readFileSync(new URL("../packaging/localization/installer-notices.json", import.meta.url), "utf8"));
const STAGING_BASE = "Library/Application Support/MixRelay";
const INSTALLER_ID = "io.mixrelay.driver.installer";
const INSTALLER_COMPONENT_ID = "io.mixrelay.driver.installer.component";
const OPERATIONS = JSON.parse(readFileSync(new URL("../packaging/localization/installer-operations.json", import.meta.url), "utf8"));
const MAX_COMMAND_OUTPUT = 4 * 1024 * 1024;
const ARCHITECTURES = ["arm64", "x86_64"];
const fsApi = {
  chmodSync,
  closeSync,
  copyFileSync,
  existsSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  rmSync,
  unlinkSync,
  writeFileSync,
};

const runCommand = (command, args, options) => spawnSync(command, args, options);

export function parseArgs(args) {
  const values = {
    payload: undefined,
    installerCli: undefined,
    payloadChecker: undefined,
    teamId: undefined,
    version: undefined,
    signingIdentity: undefined,
    output: undefined,
  };
  const options = new Set(Object.keys(values));
  let help = false;
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--help" || argument === "-h") {
      help = true;
      continue;
    }
    const equal = argument.indexOf("=");
    const name = equal === -1 ? argument : argument.slice(0, equal);
    if (!name.startsWith("--") || !options.has(name.slice(2))) {
      throw new Error(`Unknown argument: ${argument}`);
    }
    const key = name.slice(2);
    const value = equal === -1 ? args[++index] : argument.slice(equal + 1);
    if (!value || value.startsWith("--")) throw new Error(`${name} requires a value`);
    if (values[key] !== undefined) throw new Error(`${name} may only be provided once`);
    values[key] = value;
  }
  if (help) return { help: true, ...values };
  for (const key of options) {
    if (values[key] === undefined) throw new Error(`Missing required option: --${key}`);
  }
  for (const key of ["payload", "installerCli", "payloadChecker", "output"]) assertAbsolute(`--${key}`, values[key]);
  if (!/^[A-Z0-9]{10}$/.test(values.teamId)) throw new Error("--teamId must be a ten-character Apple Team ID");
  if (!/^[0-9]+(?:\.[0-9]+){1,3}$/.test(values.version)) throw new Error("--version must contain two to four numeric components");
  if (!values.signingIdentity.startsWith("Developer ID Installer: ")
    || !values.signingIdentity.endsWith(`(${values.teamId})`)
    || /[\0\r\n]/.test(values.signingIdentity)) {
    throw new Error("--signingIdentity must be this Team ID's exact Developer ID Installer certificate identity");
  }
  return { help: false, ...values };
}

function usage() {
  return [
    "Usage: node scripts/build-macos-installer-pkgs.mjs --payload PREPARED_PAYLOAD --installerCli UNIVERSAL2_INSTALLER --payloadChecker PAYLOAD_CHECKER --teamId TEAM_ID --version VERSION --signingIdentity 'Developer ID Installer: Name (TEAM_ID)' --output EMPTY_OUTPUT_DIR",
    "",
    "Verifies Developer ID Application signatures in the payload, builds the signed system-volume install .pkg and copies its signed standalone removal app. Developer ID Application and Installer identities are separate requirements; unsigned distribution is not supported.",
  ].join("\n");
}

function assertAbsolute(name, path) {
  if (!path || !path.startsWith(sep) || resolve(path) !== path) {
    throw new Error(`${name} must be an absolute, normalized path`);
  }
}

function isWithin(path, parent) {
  return path === parent || path.startsWith(`${parent}${sep}`);
}

function assertDisjoint(output, inputs) {
  for (const [name, path] of inputs) {
    if (isWithin(output, path) || isWithin(path, output)) {
      throw new Error(`--output must be separate from ${name}: ${output}`);
    }
  }
}

function assertNoSymlinkComponents(fs, name, path, allowMissingLeaf = false) {
  const components = path.split(sep).filter(Boolean);
  let current = sep;
  for (let index = 0; index < components.length; index += 1) {
    current = join(current, components[index]);
    let metadata;
    try {
      metadata = fs.lstatSync(current);
    } catch (error) {
      if (error.code === "ENOENT" && allowMissingLeaf) return;
      throw new Error(`Missing ${name} path component ${current}: ${error.message}`);
    }
    if (metadata.isSymbolicLink()) throw new Error(`${name} path must not contain symlinks: ${current}`);
    if (index < components.length - 1 && !metadata.isDirectory()) {
      throw new Error(`${name} parent component is not a directory: ${current}`);
    }
  }
}

function requirePath(fs, name, path, kind) {
  assertNoSymlinkComponents(fs, name, path);
  const metadata = fs.lstatSync(path);
  if (metadata.isSymbolicLink() || (kind === "directory" ? !metadata.isDirectory() : !metadata.isFile())) {
    throw new Error(`${name} must be a non-symlink ${kind}: ${path}`);
  }
  return metadata;
}

function walkFiles(fs, root) {
  const files = [];
  const walk = (directory) => {
    const entries = fs.readdirSync(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`Release payload contains a symlink: ${path}`);
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile()) files.push(path);
      else throw new Error(`Release payload contains an unsupported inode: ${path}`);
    }
  };
  walk(root);
  return files;
}

function validatePayload(fs, root) {
  requirePath(fs, "--payload", root, "directory");
  const manager = join(root, "Applications/MixRelay Driver.app");
  const hal = join(root, "Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver");
  const driver = join(root, "Library/Application Support/MixRelay/bin/mixrelay-driver-service");
  const template = join(root, "templates/io.mixrelay.audio.driver.plist.in");
  const uninstaller = join(root, "Applications/Uninstall MixRelay Driver.app");
  const removalTemplate = join(root, "templates/io.mixrelay.driver.removal.plist");
  requirePath(fs, "uninstaller payload", uninstaller, "directory");
  requirePath(fs, "removal launchd template", removalTemplate, "file");
  const manifestPath = join(root, "payload-manifest.json");
  const managerMetadata = requirePath(fs, "manager payload", manager, "directory");
  const halMetadata = requirePath(fs, "HAL payload", hal, "directory");
  for (const [name, path, metadata] of [["manager", manager, managerMetadata], ["HAL", hal, halMetadata]]) {
    if ((metadata.mode & 0o777) !== 0o755) {
      throw new Error(`${name} payload bundle root must have mode 0755: ${path}`);
    }
  }
  const driverMetadata = requirePath(fs, "driver payload", driver, "file");
  const templateMetadata = requirePath(fs, "launchd template", template, "file");
  const manifestMetadata = requirePath(fs, "payload manifest", manifestPath, "file");
  const license = join(manager, "Contents/Resources/LICENSE.txt");
  const driverLicense = join(manager, "Contents/Resources/LICENSE-Driver.txt");
  const driverLicenseKo = join(manager, "Contents/Resources/LICENSE-Driver.ko.txt");
  const changelog = join(manager, "Contents/Resources/CHANGELOG.md");
  const documents = LANGUAGES.flatMap(language => [language.appTerms, language.driverTerms]);
  const driverTerms = LANGUAGES.map(language => ({ ...language, path: join(manager, "Contents/Resources", language.driverTerms) }));
  for (const document of [...documents.map(filename => join(manager, "Contents/Resources", filename)), changelog]) {
    if (requirePath(fs, "release document", document, "file").size === 0) {
      throw new Error(`release document must be non-empty: ${document}`);
    }
  }
  if (manifestMetadata.size > 16 * 1024 * 1024) throw new Error("payload manifest exceeds its fixed size limit");
  if (driverMetadata.size === 0 || (driverMetadata.mode & 0o111) === 0) {
    throw new Error(`driver payload must be a non-empty executable: ${driver}`);
  }
  if (templateMetadata.size === 0 || templateMetadata.size > 16 * 1024) {
    throw new Error(`launchd template must be between 1 and 16384 bytes: ${template}`);
  }
  for (const tree of [manager, hal, uninstaller]) walkFiles(fs, tree);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.format !== 1 || manifest.status !== "payload-prepared-not-package"
    || manifest.installContract?.manager !== "/Applications/MixRelay Driver.app"
    || manifest.installContract?.hal !== "/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver"
    || manifest.installContract?.driver !== "/Library/Application Support/MixRelay/bin/mixrelay-driver-service"
    || manifest.installContract?.launchd !== "/Library/LaunchDaemons/io.mixrelay.audio.driver.plist"
    || manifest.installContract?.uninstaller !== "/Applications/Uninstall MixRelay Driver.app"
    || manifest.installContract?.removalLaunchd !== "/Library/LaunchDaemons/io.mixrelay.driver.removal.plist") {
    throw new Error("payload manifest does not describe the fixed MixRelay install contract");
  }
  if (fs.existsSync(join(manager, "Contents/Helpers/Uninstall MixRelay Driver.app"))) {
    throw new Error("A single-package release must not contain a separate uninstaller app");
  }
  return { manager, hal, driver, template, uninstaller, removalTemplate, license, driverLicense, driverLicenseKo, changelog, driverTerms };
}

function validateUniversalExecutable(fs, path, run, name) {
  const metadata = requirePath(fs, name, path, "file");
  if (metadata.size === 0 || (metadata.mode & 0o111) === 0) {
    throw new Error(`${name} must be a non-empty executable: ${path}`);
  }
  const description = runChecked(run, "/usr/bin/file", ["-b", path], `inspect ${name} ${path}`);
  if (!description.includes("Mach-O")) throw new Error(`${name} is not a Mach-O executable: ${path}`);
  const architectures = runChecked(run, "/usr/bin/xcrun", ["lipo", "-archs", path], `inspect ${name} architectures ${path}`)
    .split(/\s+/).filter(Boolean).sort();
  if (architectures.length !== ARCHITECTURES.length
    || architectures.some((architecture, index) => architecture !== [...ARCHITECTURES].sort()[index])) {
    throw new Error(`${name} must contain exactly arm64 and x86_64; found ${architectures.join(" ") || "none"}`);
  }
}

function validatePayloadChecker(fs, path) {
  if (path.split(sep).at(-1) !== "mixrelay-macos-payload-check") {
    throw new Error(`--payloadChecker must name the mixrelay-macos-payload-check binary: ${path}`);
  }
  const metadata = requirePath(fs, "--payloadChecker", path, "file");
  if (metadata.size === 0 || (metadata.mode & 0o111) === 0) {
    throw new Error(`payload checker must be a non-empty executable: ${path}`);
  }
}

function verifyPayloadReleaseContract(run, checker, payload, teamId, installerCli) {
  // The verifier checks syntax and binding of the launchd template only; it does
  // not resolve this fixture identity against the machine's account database.
  const account = ["mixrelay-preflight", "staff", "/Users/mixrelay-preflight"];
  const args = [payload, teamId, ...account, installerCli];
  const environment = {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    LC_ALL: "C",
    LANG: "C",
  };
  if (process.env.HOME) environment.HOME = process.env.HOME;
  if (process.env.TMPDIR) environment.TMPDIR = process.env.TMPDIR;
  const result = run(checker, args, {
    encoding: "utf8",
    env: environment,
    maxBuffer: MAX_COMMAND_OUTPUT,
    timeout: 30 * 60 * 1000,
  });
  if (result?.error || result?.status !== 0 || result?.signal) {
    const detail = result?.error?.message ?? result?.stderr ?? result?.signal ?? `status ${result?.status}`;
    throw new Error(`Developer ID Application payload preflight failed: ${detail}`);
  }
}

function hashFile(fs, hash, path, relativePath) {
  const metadata = fs.lstatSync(path);
  hash.update(`${relativePath}\0${metadata.mode & 0o7777}\0${metadata.size}\0`);
  const descriptor = fs.openSync(path, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  try {
    let bytesRead;
    do {
      bytesRead = fs.readSync(descriptor, buffer, 0, buffer.length, null);
      if (bytesRead > 0) hash.update(buffer.subarray(0, bytesRead));
    } while (bytesRead > 0);
  } finally {
    fs.closeSync(descriptor);
  }
}

function fingerprint(fs, paths, cli, salt = "") {
  const hash = createHash("sha256");
  hash.update(salt);
  for (const [label, root] of [...paths, ["installerCli", cli]]) {
    const metadata = fs.lstatSync(root);
    if (metadata.isDirectory()) {
      for (const file of walkFiles(fs, root)) {
        hashFile(fs, hash, file, `${label}/${relative(root, file).split(sep).join("/")}`);
      }
    } else {
      hashFile(fs, hash, root, label);
    }
  }
  return hash.digest("hex").slice(0, 32);
}

function runChecked(run, command, args, description) {
  const result = run(command, args, {
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "C", LANG: "C" },
    maxBuffer: MAX_COMMAND_OUTPUT,
    timeout: 30 * 60 * 1000,
  });
  if (result?.error || result?.status !== 0 || result?.signal) {
    const detail = [result?.error?.message, result?.stdout, result?.stderr, result?.signal]
      .filter(Boolean).join("\n") || `status ${result?.status}`;
    throw new Error(`${description} failed: ${detail}`);
  }
  return String(result.stdout ?? "").trim();
}

function ensurePackageDirectories(fs, packageRoot, directory) {
  if (!isWithin(directory, packageRoot) || directory === packageRoot) {
    throw new Error(`package directory must be below its root: ${directory}`);
  }
  let current = packageRoot;
  for (const component of relative(packageRoot, directory).split(sep)) {
    current = join(current, component);
    try {
      fs.mkdirSync(current, { mode: 0o755 });
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
    }
    const metadata = fs.lstatSync(current);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw new Error(`package staging path is not a directory: ${current}`);
    }
    fs.chmodSync(current, 0o755);
  }
}

function copyWithDitto(fs, run, source, destination, packageRoot) {
  ensurePackageDirectories(fs, packageRoot, dirname(destination));
  runChecked(run, "/usr/bin/ditto", [source, destination], `copy release payload ${source}`);
}

/**
 * Applies the fixed staging policy to every bundle discovered by pkgbuild --analyze.
 *
 * Installer bundle relocation can otherwise move a payload bundle onto a copy the user
 * previously moved elsewhere, bypassing the transaction's exact staging paths. Keep each
 * analyzer-provided path intact, require the expected manager and HAL roots, disable relocation
 * and version skipping, and require the analyzed bundle identity at its fixed staging path.
 */
export function fixedComponentBundlePolicy(componentPlist, packageRoot, requiredBundleRoots) {
  if (!Array.isArray(componentPlist)) {
    throw new Error("pkgbuild component analysis must be an array");
  }
  const allowedRoots = requiredBundleRoots.map(path => resolve(path));
  const seen = new Set();
  const bundlePaths = [];
  const visit = (bundle) => {
    if (!bundle || typeof bundle !== "object" || Array.isArray(bundle)) {
      throw new Error("pkgbuild component analysis contains an invalid bundle entry");
    }
    const relativeBundlePath = bundle.RootRelativeBundlePath;
    if (typeof relativeBundlePath !== "string" || relativeBundlePath.length === 0) {
      throw new Error("pkgbuild component analysis omitted a bundle's fixed staging path");
    }
    const bundlePath = resolve(packageRoot, relativeBundlePath);
    const canonicalRelativePath = relative(packageRoot, bundlePath).split(sep).join("/");
    if (canonicalRelativePath !== relativeBundlePath || !isWithin(bundlePath, resolve(packageRoot))) {
      throw new Error(`pkgbuild reported an unsafe bundle staging path: ${relativeBundlePath}`);
    }
    if (!allowedRoots.some(root => isWithin(bundlePath, root))) {
      throw new Error(`pkgbuild discovered a bundle outside the fixed manager/HAL payloads: ${relativeBundlePath}`);
    }
    if (seen.has(relativeBundlePath)) {
      throw new Error(`pkgbuild discovered a duplicate bundle path: ${relativeBundlePath}`);
    }
    seen.add(relativeBundlePath);
    bundlePaths.push(relativeBundlePath);

    bundle.BundleIsRelocatable = false;
    bundle.BundleIsVersionChecked = false;
    bundle.BundleHasStrictIdentifier = true;
    bundle.BundleOverwriteAction = "upgrade";
    if (bundle.ChildBundles !== undefined) {
      if (!Array.isArray(bundle.ChildBundles)) {
        throw new Error(`pkgbuild returned invalid child bundles for ${relativeBundlePath}`);
      }
      for (const child of bundle.ChildBundles) visit(child);
    }
  };
  for (const bundle of componentPlist) visit(bundle);

  for (const root of allowedRoots) {
    if (!seen.has(relative(packageRoot, root).split(sep).join("/"))) {
      throw new Error(`pkgbuild did not discover required release bundle: ${root}`);
    }
  }
  return { componentPlist, bundlePaths };
}

function prepareComponentPlist({ fs, run, packageRoot, work, mode, requiredBundleRoots }) {
  const analyzedPath = join(work, `${mode}-analyzed-components.plist`);
  const fixedPath = join(work, `${mode}-fixed-components.plist`);
  const jsonPath = join(work, `${mode}-fixed-components.json`);
  runChecked(run, "/usr/bin/pkgbuild", ["--analyze", "--root", packageRoot, analyzedPath], `analyze ${mode} package bundles`);
  requirePath(fs, `${mode} component analysis`, analyzedPath, "file");
  const analyzed = JSON.parse(runChecked(run, "/usr/bin/plutil", ["-convert", "json", "-o", "-", analyzedPath], `read ${mode} component analysis`));
  const fixed = fixedComponentBundlePolicy(analyzed, packageRoot, requiredBundleRoots);
  fs.writeFileSync(jsonPath, JSON.stringify(fixed.componentPlist), "utf8");
  runChecked(run, "/usr/bin/plutil", ["-convert", "xml1", "-o", fixedPath, jsonPath], `write fixed ${mode} component policy`);
  requirePath(fs, `${mode} fixed component policy`, fixedPath, "file");
  return fixedPath;
}

function installPostinstall(teamId, stagingId) {
  const stage = `"$3/${STAGING_BASE}/.installer-inputs/${stagingId}"`;
  return `#!/bin/sh
set -eu
fail() { echo "MixRelay installer: $*" >&2; exit 1; }
[ "$#" -ge 3 ] || fail "expected at least 3 Installer script arguments; received $#"
[ "$3" = "/" ] || fail "the signed product may only install on the system volume"
[ "$(/usr/bin/id -u)" = "0" ] || fail "administrator authorization did not provide root"
stage=${stage}
[ -d "$stage" ] && [ ! -L "$stage" ] || fail "private package staging directory is missing or unsafe"
check_dir() {
  path=$1
  [ -d "$path" ] && [ ! -L "$path" ] || fail "unsafe package directory: $path"
  owner=$(LC_ALL=C /usr/bin/stat -f '%u:%g:%Lp' "$path") || fail "cannot inspect package directory: $path"
  [ "$owner" = "0:0:755" ] || fail "package directory is not root:wheel 0755: $path"
}
check_shared_application_support_dir() {
  path=$1
  [ -d "$path" ] && [ ! -L "$path" ] || fail "unsafe shared application support directory: $path"
  owner=$(LC_ALL=C /usr/bin/stat -f '%Su:%Sg:%Lp' "$path") || fail "cannot inspect shared application support directory: $path"
  case "$owner" in
    root:admin:755|root:wheel:755) ;;
    *) fail "shared application support directory has unexpected ownership or mode: $path" ;;
  esac
}
check_file() {
  path=$1
  [ -f "$path" ] && [ ! -L "$path" ] || fail "unsafe package file: $path"
  owner=$(LC_ALL=C /usr/bin/stat -f '%u:%g' "$path") || fail "cannot inspect package file: $path"
  [ "$owner" = "0:0" ] || fail "package file is not root:wheel: $path"
}
check_dir "$3/Library"
check_shared_application_support_dir "$3/Library/Application Support"
check_dir "$3/${STAGING_BASE}"
check_dir "$3/${STAGING_BASE}/.installer-inputs"
check_dir "$stage"
stage_identity=$(LC_ALL=C /usr/bin/stat -f '%d:%i' "$stage") || fail "cannot capture package staging identity"
cli="$stage/bin/mixrelay-macos-installer"
[ -f "$cli" ] && [ ! -L "$cli" ] && [ -x "$cli" ] || fail "embedded installer CLI is missing or unsafe"
check_file "$cli"
check_dir "$stage/payload"
check_dir "$stage/payload/Applications/MixRelay Driver.app"
check_dir "$stage/payload/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver"
check_file "$stage/payload/Library/Application Support/MixRelay/bin/mixrelay-driver-service"
check_file "$stage/payload/templates/io.mixrelay.audio.driver.plist.in"
check_dir "$stage/payload/Applications/Uninstall MixRelay Driver.app"
check_file "$stage/payload/templates/io.mixrelay.driver.removal.plist"
set +e
"$cli" install ${teamId} \\
  "$stage/payload/Applications/MixRelay Driver.app" \\
  "$stage/payload/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver" \\
  "$stage/payload/Library/Application Support/MixRelay/bin/mixrelay-driver-service" \\
  "$stage/payload/templates/io.mixrelay.audio.driver.plist.in" \\
  "$stage/payload/Applications/Uninstall MixRelay Driver.app" \\
  "$stage/payload/templates/io.mixrelay.driver.removal.plist" --reload-coreaudio
result=$?
set -e
if [ -L "$stage" ]; then
  fail "private package staging path changed to a symlink; left it untouched"
fi
current_identity=$(LC_ALL=C /usr/bin/stat -f '%d:%i' "$stage") || fail "cannot recheck package staging identity"
[ "$current_identity" = "$stage_identity" ] || fail "private package staging inode changed; left it untouched"
/bin/rm -rf "$stage" || fail "could not clean the exact private package staging directory"
[ ! -e "$stage" ] || fail "private package staging cleanup did not complete"
exit "$result"
`;
}

function preinstall(stagingId, fs, packageStage) {
  const allowedInputs = new Set([""]);
  const collectInputs = directory => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      allowedInputs.add(`/${relative(packageStage, path).split(sep).join("/")}`);
      if (entry.isDirectory()) collectInputs(path);
    }
  };
  collectInputs(packageStage);
  const allowedPattern = [...allowedInputs].sort().map(path => `'${path.replaceAll("'", "'\\''")}'`).join("|");
  return `#!/bin/sh
set -eu
fail() { echo "MixRelay package: $*" >&2; exit 1; }
[ "$#" -ge 3 ] || fail "expected at least 3 Installer script arguments; received $#"
[ "$3" = "/" ] || fail "the signed product may only install on the system volume"
[ "$(/usr/bin/id -u)" = "0" ] || fail "administrator authorization did not provide root"
check_dir() {
  path=$1
  if [ -e "$path" ] || [ -L "$path" ]; then
    [ -d "$path" ] && [ ! -L "$path" ] || fail "unsafe package parent: $path"
    owner=$(LC_ALL=C /usr/bin/stat -f '%u:%g:%Lp' "$path") || fail "cannot inspect package parent: $path"
    [ "$owner" = "0:0:755" ] || fail "package parent is not root:wheel 0755: $path"
  fi
}
check_shared_application_support_dir() {
  path=$1
  if [ -e "$path" ] || [ -L "$path" ]; then
    [ -d "$path" ] && [ ! -L "$path" ] || fail "unsafe shared application support directory: $path"
    owner=$(LC_ALL=C /usr/bin/stat -f '%Su:%Sg:%Lp' "$path") || fail "cannot inspect shared application support directory: $path"
    case "$owner" in
      root:admin:755|root:wheel:755) ;;
      *) fail "shared application support directory has unexpected ownership or mode: $path" ;;
    esac
  fi
}
check_dir "$3/Library"
check_shared_application_support_dir "$3/Library/Application Support"
check_dir "$3/${STAGING_BASE}"
check_dir "$3/${STAGING_BASE}/.installer-inputs"
stage="$3/${STAGING_BASE}/.installer-inputs/${stagingId}"
if [ -e "$stage" ] || [ -L "$stage" ]; then
  check_dir "$stage"
  stage_identity=$(LC_ALL=C /usr/bin/stat -f '%d:%i' "$stage") || fail "cannot capture package staging identity"
  check_retry_input() {
    input=$1
    case "\${input#"$stage"}" in
      ${allowedPattern}) ;;
      *) fail "unexpected file in interrupted package inputs: $input" ;;
    esac
    [ ! -L "$input" ] || fail "interrupted package input is a symlink: $input"
    if [ -d "$input" ]; then
      check_dir "$input"
      for child in "$input"/* "$input"/.[!.]* "$input"/..?*; do
        [ -e "$child" ] || [ -L "$child" ] || continue
        check_retry_input "$child"
      done
    else
      [ -f "$input" ] || fail "interrupted package input is not a regular file: $input"
      owner=$(LC_ALL=C /usr/bin/stat -f '%u:%g' "$input") || fail "cannot inspect interrupted package input: $input"
      [ "$owner" = "0:0" ] || fail "interrupted package input is not root:wheel: $input"
    fi
  }
  check_retry_input "$stage"
  # Retry cleanup relies on Installer serializing installation on this volume.
  check_dir "$3/Library"
  check_shared_application_support_dir "$3/Library/Application Support"
  check_dir "$3/${STAGING_BASE}"
  check_dir "$3/${STAGING_BASE}/.installer-inputs"
  check_dir "$stage"
  current_identity=$(LC_ALL=C /usr/bin/stat -f '%d:%i' "$stage") || fail "cannot recheck package staging identity"
  [ "$current_identity" = "$stage_identity" ] || fail "private package staging inode changed; left it untouched"
  /bin/rm -rf "$stage" || fail "could not clean the exact interrupted package inputs"
  [ ! -e "$stage" ] && [ ! -L "$stage" ] || fail "interrupted package input cleanup did not complete"
fi
exit 0
`;
}

function installerRtf(text) {
  // RTF uses signed UTF-16 code units, including one escape per surrogate.
  const escape = value => value.replace(/[\\{}]|[^\x20-\x7e]/g, character => {
    if (character === "\n") return "\\line ";
    if (character === "\t") return "\\tab ";
    if (/[\\{}]/.test(character)) return `\\${character}`;
    const unit = character.charCodeAt(0);
    return `\\u${unit > 32767 ? unit - 65536 : unit}?`;
  });
  const paragraph = (value, style = "\\fs22\\b0\\sl280\\slmult0\\sb0\\sa120") =>
    `\\pard\\ql\\f0${style} ${escape(value)}\\par\n`;
  const blocks = text.trim().replace(/\r\n?/g, "\n").split(/\n[ \t]*\n+/);
  const paragraphs = blocks.flatMap((block, index) => {
    const lines = block.split("\n");
    const heading = lines[0];
    if (index === 0 && LANGUAGES.some(language => [language.agreementTitle, language.guideTitle].includes(lines[1]))) {
      return [
        paragraph(heading, "\\fs28\\b\\sb0\\sa80"),
        paragraph(lines[1], "\\fs24\\b0\\sb0\\sa160"),
        ...lines.slice(2).map(line => paragraph(line, "\\fs20\\b0\\sb0\\sa160")),
      ];
    }
    if (index === 0 || /^\d+\. /.test(heading)
      || LANGUAGES.some(language => [language.businessHeading, language.beforeHeading, language.afterHeading].includes(heading))) {
      const title = paragraph(heading, index === 0 ? "\\fs28\\b\\sb0\\sa80" : "\\fs22\\b\\sb200\\sa80");
      if (lines.length === 1) return [title];
      // Contact labels retain separate lines; hard-wrapped address text rejoins.
      const body = LANGUAGES.some(language => language.businessHeading === heading)
        ? lines.slice(1).join("\n").replace(/\n(?![^\n:：]+[:：])/g, " ")
        : lines.slice(1).join(" ");
      return [title, paragraph(body)];
    }
    return [paragraph(lines.join(" "))];
  });
  // No color table or foreground/background overrides: Installer owns appearance.
  return `{\\rtf1\\ansi\\deff0\\uc1{\\fonttbl{\\f0\\fswiss Helvetica;}}\n${paragraphs.join("")}}\n`;
}

export function distribution(packageFilename, version) {
  return `<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>MixRelay Driver</title>
  <readme file="INSTALLER-README.rtf" mime-type="text/rtf"/>
  <license file="INSTALLER-LICENSE.rtf" mime-type="text/rtf"/>
  <options customize="never" allow-external-scripts="false"/>
  <domains enable_anywhere="false" enable_currentUserHome="false" enable_localSystem="true"/>
  <choices-outline><line choice="install"/></choices-outline>
  <choice id="install" visible="false" enabled="false" selected="true"><pkg-ref id="${INSTALLER_COMPONENT_ID}"/></choice>
  <pkg-ref id="${INSTALLER_COMPONENT_ID}" version="${version}">${packageFilename}</pkg-ref>
  <pkg-ref id="${INSTALLER_COMPONENT_ID}">
    <must-close><app id="io.mixrelay.driver.uninstaller"/></must-close>
  </pkg-ref>
</installer-gui-script>
`;
}

function assertEmptyOutput(fs, output) {
  assertNoSymlinkComponents(fs, "--output", output, true);
  if (!fs.existsSync(output)) {
    fs.mkdirSync(output, { recursive: true, mode: 0o755 });
    return;
  }
  const metadata = fs.lstatSync(output);
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error(`--output must be a non-symlink directory: ${output}`);
  if (fs.readdirSync(output).length !== 0) throw new Error(`--output directory must be empty: ${output}`);
}

function buildInstaller({ fs, run, work, packageRoot, stagingId, paths, installerCli, teamId, version, signingIdentity }) {
  const mode = "install";
  const stageRelative = `${STAGING_BASE}/.installer-inputs/${stagingId}`;
  const packageStage = join(packageRoot, stageRelative);
  const binPath = join(packageStage, "bin/mixrelay-macos-installer");
  const requiredBundleRoots = [];
  fs.mkdirSync(packageRoot, { mode: 0o755 });
  copyWithDitto(fs, run, installerCli, binPath, packageRoot);
  const stagedPayload = join(packageStage, "payload");
  ensurePackageDirectories(fs, packageRoot, stagedPayload);
  const stagedManager = join(stagedPayload, "Applications/MixRelay Driver.app");
  const stagedHal = join(stagedPayload, "Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver");
  copyWithDitto(fs, run, paths.manager, stagedManager, packageRoot);
  copyWithDitto(fs, run, paths.hal, stagedHal, packageRoot);
  const stagedUninstaller = join(stagedPayload, "Applications/Uninstall MixRelay Driver.app");
  copyWithDitto(fs, run, paths.uninstaller, stagedUninstaller, packageRoot);
  requiredBundleRoots.push(stagedManager, stagedHal, stagedUninstaller);
  copyWithDitto(fs, run, paths.driver, join(stagedPayload, "Library/Application Support/MixRelay/bin/mixrelay-driver-service"), packageRoot);
  copyWithDitto(fs, run, paths.template, join(stagedPayload, "templates/io.mixrelay.audio.driver.plist.in"), packageRoot);

  copyWithDitto(fs, run, paths.removalTemplate, join(stagedPayload, "templates/io.mixrelay.driver.removal.plist"), packageRoot);

  const scriptsPath = join(work, `${mode}-scripts`);
  fs.mkdirSync(scriptsPath, { mode: 0o755 });
  const script = installPostinstall(teamId, stagingId);
  fs.writeFileSync(join(scriptsPath, "preinstall"), preinstall(stagingId, fs, packageStage), { encoding: "utf8", mode: 0o755 });
  fs.chmodSync(join(scriptsPath, "preinstall"), 0o755);
  fs.writeFileSync(join(scriptsPath, "postinstall"), script, { encoding: "utf8", mode: 0o755 });
  fs.chmodSync(join(scriptsPath, "postinstall"), 0o755);

  const componentFilename = `MixRelay-Driver-Installer-${version}.component.pkg`;
  const componentPath = join(work, componentFilename);
  const componentPlist = prepareComponentPlist({ fs, run, packageRoot, work, mode, requiredBundleRoots });
  runChecked(run, "/usr/bin/pkgbuild", [
    "--root", packageRoot,
    "--component-plist", componentPlist,
    "--scripts", scriptsPath,
    "--install-location", "/",
    "--identifier", INSTALLER_COMPONENT_ID,
    "--version", version,
    "--ownership", "recommended",
    "--sign", signingIdentity,
    "--timestamp",
    componentPath,
  ], `build signed ${mode} component package`);
  requirePath(fs, `${mode} component package`, componentPath, "file");

  const distributionPath = join(work, `${mode}-distribution.xml`);
  fs.writeFileSync(distributionPath, distribution(componentFilename, version), "utf8");
  const resourcesPath = join(work, `${mode}-resources`);
  fs.mkdirSync(resourcesPath, { mode: 0o755 });
  fs.copyFileSync(paths.changelog, join(resourcesPath, "CHANGELOG.md"));
  // Installer resolves .lproj resources and owns its native language selector.
  // Unlocalized files shadow resources chosen by the native language selector.
  for (const entry of paths.driverTerms) {
    const { language, path: terms } = entry;
    const notice = LOCALIZED_NOTICES[language];
    if (!notice || !notice.startsWith(`MixRelay Driver\n${entry.guideTitle}\n`)) {
      throw new Error(`Missing or mismatched installation guide: ${language}`);
    }
    const labels = OPERATIONS[language];
    if (!labels || Object.values(labels).some(value => typeof value !== "string" || !value.trim())) throw new Error(`Missing operation labels: ${language}`);
    const localized = join(resourcesPath, `${language}.lproj`);
    fs.mkdirSync(localized, { mode: 0o755 });
    fs.writeFileSync(join(localized, "Localizable.strings"), Object.entries(labels).map(([key, value]) => `${JSON.stringify(key)} = ${JSON.stringify(value)};`).join("\n") + "\n", "utf8");
    fs.writeFileSync(join(localized, "INSTALLER-LICENSE.rtf"), installerRtf(fs.readFileSync(terms, "utf8")), "utf8");
    fs.writeFileSync(join(localized, "INSTALLER-README.rtf"), installerRtf(notice), "utf8");
  }
  const productPath = join(work, `MixRelay-Driver-Installer-${version}.pkg`);
  runChecked(run, "/usr/bin/productbuild", [
    "--distribution", distributionPath,
    "--resources", resourcesPath,
    "--package-path", work,
    "--identifier", INSTALLER_ID,
    "--version", version,
    "--sign", signingIdentity,
    "--timestamp",
    productPath,
  ], `build signed ${mode} product package`);
  requirePath(fs, `${mode} product package`, productPath, "file");
  const signature = runChecked(run, "/usr/sbin/pkgutil", ["--check-signature", productPath], `verify signed ${mode} package`);
  verifyPackageSignature(signature, signingIdentity, mode);
  return { productPath, filename: `MixRelay-Driver-Installer-${version}.pkg` };
}

function verifyPackageSignature(signature, signingIdentity, mode) {
  const signatureStatus = signature.match(/^[ \t]*Status:[ \t]*([^\r\n]+)\r?$/m)?.[1].trim();
  const trustedStatus = signatureStatus === "signed by a certificate trusted by macOS"
    || signatureStatus === "signed by a developer certificate issued by Apple for distribution";
  const signingLeaf = signature.match(/^[ \t]*Certificate Chain:[ \t]*\r?\n[ \t]*1\.[ \t]+([^\r\n]+)\r?$/m)?.[1].trim();
  const trustedTimestamp = /^[ \t]*Signed with a trusted timestamp on:[ \t]+\S[^\r\n]*\r?$/m.test(signature);
  if (!trustedStatus || signingLeaf !== signingIdentity || !trustedTimestamp) {
    throw new Error(`${mode} product signature is not trusted with the requested Developer ID Installer identity:\n${signature}`);
  }
}

/**
 * Build one signed product for installation, update and removal without
 * building Rust/Xcode or performing any host installation, launchd, or CoreAudio action.
 */
export function buildPackages({ payload, installerCli, payloadChecker, teamId, version, signingIdentity, output, fs = fsApi, run = runCommand }) {
  for (const [name, path] of [["--payload", payload], ["--installerCli", installerCli], ["--payloadChecker", payloadChecker], ["--output", output]]) assertAbsolute(name, path);
  if (!/^[A-Z0-9]{10}$/.test(teamId)) throw new Error("--teamId must be a ten-character Apple Team ID");
  if (!/^[0-9]+(?:\.[0-9]+){1,3}$/.test(version)) throw new Error("--version must contain two to four numeric components");
  if (!signingIdentity.startsWith("Developer ID Installer: ") || !signingIdentity.endsWith(`(${teamId})`) || /[\0\r\n]/.test(signingIdentity)) {
    throw new Error("a matching Developer ID Installer identity is required; unsigned packages are not supported");
  }
  assertDisjoint(output, [["prepared payload", payload], ["installer CLI", installerCli], ["payload checker", payloadChecker]]);
  for (const [name, path] of [["prepared payload", payload], ["installer CLI", installerCli]]) {
    if (isWithin(payloadChecker, path) || isWithin(path, payloadChecker)) {
      throw new Error(`--payloadChecker must be separate from ${name}: ${payloadChecker}`);
    }
  }
  assertNoSymlinkComponents(fs, "--payload", payload);
  assertNoSymlinkComponents(fs, "--installerCli", installerCli);
  assertNoSymlinkComponents(fs, "--payloadChecker", payloadChecker);
  const paths = validatePayload(fs, payload);
  validateUniversalExecutable(fs, installerCli, run, "installer CLI");
  validatePayloadChecker(fs, payloadChecker);
  verifyPayloadReleaseContract(run, payloadChecker, payload, teamId, installerCli);
  assertEmptyOutput(fs, output);

  const stageId = fingerprint(fs, Object.entries(paths).filter(([label]) => label !== "driverTerms"), installerCli);
  const work = fs.mkdtempSync(join(output, ".mixrelay-pkg-build-"));
  try {
    const artifact = buildInstaller({ fs, run, work, packageRoot: join(work, "installer-root"), stagingId: stageId, paths, installerCli, teamId, version, signingIdentity });
    const installer = join(output, artifact.filename);
    const published = [];
    try {
      const identity = fs.lstatSync(artifact.productPath);
      fs.linkSync(artifact.productPath, installer);
      published.push({ path: installer, identity });
      fs.unlinkSync(artifact.productPath);
    } catch (error) {
      for (const { path, identity } of published) {
        try {
          const current = fs.lstatSync(path);
          if (current.dev === identity.dev && current.ino === identity.ino) {
            if (current.isDirectory()) fs.rmSync(path, { recursive: true });
            else if (current.isFile()) fs.unlinkSync(path);
          }
        } catch { /* Keep the original publication failure; do not broaden cleanup. */ }
      }
      throw error;
    }
    return {
      status: "signed-artifacts-built",
      installer,
      teamId,
      version,
      stagingIds: { installer: stageId },
    };
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

export function main(args = process.argv.slice(2)) {
  try {
    const parsed = parseArgs(args);
    if (parsed.help) {
      console.log(usage());
      return 0;
    }
    if (process.platform !== "darwin") throw new Error("signed macOS package building must run on macOS");
    const result = buildPackages({
      payload: parsed.payload,
      installerCli: parsed.installerCli,
      payloadChecker: parsed.payloadChecker,
      teamId: parsed.teamId,
      version: parsed.version,
      signingIdentity: parsed.signingIdentity,
      output: parsed.output,
    });
    console.log(`Built one signed macOS installation and update package in ${parsed.output}`);
    return result.status === "signed-artifacts-built" ? 0 : 1;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = main();
}
