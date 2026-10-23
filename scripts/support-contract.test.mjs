import assert from 'node:assert/strict';
import test from 'node:test';
import { loadContract, validateContract, ciMatrices, checkGenerated, requiredScenarios } from './support-contract.mjs';

test('support registry drives native multi-architecture CI and checked-in generated consumers', () => {
  const { contract } = loadContract();
  checkGenerated(contract);
  const matrices = ciMatrices(contract);
  assert.equal(matrices.linux.include.length, 7);
  assert.deepEqual(new Set(matrices.macos.include.map(t => t.arch)), new Set(['x64', 'arm64']));
  assert.equal(matrices.linux.include.find(t => t.id === 'debian-arm64').runner, 'ubuntu-24.04-arm');
  assert.ok(!matrices.linux.include.some(t => t.id === 'arch-arm64'));
  assert.ok(requiredScenarios(contract, { os: 'macos' }).some(s => s.id === 'AUDIO-11'));
  assert.ok(!requiredScenarios(contract, { os: 'linux' }).some(s => s.id === 'AUDIO-11'));
});
test('duplicate environments, unknown artifacts and floating distro images fail contract validation', () => {
  for (const mutate of [c => c.qualificationTargets.push(c.qualificationTargets[0]),
    c => { c.qualificationTargets[0].artifact = 'missing'; },
    c => { c.codeChecks.linuxDistributions[0].image = 'debian:latest'; },
    c => { c.minimums.linux.glibc = "2.39'; false; '"; },
    c => { c.qualificationTargets[0].version = '14'; },
    c => { c.qualificationTargets.find(t => t.id === 'macos-27-arm64').arch = 'x64'; },
    c => { c.qualificationTargets[0].arch = 'universal'; }]) {
    const contract = structuredClone(loadContract().contract); mutate(contract);
    assert.throws(() => validateContract(contract));
  }
});
