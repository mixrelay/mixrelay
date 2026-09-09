import assert from 'node:assert/strict';
import test from 'node:test';
import { validateProfile, rewriteFormats, profilePairs, profileCapture } from './sysvad-endpoint-profile.mjs';

const format = `KSDATAFORMAT_WAVEFORMATEXTENSIBLE Example[] =
{
  { {}, { { WAVE_FORMAT_EXTENSIBLE,
    2, // channels
    48000, 192000, 4, 16, sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)
  }, 16, KSAUDIO_SPEAKER_STEREO, STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM) } }
};`;
test('native PCM profiles compute channel width, byte rate and discrete mask together', () => {
  for (const channels of [1, 2, 8, 16, 32, 64]) {
    const output = rewriteFormats(format, 'Example', channels, 1);
    assert.ok(output.includes(`${channels}, 48000, ${channels * 96000}, ${channels * 2}, 16,`));
    assert.ok(output.includes(channels === 1 ? '16, KSAUDIO_SPEAKER_MONO,' : channels === 2 ? '16, KSAUDIO_SPEAKER_STEREO,' : '16, 0,'));
    // Idempotent arithmetic is essential when checking generated profile output.
    assert.equal(rewriteFormats(output, 'Example', channels, 1), output);
  }
});
test('invalid profiles and changed/malformed upstream tables fail closed', () => {
  for (const channels of [0, -1, 65, 2.5, '64', null]) assert.throws(() => validateProfile({ version: 1, renderChannels: channels, captureChannels: 16 }));
  assert.throws(() => validateProfile({ version: 2, renderChannels: 64, captureChannels: 16 }));
  assert.throws(() => validateProfile({ version: 1, renderChannels: 64, captureChannels: 16, install: true }));
  assert.throws(() => rewriteFormats(format, 'Missing', 64, 1));
  assert.throws(() => rewriteFormats(format, 'Example', 64, 2));
  assert.throws(() => rewriteFormats(format.replace('192000', '100000'), 'Example', 64, 1));
});
test('capture profiles preserve raw PCM width and disable the unbridged keyword pin', () => {
  const entry = format.slice(format.indexOf('  {'), format.lastIndexOf('\n};'));
  const raw = entry.replace('192000, 4, 16,', '384000, 8, 32,').replace('}, 16,', '}, 32,');
  const capture = `#define MICARRAY_RAW_CHANNELS 2
#define MICARRAY_PROCESSED_CHANNELS 1
#define MICARRAY_DEVICE_MAX_CHANNELS 2
KSDATAFORMAT_WAVEFORMATEXTENSIBLE MicArrayPinSupportedDeviceFormats[] =
{
${Array(8).fill(entry).join(',\n')},
${raw}
};
// Keyword Detector Streaming Pin (Capture) KSPIN_WAVEIN_KEYWORD
    {
        1,
        1,
        0,
    }`;
  const output = profileCapture(capture, 16);
  assert.ok(output.includes('16, 48000, 1536000, 32, 16,'));
  assert.ok(output.includes('16, 48000, 3072000, 64, 32,'));
  assert.match(output, /KSPIN_WAVEIN_KEYWORD\n\s*\{\s*0,\s*0,\s*0,/);
  assert.ok(output.includes('#define MICARRAY_DEVICE_MAX_CHANNELS 16'));
});
test('only bridged endpoint registrations survive, without offload advertising', () => {
  const pairs = `ENDPOINT_MINIPAIR SpeakerMiniports = { ENDPOINT_OFFLOAD_SUPPORTED, };
PENDPOINT_MINIPAIR  g_RenderEndpoints[] =
{
&SpeakerMiniports, &HdmiMiniports,
};
PENDPOINT_MINIPAIR  g_CaptureEndpoints[] =
{
&MicInMiniports, &MicArray1Miniports,
};`;
  const output = profilePairs(pairs);
  assert.ok(output.includes('ENDPOINT_NO_FLAGS'));
  assert.doesNotMatch(output, /&HdmiMiniports|&MicInMiniports/);
  assert.ok(output.includes('&SpeakerMiniports'));
  assert.ok(output.includes('&MicArray1Miniports'));
  assert.throws(() => profilePairs(pairs + pairs));
});
