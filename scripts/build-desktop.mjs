import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { inspectPackage } from "./check-linux-package.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const desktopRoot = resolve(root, "apps/desktop");
const require = createRequire(import.meta.url);
const LINUX_BUNDLE_FORMATS = new Set(["deb", "rpm"]);
const LEMON_CONFIGURATION_KEYS = [
  "MIXRELAY_LEMON_STORE_ID",
  "MIXRELAY_LEMON_PRODUCT_ID",
  "MIXRELAY_LEMON_VARIANT_ID",
  "MIXRELAY_LEMON_CHECKOUT_URL",
];
export const DESKTOP_CODE_CHECK_TAURI_CONFIG = '{"bundle":{"externalBin":[],"resources":[]}}';

export function desktopCodeCheckEnvironment(env) {
  if (env.TAURI_CONFIG !== undefined && env.TAURI_CONFIG !== DESKTOP_CODE_CHECK_TAURI_CONFIG) {
    throw new Error("TAURI_CONFIG conflicts with the canonical desktop code-check configuration");
  }
  return {
    ...env,
    TAURI_CONFIG: DESKTOP_CODE_CHECK_TAURI_CONFIG,
    MIXRELAY_DESKTOP_CODE_CHECK: "1",
  };
}

export function assertDesktopPackagingAllowed(env) {
  if (env.MIXRELAY_DESKTOP_CODE_CHECK !== undefined
    || env.TAURI_CONFIG === DESKTOP_CODE_CHECK_TAURI_CONFIG) {
    throw new Error("Desktop packaging is disabled in the code-check environment");
  }
}

export function validateLicensingBuildEnvironment(env) {
  const values = LEMON_CONFIGURATION_KEYS.map(key => env[key]);
  if (values.every(value => value === undefined)) return;
  if (values.some(value => typeof value !== "string" || value.trim().length === 0)) {
    throw new Error("MIXRELAY_LEMON_* build configuration must be all unset or all non-empty");
  }
}

export function desktopBuildCommand(platform, args = []) {
  const { driver, buildArgs } = productArguments(args);
  if (platform === "darwin") {
    if (buildArgs.length !== 0) {
      throw new Error("macOS desktop:build produces a universal release app and accepts no overrides");
    }
    return ["zsh", "scripts/build-macos-universal-app.sh", ...(driver ? ["--driver"] : [])];
  }
  if (platform === "linux") {
    const cli = require.resolve("@tauri-apps/cli/tauri.js", { paths: [desktopRoot] });
    if (!driver) return [process.execPath, cli, "build", ...buildArgs];
    if (hasLinuxConfigOverride(buildArgs)) {
      throw new Error("Driver builds use the canonical Driver configuration and do not accept --config/-c");
    }
    const shared = JSON.parse(readFileSync(resolve(desktopRoot, "src-tauri/tauri.driver.conf.json"), "utf8"));
    const linux = JSON.parse(readFileSync(resolve(desktopRoot, "src-tauri/tauri.driver.linux.conf.json"), "utf8"));
    // Tauri applies this patch to the automatically loaded App platform config.
    // Keep the Linux file-map nulls: RFC 7396 uses them to delete App-owned files.
    const config = { ...shared, ...linux, bundle: { ...shared.bundle, ...linux.bundle } };
    return [process.execPath, cli, "build", "--config", JSON.stringify(config), ...buildArgs];
  }
  throw new Error(`Unsupported desktop build host: ${platform}`);
}

function productArguments(args) {
  const separator = args.indexOf("--");
  const options = separator === -1 ? args : args.slice(0, separator);
  const driver = options.includes("--driver");
  const buildArgs = options.filter(argument => argument !== "--driver");
  if (separator !== -1) buildArgs.push(...args.slice(separator));
  return { driver, buildArgs };
}

function bundleArguments(args) {
  const selected = [];
  let explicit = false;
  const appendValues = value => selected.push(...value.split(",").filter(Boolean));
  const appendFollowingValues = index => {
    while (index + 1 < args.length && !args[index + 1].startsWith("-")) {
      index += 1;
      appendValues(args[index]);
    }
    return index;
  };

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--") break;
    if (argument === "--no-bundle") return { explicit: true, selected: [] };
    if (argument === "-b" || argument === "--bundles") {
      explicit = true;
      index = appendFollowingValues(index);
    } else if (argument.startsWith("--bundles=")) {
      explicit = true;
      appendValues(argument.slice("--bundles=".length));
      index = appendFollowingValues(index);
    } else if (argument.startsWith("-b") && argument.length > 2) {
      explicit = true;
      appendValues(argument.slice(2).replace(/^=/, ""));
      index = appendFollowingValues(index);
    }
  }
  return { explicit, selected };
}

