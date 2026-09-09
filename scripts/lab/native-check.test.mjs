import assert from "node:assert/strict";
import test from "node:test";
import { buildPlan, runChecks } from "./native-check.mjs";

for (const os of ["linux", "darwin", "win32"]) {
  test(`${os} builds the desktop and engine, with locked dependencies`, () => {
    assert.ok(buildPlan(os, "build").some(c => c.join(" ") === "cargo build --locked --workspace"));
    assert.equal(buildPlan(os, "build").some(c => c.includes("test")), false);
  });
  test(`${os} runs tests without installing a driver`, () => {
    const plan = buildPlan(os, "test");
    assert.ok(plan.some(c => c.includes("test")));
    assert.ok(plan.some(c => c.join(" ") === "cargo test --locked --workspace --lib --bins"));
    assert.equal(plan.some(c => c.join(" ").includes("install-")), false);
    assert.equal(plan.some(c => c.join(" ") === "pnpm check:native"), os === "darwin");
    assert.equal(plan.some(c => c.includes("scripts/check-pcm-native.sh")), os !== "win32");
  });
}
test("invalid modes and hosts fail before executing anything", () => {
  assert.throws(() => buildPlan("linux", "release"));
  assert.throws(() => buildPlan("unsupported", "build"));
});
test("command failure stops the plan and cannot be reported as a pass", () => {
  let calls = 0;
  const result = runChecks("linux", "test", () => ({ status: ++calls === 2 ? 1 : 0 }));
  assert.equal(result.status, "failed");
  assert.equal(calls, 2);
});
test("missing executables and terminated children are failures", () => {
  for (const result of [{ status: null, error: new Error("ENOENT") }, { status: null, signal: "SIGTERM" }]) {
    assert.equal(runChecks("win32", "build", () => result).status, "failed");
  }
});
