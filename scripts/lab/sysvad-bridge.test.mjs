import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
const read = path => readFileSync(new URL(path,import.meta.url),'utf8');
const overlay=read('./prepare-sysvad-bridge.mjs');
test('bridge overlay preserves upstream inputs and fails closed on changed anchors',()=>{
  assert.ok(overlay.includes('if (existsSync(destination)) throw'));
  assert.ok(overlay.includes("createHash('sha256')"));
  assert.ok(overlay.includes('text.split(from).length !== 2'));
  assert.ok(overlay.includes("join(reference,'LICENSE')"));
  assert.ok(overlay.includes('PUT_GUIDS_HERE before PortCls'));
  assert.ok(overlay.includes('/INTEGRITYCHECK'));
});
test('bridge hooks preserve nonselected pins, protected audio policy and frame alignment',()=>{
  for (const marker of ['eSpeakerDevice','IsSystemRenderPin(Pin_)','eMicArrayDevice1',
    'IsSystemCapturePin(Pin_)','remainder * 1000','TnTransportReleaseStream(m_bCapture)',
    'if (drmRights->CopyProtect) return STATUS_NOT_SUPPORTED','m_tnProtected, 0, 0'])
    assert.ok(overlay.includes(marker),marker);
  assert.ok(overlay.includes('#define SPEAKER_MAX_INPUT_SYSTEM_STREAMS            1'));
  assert.ok(overlay.includes('#define MICARRAY_MAX_INPUT_STREAMS              1'));
  const bridge=read('../../native/windows/sysvad-bridge/TnBridge.cpp');
  assert.doesNotMatch(bridge,/wave->nSamplesPerSec != (44100|48000|96000)/);
  assert.ok(bridge.includes('TnTransportReserveStream(capture, wave->nChannels, wave->nSamplesPerSec)'));
});
test('native PCM conversion has no floating-point arithmetic or allocation',()=>{
  const pcm=read('../../native/windows/virtual-audio-driver/include/ToneNariPcm.h')
    .replace(/\/\/[^\n]*/g,'');
  assert.doesNotMatch(pcm,/\b(float|double|new|malloc|ExAllocatePool2)\b/);
  assert.ok(pcm.includes('exponent == 255 && fraction'));
  assert.ok(pcm.includes('frames - plan.count()'));
});
test('VM bridge install and audio probe cannot replace baseline or use default hardware',()=>{
  const install=read('./guest-windows-sysvad-bridge-install.cmd');
  assert.ok(install.includes('Root\\tonenari_SysvadBridge'));
  assert.doesNotMatch(install,/devcon\.exe (update|remove)|bcdedit|Set-MpPreference|ExecutionPolicy/);
  const audio=read('./guest-windows-sysvad-bridge-audio.cmd');
  assert.ok(audio.includes('DEVPKEY_Device_Parent'));
  assert.ok(audio.includes('refusing default device fallback'));
  assert.doesNotMatch(audio,/FriendlyName -eq/);
  const probe=read('./SysvadBridgeProbe.cpp');
  assert.doesNotMatch(probe,/GetDefaultAudioEndpoint/);
  assert.ok(probe.includes('(external || owner->matchedRender>48000) && matched>48000'));
  assert.ok(probe.includes('NOT product engine qualification'));
});
test('bridge lifecycle test and cleanup remain bounded and identity-scoped',()=>{
  const audio=read('./guest-windows-sysvad-bridge-audio.cmd');
  assert.ok(audio.includes('$i -lt 20'));
  assert.ok(audio.includes('--owner-exit; if ($LASTEXITCODE -ne 0)'));
  const probe=read('./SysvadBridgeProbe.cpp');
  assert.ok(probe.includes('read+i<24000'));
  assert.ok(probe.includes('frames+i<24000'));
  assert.ok(probe.includes('std::fabs(left+2*right)<0.001f'));
  const cleanup=read('./cleanup-windows-sysvad-bridge-audio-task.cmd');
  for (const marker of ['712A936E-9D91-4455-A8A9-A1F65EB60719',
    'ToneNariLabSysvadBridgeAudio','sysvad-bridge-audio-v11\\exit-code',
    'SecurityIdentifier',"$t.State -eq 'Running'",'$owner -ne $expected',
    'Task removal not confirmed']) assert.ok(cleanup.includes(marker),marker);
  assert.doesNotMatch(cleanup,/devcon|pnputil|Stop-Process|bcdedit/);
});