function hasLinuxConfigOverride(args) {
  for (const argument of args) {
    if (argument === "--") return false;
    if (argument === "--config" || argument.startsWith("--config=")
      || argument === "-c" || (argument.startsWith("-c") && argument.length > 2)) {
      return true;
    }
  }
  return false;
}

function hasExplicitLinuxBundleAction(args) {
  for (const argument of args) {
    if (argument === "--") break;
    if (argument === "--no-bundle") return true;
  }
  const { explicit, selected } = bundleArguments(args);
  return explicit && selected.length > 0 && selected.every(format => LINUX_BUNDLE_FORMATS.has(format));
}

function validateLinuxConfigBuildArguments(args) {
  const { selected } = bundleArguments(args);
  if (selected.some(format => !LINUX_BUNDLE_FORMATS.has(format))) {
    throw new Error("Linux release packages must use deb or rpm");
  }
  if (hasLinuxConfigOverride(args) && !hasExplicitLinuxBundleAction(args)) {
    throw new Error("Linux desktop builds with --config/-c must provide a valid --bundles/-b selection or --no-bundle");
  }
}

export function linuxBuildIncludesDebian(args = []) {
  const { explicit, selected } = bundleArguments(args);
  // The Linux config defaults to deb and rpm.
  return explicit ? selected.includes("deb") : true;
}

function optionValue(args, longName, shortName) {
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--") break;
    if (argument === longName || argument === shortName) return args[index + 1];
    if (argument.startsWith(`${longName}=`)) return argument.slice(longName.length + 1);
    if (argument.startsWith(`${shortName}=`)) return argument.slice(shortName.length + 1);
    if (argument.startsWith(shortName) && argument.length > shortName.length) {
      return argument.slice(shortName.length).replace(/^=/, "");
    }
  }
  return undefined;
}

export function linuxDebianBundleDirectory(args = [], env = process.env) {
  const targetRoot = env.CARGO_TARGET_DIR
    ? resolve(root, env.CARGO_TARGET_DIR)
    : resolve(root, "target");
  const target = optionValue(args, "--target", "-t");
  const separator = args.indexOf("--");
  const buildArgs = separator === -1 ? args : args.slice(0, separator);
  const profile = buildArgs.some(argument => argument === "--debug" || argument === "-d") ? "debug" : "release";
  return resolve(targetRoot, ...(target ? [target] : []), profile, "bundle", "deb");
}

export function listDebianPackages(directory, listDirectory = readdirSync) {
  let entries;
  try {
    entries = listDirectory(directory, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
  const packages = [];
  for (const entry of entries) {
    if (!entry.name.endsWith(".deb")) continue;
    if (!entry.isFile()) throw new Error(`Debian build artifact is not a regular file: ${entry.name}`);
    packages.push(resolve(directory, entry.name));
  }
  return packages.sort();
}

export async function main(
  args = process.argv.slice(2),
  platform = process.platform,
  run = spawnSync,
  inspectDebianPackage = inspectPackage,
  findDebianPackages = listDebianPackages,
  env = process.env,
) {
  try {
    const buildEnv = { ...env };
    const { driver, buildArgs } = productArguments(args);
    assertDesktopPackagingAllowed(buildEnv);
    if (!driver) validateLicensingBuildEnvironment(buildEnv);
    if (platform === "linux") validateLinuxConfigBuildArguments(buildArgs);
    const [command, ...commandArgs] = desktopBuildCommand(platform, args);
    // Tauri and its engine hook run from different directories. Resolve once
    // at the repository boundary so both build and inspect the same target tree.
    if (platform !== "darwin" && buildEnv.CARGO_TARGET_DIR) {
      buildEnv.CARGO_TARGET_DIR = resolve(root, buildEnv.CARGO_TARGET_DIR);
    }
    const result = run(command, commandArgs, {
      cwd: platform === "darwin" ? root : desktopRoot,
      stdio: "inherit",
      env: buildEnv,
    });
    if (result.error) throw result.error;
    if (result.signal) throw new Error(`Desktop build interrupted by ${result.signal}`);
    const status = result.status ?? 1;
    if (status !== 0 || platform !== "linux" || !linuxBuildIncludesDebian(buildArgs)) return status;

    const bundleDirectory = linuxDebianBundleDirectory(buildArgs, buildEnv);
    const packages = findDebianPackages(bundleDirectory).filter(path =>
      !driver || /^(?:MixRelayDriver|mix-relay-driver)_/.test(basename(path)));
    if (packages.length !== 1) {
      throw new Error(`Expected exactly one Debian package in ${bundleDirectory}; found ${packages.length}`);
    }
    const inspected = await inspectDebianPackage(packages[0]);
    if (driver && inspected?.packageName !== "mix-relay-driver") {
      throw new Error("The Driver build must produce the mix-relay-driver package");
    }
    return 0;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().then(status => { process.exitCode = status; });
}
