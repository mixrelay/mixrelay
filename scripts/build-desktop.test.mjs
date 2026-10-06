import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  DESKTOP_CODE_CHECK_TAURI_CONFIG,
  desktopCodeCheckEnvironment,
  desktopBuildCommand,
  linuxBuildIncludesDebian,
  linuxDebianBundleDirectory,
  listDebianPackages,
  main,
  validateLicensingBuildEnvironment,
} from "./build-desktop.mjs";

const licensingKeys = [
  "MIXRELAY_LEMON_STORE_ID",
  "MIXRELAY_LEMON_PRODUCT_ID",
  "MIXRELAY_LEMON_VARIANT_ID",
  "MIXRELAY_LEMON_CHECKOUT_URL",
];

test("desktop code checks use only the canonical no-payload Tauri override", () => {
  const environment = desktopCodeCheckEnvironment({ CARGO_TARGET_DIR: "target/fixture" });
  assert.equal(environment.TAURI_CONFIG, DESKTOP_CODE_CHECK_TAURI_CONFIG);
  assert.equal(environment.MIXRELAY_DESKTOP_CODE_CHECK, "1");
  assert.equal(environment.CARGO_TARGET_DIR, "target/fixture");
  assert.deepEqual(desktopCodeCheckEnvironment({ TAURI_CONFIG: DESKTOP_CODE_CHECK_TAURI_CONFIG }), {
    TAURI_CONFIG: DESKTOP_CODE_CHECK_TAURI_CONFIG,
    MIXRELAY_DESKTOP_CODE_CHECK: "1",
  });
  assert.throws(() => desktopCodeCheckEnvironment({ TAURI_CONFIG: '{"bundle":{"resources":["drivers/"]}}' }),
    /conflicts with the canonical desktop code-check configuration/);
});

test("licensing build configuration is either wholly absent or wholly non-empty", () => {
  const complete = {
    MIXRELAY_LEMON_STORE_ID: "1",
    MIXRELAY_LEMON_PRODUCT_ID: "2",
    MIXRELAY_LEMON_VARIANT_ID: "3",
    MIXRELAY_LEMON_CHECKOUT_URL: "https://mixrelay.lemonsqueezy.com/buy/product",
  };
  assert.doesNotThrow(() => validateLicensingBuildEnvironment({}));
  assert.doesNotThrow(() => validateLicensingBuildEnvironment(complete));
  for (const key of licensingKeys) {
    const partial = { ...complete };
    delete partial[key];
    assert.throws(() => validateLicensingBuildEnvironment(partial), /all unset or all non-empty/);
    for (const empty of ["", "   "]) {
      assert.throws(() => validateLicensingBuildEnvironment({ ...complete, [key]: empty }),
        /all unset or all non-empty/);
    }
  }
});

test("desktop build rejects partial licensing configuration before spawning and forwards the checked environment", async t => {
  const saved = Object.fromEntries(licensingKeys.map(key => [key, process.env[key]]));
  t.after(() => {
    for (const key of licensingKeys) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
  });
  for (const key of licensingKeys) delete process.env[key];
  process.env.MIXRELAY_LEMON_STORE_ID = "1";
  const errors = [];
  t.mock.method(console, "error", message => errors.push(message));
  assert.equal(await main([], "darwin", () => assert.fail("partial configuration must not spawn")), 1);
  assert.match(errors[0], /all unset or all non-empty/);
  process.env.MIXRELAY_LEMON_PRODUCT_ID = "2";
  process.env.MIXRELAY_LEMON_VARIANT_ID = "3";
  process.env.MIXRELAY_LEMON_CHECKOUT_URL = "https://mixrelay.lemonsqueezy.com/buy/product";
  assert.equal(await main([], "darwin", (_command, _args, options) => {
    for (const key of licensingKeys) assert.equal(options.env[key], process.env[key]);
    return { status: 0 };
  }), 0);
});

test("macOS defaults exclusively to the checked universal build", () => {
  assert.deepEqual(desktopBuildCommand("darwin"), ["zsh", "scripts/build-macos-universal-app.sh"]);
  assert.deepEqual(desktopBuildCommand("darwin", ["--driver"]), ["zsh", "scripts/build-macos-universal-app.sh", "--driver"]);
  for (const args of [["--target", "aarch64-apple-darwin"], ["--debug"], ["--config", "other.json"]]) {
    assert.throws(() => desktopBuildCommand("darwin", args), /accepts no overrides/);
  }
});

