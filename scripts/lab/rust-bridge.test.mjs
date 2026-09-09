import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import test from 'node:test';
const read=p=>readFileSync(new URL(p,import.meta.url),'utf8');
test('literal lab command matches its reviewed source and does not change policy',()=>{
  assert.equal(execFileSync(process.execPath,[new URL('./render-rust-bridge-command.mjs',import.meta.url).pathname],{encoding:'utf8'}),read('./windows-rust-bridge-check.cmd'));
  assert.doesNotMatch(read('./windows-rust-bridge-check.cmd'),/ExecutionPolicy|Invoke-Expression|ScriptBlock|Set-MpPreference/);
});
test('Rust integration test verifies engine graph, real PCM, meter and release',()=>{
  const check=read('./windows-rust-bridge-check.ps1');
  for(const marker of ['712A936E-9D91-4455-A8A9-A1F65EB60719','DEVPKEY_Device_Parent',
    'Control port already in use','--external-engine','$null=$probe.Handle','-6.020599913',
    'sawMeter','observedDeviceSampleRate','--owner-exit','Removing graph did not release',
    'reprobe.out','Reattached engine did not restore real PCM','rerelease.out'])
    assert.ok(check.includes(marker),marker);
  assert.doesNotMatch(check,/Stop-Process -Name|GetDefaultAudioEndpoint/);
});
test('Windows transport stays scoped and owns private copy bounds',()=>{
  const native=read('../../apps/engine-service/src/windows_transport.rs');
  assert.ok(native.includes('ToneNariSysvadBridge'));
  assert.doesNotMatch(native,/UnmapViewOfFile|VirtualFree|GetDefaultAudioEndpoint/);
  for(const marker of ['valid_counters','Ordering::Acquire','Ordering::Release',
    'struct Handle','impl Drop for Handle','NonNull<u64>']) assert.ok(native.includes(marker));
  const engine=read('../../apps/engine-service/src/windows.rs');
  assert.ok(engine.includes('TONENARI_WINDOWS_SYSVAD_LAB'));
  assert.ok(engine.includes('new_with_initial_document'));
  assert.ok(engine.includes('virtual_overruns.load(Ordering::Acquire)'));
});
test('Rust bridge task cleanup validates completion, action and owner',()=>{
  const cleanup=read('./cleanup-windows-rust-bridge-task.cmd');
  for(const marker of ['712A936E-9D91-4455-A8A9-A1F65EB60719','ToneNariLabRustBridge',
    'windows-rust-bridge-audio-v11\\exit-code','SecurityIdentifier',
    "State -eq 'Running'",'windows-rust-bridge-check.cmd','Task removal not confirmed'])
    assert.ok(cleanup.includes(marker),marker);
  assert.doesNotMatch(cleanup,/devcon|pnputil|Stop-Process|bcdedit/);
});
