import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { platform, arch, release } from "node:os";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export function buildPlan(os, mode) {
  if (!["darwin", "linux", "win32"].includes(os)) throw new Error(`Unsupported OS: ${os}`);
  if (!["build", "test"].includes(mode)) throw new Error("Expected build or test");
  const commands = [
    ["rustc", "--version", "--verbose"],
    ["pnpm", "--version"],
    ["pnpm", "--filter", "@mixrelay/control-ui", "check:boundaries"],
    ["pnpm", "build"],
    ["cargo", "build", "--locked", "--workspace"],
  ];
  if (mode === "test") {
    commands.push(["cargo", "test", "--locked", "-p", "truce-rack-vst3", "-p", "truce-rack-clap", "--lib"]);
    // Platform adapters live in the engine binary, not its portable library.
    // Omitting --bins silently skips platform adapter tests.
    commands.push(["cargo", "test", "--locked", "--workspace", "--lib", "--bins"]);
    commands.push(["pnpm", "--filter", "@mixrelay/control-ui", "exec", "vitest", "run"]);
    if (os === "darwin") commands.push(["pnpm", "check:native"]);
    if (os !== "win32") commands.push(["bash", "scripts/check-pcm-native.sh"]);
  }
  return commands;
}

export function runChecks(os, mode, run) {
  const steps = [];
  for (const [command, ...args] of buildPlan(os, mode)) {
    const start = performance.now();
    const result = run(command, args);
    steps.push({ command: [command, ...args], exitCode: result.status,
      error: result.error?.message, signal: result.signal,
      durationMs: Math.round(performance.now() - start) });
    if (result.error || result.status !== 0) return { status: "failed", steps };
  }
  return { status: "passed", steps };
}

function main() {
  const mode = process.argv[2] ?? "build";
  // Validate before creating reports or starting subprocesses.
  buildPlan(platform(), mode);
  let distro;
  if (platform() === "linux") distro = readFileSync("/etc/os-release", "utf8");
  const startedAt = new Date().toISOString();
  const result = runChecks(platform(), mode, (command, args) => {
    console.log(`\n> ${command} ${args.join(" ")}`);
    return spawnSync(command, args, {
      cwd: root, stdio: "inherit",
      // pnpm is a .cmd shim on Windows. All command/argument values above are
      // fixed program literals, never VM names, user input, or repository data.
      shell: platform() === "win32" && command === "pnpm",
    });
  });
  const report = { startedAt, endedAt: new Date().toISOString(), mode,
    os: platform(), arch: arch(), kernel: release(), distro, ...result,
    notVerified: ["driver install/signing", "live audio routing", "plugin editor UI",
      "hardware latency/dropouts", "memory endurance", "release packaging"] };
  const directory = resolve(root, ".lab/reports");
  mkdirSync(directory, { recursive: true });
  const path = resolve(directory, `${startedAt.replaceAll(":", "-")}-${platform()}-${arch()}.json`);
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`\n${result.status}: ${path}\nNative audio and GUI are NOT verified by this check.`);
  process.exitCode = result.status === "passed" ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