test("macOS Driver alone declares an agent app in its canonical bundle plist", () => {
  const directory = new URL("../apps/desktop/src-tauri/", import.meta.url);
  const driver = JSON.parse(readFileSync(new URL("tauri.driver.conf.json", directory), "utf8"));
  const graph = JSON.parse(readFileSync(new URL("tauri.conf.json", directory), "utf8"));
  assert.equal(driver.bundle.macOS.infoPlist, "Info.driver.plist");
  const driverPlist = readFileSync(new URL(driver.bundle.macOS.infoPlist, directory), "utf8");
  assert.match(driverPlist, /<key>LSUIElement<\/key>\s*<true\s*\/>/);
  assert.notEqual(graph.bundle.macOS.infoPlist, driver.bundle.macOS.infoPlist);
  const graphPlist = readFileSync(new URL(graph.bundle.macOS.infoPlist ?? "Info.plist", directory), "utf8");
  assert.doesNotMatch(graphPlist, /<key>LSUIElement<\/key>/);
});

test("Linux Driver builds use both canonical profiles and retain RFC 7396 App-file deletions", () => {
  const command = desktopBuildCommand("linux", ["--driver", "--bundles", "deb"]);
  assert.equal(command[2], "build");
  assert.equal(command[3], "--config");
  const config = JSON.parse(command[4]);
  const shared = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.driver.conf.json", import.meta.url), "utf8"));
  const linux = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.driver.linux.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(config, { ...shared, ...linux, bundle: { ...shared.bundle, ...linux.bundle } });
  assert.equal(config.identifier, "io.mixrelay.driver.desktop");
  assert.equal(config.mainBinaryName, "mixrelay-driver-manager");
  assert.match(config.build.beforeBuildCommand, /desktop:prepare-driver --profile release/);
  assert.doesNotMatch(config.build.beforeBuildCommand, /desktop:prepare-engine/);
  assert.deepEqual(config.bundle.externalBin, ["binaries/mixrelay-driver-service"]);
  for (const format of ["deb", "rpm"]) {
    const files = config.bundle.linux[format].files;
    assert.equal(files["/usr/lib/mixrelay/uninstall.sh"], null);
    assert.equal(files["/usr/share/wireplumber/scripts/mixrelay-source-mute.lua"], null);
    assert.ok(files["/usr/share/mixrelay-driver/version.json"]);
  }
  assert.deepEqual(command.slice(5), ["--bundles", "deb"]);
  assert.throws(() => desktopBuildCommand("linux", ["--driver", "--config", "other.json"]), /canonical Driver configuration/);
  assert.deepEqual(desktopBuildCommand("linux", ["--", "--driver"]).slice(2), ["build", "--", "--driver"]);
});

test("free Driver builds do not require the paid App licensing build environment", async () => {
  const env = { MIXRELAY_LEMON_STORE_ID: "1" };
  assert.equal(await main(["--driver"], "darwin", (_command, args) => {
    assert.deepEqual(args, ["scripts/build-macos-universal-app.sh", "--driver"]);
    return { status: 0 };
  }, undefined, undefined, env), 0);
});

test("Linux retains Tauri build arguments", () => {
  const linux = desktopBuildCommand("linux", ["--bundles", "deb"]);
  assert.equal(linux[0], process.execPath);
  assert.match(linux[1], /tauri\.js$/);
  assert.deepEqual(linux.slice(2), ["build", "--bundles", "deb"]);
  assert.throws(() => desktopBuildCommand("freebsd"), /Unsupported/);
});

test("build runs from the repository and propagates its exit status", async () => {
  const calls = [];
  const status = await main([], "darwin", (...args) => {
    calls.push(args);
    return { status: 17 };
  });
  assert.equal(status, 17);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][2].stdio, "inherit");
  assert.equal(calls[0][2].cwd, fileURLToPath(new URL("..", import.meta.url)).replace(/\/$/, ""));
});

test("native Tauri builds run in the desktop package without a shell", async () => {
  await main(["--no-bundle"], "linux", (_command, _args, options) => {
    assert.equal(options.cwd, fileURLToPath(new URL("../apps/desktop", import.meta.url)));
    assert.equal(options.shell, undefined);
    return { status: 0 };
  });
});

