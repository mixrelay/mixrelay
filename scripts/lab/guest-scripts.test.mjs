import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, statSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const job = fileURLToPath(new URL("./guest-job.sh", import.meta.url));
const installer = fileURLToPath(new URL("../install-linux-dev-audio.sh", import.meta.url));
const pipewireCheck = fileURLToPath(new URL("./guest-pipewire-check.sh", import.meta.url));
const unix = { skip: process.platform === "win32" };
function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), "tonenari-guest-test-"));
  t.after(() => rmSync(directory, { recursive: true })); // Only this test's mkdtemp directory.
  return directory;
}
test("guest job records completion and preserves command exit status", unix, t => {
  const directory = fixture(t);
  for (const code of [0, 7]) {
    const id = `result-${code}`;
    const result = spawnSync("bash", [job, id, "bash", "-c", `echo job-output; exit ${code}`], {
      env: { ...process.env, TONENARI_LAB_JOB_ROOT: directory }, encoding: "utf8",
    });
    assert.equal(result.status, code);
    const path = join(directory, `tonenari-lab-${id}`);
    assert.equal(readFileSync(join(path, "exit-code"), "utf8"), `${code}\n`);
    assert.equal(readFileSync(join(path, "output.log"), "utf8"), "job-output\n");
    assert.equal(statSync(path).mode & 0o777, 0o700);
  }
});
test("guest job rejects unsafe IDs and never overwrites previous evidence", unix, t => {
  const directory = fixture(t);
  const run = id => spawnSync("bash", [job, id, "true"], {
    env: { ...process.env, TONENARI_LAB_JOB_ROOT: directory }, encoding: "utf8",
  });
  for (const id of ["../escape", "a/b", "has space"]) assert.equal(run(id).status, 2);
  assert.equal(run("once").status, 0);
  assert.notEqual(run("once").status, 0);
  assert.equal(readFileSync(join(directory, "tonenari-lab-once", "exit-code"), "utf8"), "0\n");
});
test("Linux installer resolves default paths from HOME, not username or working directory", unix, t => {
  const directory = fixture(t);
  const bin = join(directory, "bin");
  const home = join(directory, "guest-home");
  mkdirSync(bin); mkdirSync(home);
  for (const command of ["pw-cli", "pw-dump", "wpctl", "systemctl", "systemd-analyze", "pkg-config", "cargo", "install"]) {
    writeFileSync(join(bin, command), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  }
  writeFileSync(join(bin, "uname"), "#!/bin/sh\necho Linux\n", { mode: 0o755 });
  const result = spawnSync("bash", [installer], { cwd: directory, encoding: "utf8",
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, HOME: home, USER: "wrong-relative-user",
      XDG_CONFIG_HOME: "", XDG_DATA_HOME: "" } });
  assert.equal(result.status, 0, result.stderr);
  const unit = readFileSync(join(home, ".config/systemd/user/io.tonenari.audio.engine.service"), "utf8");
  assert.ok(unit.includes(`ExecStart=${home}/.local/share/tonenari/bin/tonenari-engine-service`));
});

for (const scenario of ["occupied", "start-failed", "probe-failed", "success"]) {
  test(`PipeWire probe lifecycle: ${scenario}`, unix, t => {
    const directory = fixture(t);
    const bin = join(directory, "bin");
    mkdirSync(bin);
    const mock = (name, body) => writeFileSync(join(bin, name), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
    mock("uname", "echo Linux");
    mock("id", "echo 1000");
    mock("sha256sum", "exit 0");
    mock("pw-cat", "exit 0");
    mock("engine", "exit 0");
    // A 404 response is still an occupied API port. The first probe must not
    // use curl --fail, which would incorrectly allow replacing that service.
    mock("curl", `
      case " $* " in *" --fail "*) fail=1 ;; *) fail=0 ;; esac
      if [ ! -f "$LAB_MOCK_ROOT/queried" ]; then
        touch "$LAB_MOCK_ROOT/queried"
        [ "$LAB_SCENARIO" = occupied ] && [ "$fail" = 0 ] && exit 0
        exit 7
      fi
      exit 0`);
    mock("systemd-run", `echo start >> "$LAB_MOCK_ROOT/lifecycle"
      [ "$LAB_SCENARIO" != start-failed ]`);
    mock("systemctl", `echo "$*" >> "$LAB_MOCK_ROOT/lifecycle"`);
    mock("node", `echo probe >> "$LAB_MOCK_ROOT/lifecycle"
      [ "$LAB_SCENARIO" != probe-failed ]`);
    const result = spawnSync("bash", [pipewireCheck, join(bin, "engine"), join(bin, "node"), "/unused-probe"], {
      encoding: "utf8", timeout: 5000,
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, LAB_MOCK_ROOT: directory, LAB_SCENARIO: scenario },
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, { occupied: 2, "start-failed": 1, "probe-failed": 1, success: 0 }[scenario], result.stderr);
    if (scenario === "occupied") {
      assert.match(result.stderr, /Refusing to test over an existing engine/);
      assert.throws(() => readFileSync(join(directory, "lifecycle")), { code: "ENOENT" });
    } else {
      const lifecycle = readFileSync(join(directory, "lifecycle"), "utf8").trim().split("\n");
      assert.deepEqual(lifecycle, scenario === "start-failed" ? ["start"]
        : Array.from({ length: 3 }, () => ["start", "probe", "--user stop tonenari-lab-engine"]).flat());
    }
  });
}