test("desktop packaging refuses the code-check environment before spawning", async t => {
  const errors = [];
  t.mock.method(console, "error", message => errors.push(message));
  const status = await main([], "linux", () => assert.fail("code-check artifacts must not be packaged"),
    undefined, undefined, { MIXRELAY_DESKTOP_CODE_CHECK: "1" });
  assert.equal(status, 1);
  assert.match(errors[0], /disabled in the code-check environment/);
});

test("desktop packaging rejects canonical code-only config even without its marker", async t => {
  const errors = [];
  t.mock.method(console, "error", message => errors.push(message));
  assert.equal(await main([], "linux", () => assert.fail("must not spawn"),
    undefined, undefined, { TAURI_CONFIG: DESKTOP_CODE_CHECK_TAURI_CONFIG }), 1);
  assert.match(errors[0], /disabled in the code-check environment/);
});

test("Linux Debian output selection and target path follow Tauri build options", () => {
  assert.equal(linuxBuildIncludesDebian([]), true);
  assert.equal(linuxBuildIncludesDebian(["--bundles", "deb", "rpm"]), true);
  assert.equal(linuxBuildIncludesDebian(["--bundles=appimage,rpm"]), false);
  assert.equal(linuxBuildIncludesDebian(["-bdeb"]), true);
  assert.equal(linuxBuildIncludesDebian(["--bundles=rpm", "deb"]), true);
  assert.equal(linuxBuildIncludesDebian(["-brpm", "deb"]), true);
  assert.equal(linuxBuildIncludesDebian(["-b=appimage,rpm"]), false);
  assert.equal(linuxBuildIncludesDebian(["-brpm", "--", "deb"]), false);
  assert.equal(linuxBuildIncludesDebian(["-b", "rpm"]), false);
  assert.equal(linuxBuildIncludesDebian(["--no-bundle"]), false);
  assert.equal(linuxBuildIncludesDebian(["--", "--bundles", "rpm"]), true);

  const defaultDirectory = fileURLToPath(new URL("../target/release/bundle/deb", import.meta.url));
  assert.equal(linuxDebianBundleDirectory([]), defaultDirectory);
  assert.equal(linuxDebianBundleDirectory(["--debug", "--target", "aarch64-unknown-linux-gnu"]),
    fileURLToPath(new URL("../target/aarch64-unknown-linux-gnu/debug/bundle/deb", import.meta.url)));
  const targetedDebianDirectory = fileURLToPath(new URL(
    "../target/aarch64-unknown-linux-gnu/release/bundle/deb", import.meta.url,
  ));
  for (const args of [
    ["--target=aarch64-unknown-linux-gnu"], ["-t", "aarch64-unknown-linux-gnu"],
    ["-taarch64-unknown-linux-gnu"], ["-t=aarch64-unknown-linux-gnu"],
  ]) {
    assert.equal(linuxDebianBundleDirectory(args), targetedDebianDirectory, args.join(" "));
  }
  assert.equal(linuxDebianBundleDirectory(["--", "--debug", "-d", "-taarch64-unknown-linux-gnu"]),
    defaultDirectory);
  assert.equal(linuxDebianBundleDirectory([], { CARGO_TARGET_DIR: "/fixture/mixrelay-target" }),
    "/fixture/mixrelay-target/release/bundle/deb");
  assert.equal(linuxDebianBundleDirectory([], { CARGO_TARGET_DIR: "target/custom" }),
    fileURLToPath(new URL("../target/custom/release/bundle/deb", import.meta.url)));
});

test("Linux config override without a bundle action fails before stale Debian artifacts can pass", async t => {
  const errors = [];
  t.mock.method(console, "error", message => errors.push(message));
  let builds = 0;
  let listings = 0;
  let inspections = 0;
  const status = await main(["--config", "override.json"], "linux", () => {
    builds += 1;
    return { status: 0 };
  }, async () => { inspections += 1; }, () => {
    listings += 1;
    return ["/fixture/stale-from-previous-build.deb"];
  });
  assert.equal(status, 1);
  assert.equal(builds, 0);
  assert.equal(listings, 0);
  assert.equal(inspections, 0);
  assert.match(errors[0], /provide a valid --bundles\/-b selection or --no-bundle/);
});

test("Linux config override recognizes Tauri config and bundle attached forms", async t => {
  t.mock.method(console, "error", () => {});
  for (const config of [
    ["--config", "override.json"], ["--config=override.json"],
    ["-c", "override.json"], ["-coverride.json"], ["-c=override.json"],
  ]) {
    assert.equal(await main(config, "linux", () => assert.fail("config-only build must fail before spawn")), 1);
  }

  for (const bundle of [
    ["--bundles", "deb"], ["--bundles=deb"], ["-b", "deb"], ["-bdeb"], ["-b=deb"],
    ["--bundles=rpm", "deb"], ["-brpm", "deb"], ["-b=rpm", "deb"],
  ]) {
    let inspected = false;
    const status = await main(["-c", "override.json", ...bundle], "linux",
      () => ({ status: 0 }), async () => { inspected = true; },
      () => ["/fixture/MixRelay_0.1.0_amd64.deb"]);
    assert.equal(status, 0, bundle.join(" "));
    assert.equal(inspected, true, bundle.join(" "));
  }
});

test("Linux config overrides can explicitly select non-Debian or no bundles", async () => {
  const failIfCalled = () => assert.fail("non-Debian output must not be inspected as a Debian build");
  assert.equal(await main(["--config=override.json", "-brpm"], "linux",
    () => ({ status: 0 }), failIfCalled, failIfCalled), 0);
  assert.equal(await main(["-c=override.json", "--no-bundle"], "linux",
    () => ({ status: 0 }), failIfCalled, failIfCalled), 0);
});

test("Linux product builds reject bundle formats without coordinated package removal", async t => {
  const errors = [];
  t.mock.method(console, "error", message => errors.push(message));
  for (const format of ["appimage", "tar", "updater", "all"]) {
    assert.equal(await main(["--bundles", format], "linux",
      () => assert.fail("unsupported package must not build")), 1);
  }
  assert.equal(errors.length, 4);
  assert.ok(errors.every(message => message.includes("must use deb or rpm")));
});

test("config-like arguments after the separator are forwarded, not treated as wrapper overrides", async () => {
  let builds = 0;
  let inspected = false;
  const status = await main(["--", "--config", "runner-value"], "linux", (_command, args) => {
    builds += 1;
    assert.deepEqual(args.slice(-3), ["--", "--config", "runner-value"]);
    return { status: 0 };
  }, async () => { inspected = true; }, () => ["/fixture/default.deb"]);
  assert.equal(status, 0);
  assert.equal(builds, 1);
  assert.equal(inspected, true);
});

test("relative target directory is shared by Tauri, engine hook and Debian inspection", async t => {
  const original = process.env.CARGO_TARGET_DIR;
  t.after(() => {
    if (original === undefined) delete process.env.CARGO_TARGET_DIR;
    else process.env.CARGO_TARGET_DIR = original;
  });
  process.env.CARGO_TARGET_DIR = "target/custom";
  const expectedRoot = fileURLToPath(new URL("../target/custom", import.meta.url));
  let buildEnvironment;
  assert.equal(await main([], "linux", (_command, _args, options) => {
    buildEnvironment = options.env;
    assert.equal(options.env.CARGO_TARGET_DIR, expectedRoot);
    return { status: 0 };
  }, async () => {}, directory => {
    assert.equal(directory, `${expectedRoot}/release/bundle/deb`);
    return ["/fixture/MixRelay_0.1.0_arm64.deb"];
  }), 0);
  assert.equal(process.env.CARGO_TARGET_DIR, "target/custom");
  const { buildPlan } = await import("./prepare-desktop-engine.mjs");
  const plan = buildPlan({
    profile: "release", target: "aarch64-unknown-linux-gnu", platform: "linux", env: buildEnvironment,
  });
  assert.equal(plan.output, `${expectedRoot}/aarch64-unknown-linux-gnu/release/mixrelay-engine-service`);
});

test("Debian package discovery ignores other bundles and rejects non-regular artifacts", () => {
  const entries = [
    { name: "MixRelay.deb", isFile: () => true },
    { name: "MixRelay.rpm", isFile: () => true },
  ];
  assert.deepEqual(listDebianPackages("/fixture/bundle/deb", () => entries), ["/fixture/bundle/deb/MixRelay.deb"]);
  assert.throws(() => listDebianPackages("/fixture/bundle/deb", () => [
    { name: "link.deb", isFile: () => false },
  ]), /not a regular file/);
  assert.deepEqual(listDebianPackages("/fixture/missing", () => {
    const error = new Error("missing");
    error.code = "ENOENT";
    throw error;
  }), []);
});

test("successful Linux Debian build inspects its single produced artifact before returning success", async () => {
  const calls = [];
  const inspected = [];
  const status = await main([], "linux", (command, args, options) => {
    calls.push({ command, args, options });
    return { status: 0 };
  }, async path => { inspected.push(path); }, directory => {
    assert.equal(directory, linuxDebianBundleDirectory([]));
    return ["/fixture/MixRelay_0.1.0_amd64.deb"];
  });
  assert.equal(status, 0);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].options.cwd, fileURLToPath(new URL("../apps/desktop", import.meta.url)));
  assert.deepEqual(inspected, ["/fixture/MixRelay_0.1.0_amd64.deb"]);
});

test("Linux Driver builds inspect the actual Driver package beside a prior App artifact", async () => {
  const inspected = [];
  const status = await main(["--driver", "--bundles", "deb"], "linux", (_command, args) => {
    assert.equal(args.includes("--driver"), false);
    assert.equal(JSON.parse(args[args.indexOf("--config") + 1]).identifier, "io.mixrelay.driver.desktop");
    return { status: 0 };
  }, async path => {
    inspected.push(path);
    return { packageName: "mix-relay-driver" };
  }, () => ["/fixture/MixRelay_0.1.0_amd64.deb", "/fixture/MixRelayDriver_0.1.0_amd64.deb"], {});
  assert.equal(status, 0);
  assert.deepEqual(inspected, ["/fixture/MixRelayDriver_0.1.0_amd64.deb"]);
});

test("Driver builds reject missing, ambiguous, and mislabeled Driver package artifacts", async t => {
  t.mock.method(console, "error", () => {});
  for (const packages of [
    ["/fixture/MixRelay_0.1.0_amd64.deb"],
    ["/fixture/MixRelayDriver_0.1.0_amd64.deb", "/fixture/MixRelayDriver_0.2.0_amd64.deb"],
  ]) {
    assert.equal(await main(["--driver"], "linux", () => ({ status: 0 }),
      () => assert.fail("must select one Driver artifact"), () => packages, {}), 1);
  }
  assert.equal(await main(["--driver"], "linux", () => ({ status: 0 }),
    async () => ({ packageName: "mix-relay" }),
    () => ["/fixture/MixRelayDriver_0.1.0_amd64.deb"], {}), 1);
});

test("missing or multiple Debian artifacts fail without inspecting a guessed package", async t => {
  t.mock.method(console, "error", () => {});
  for (const packages of [[], ["/fixture/one.deb", "/fixture/two.deb"]]) {
    let inspected = false;
    const status = await main(["--bundles", "deb"], "linux", () => ({ status: 0 }), () => {
      inspected = true;
    }, () => packages);
    assert.equal(status, 1);
    assert.equal(inspected, false);
  }
});

test("invalid Debian package inspection prevents a successful Linux build result", async t => {
  t.mock.method(console, "error", () => {});
  const status = await main(["--bundles=deb"], "linux", () => ({ status: 0 }), async () => {
    throw new Error("ELF architecture mismatch");
  }, () => ["/fixture/wrong-architecture.deb"]);
  assert.equal(status, 1);
});

test("non-Debian builds and nonzero Tauri builds do not invoke Debian inspection", async t => {
  t.mock.method(console, "error", () => {});
  const failIfCalled = () => assert.fail("Debian package path should not be inspected");
  assert.equal(await main(["--bundles", "rpm"], "linux", () => ({ status: 0 }), failIfCalled, failIfCalled), 0);
  assert.equal(await main(["--bundles", "deb"], "linux", () => ({ status: 9 }), failIfCalled, failIfCalled), 9);
});

test("spawn failures, signals, and missing exit status fail the build", async t => {
  t.mock.method(console, "error", () => {});
  for (const result of [{ error: new Error("missing tool") }, { signal: "SIGTERM", status: null }, { status: null }]) {
    assert.equal(await main([], "darwin", () => result), 1);
  }
  assert.equal(await main(["--debug"], "darwin", () => assert.fail("must not spawn")), 1);
});
