// Native descriptor profiles are a lab build boundary, not runtime channel control.
// Pinned upstream input is verified by prepare-sysvad-bridge before these transforms.
import assert from 'node:assert/strict';

export function validateProfile(profile) {
  assert.deepEqual(Object.keys(profile).sort(), ['captureChannels', 'renderChannels', 'version']);
  assert.equal(profile.version, 1);
  for (const side of ['renderChannels', 'captureChannels']) {
    assert.ok(Number.isSafeInteger(profile[side]) && profile[side] >= 1 && profile[side] <= 64, side);
  }
  return profile;
}

function once(text, pattern, replacement) {
  let count = 0;
  const output = text.replace(pattern, (...args) => { count++; return typeof replacement === 'function' ? replacement(...args) : replacement; });
  assert.equal(count, 1, `Nonunique profile anchor: ${pattern}`);
  return output;
}
function define(text, name, value) {
  return once(text, new RegExp(`^#define ${name}[^\\n]*$`, 'gm'), `#define ${name} ${value}`);
}

// Parse/rebuild the *actual* WAVEFORMATEXTENSIBLE initializers. Keep rates, bit
// containers, valid bits and table indices used by mode-default references intact.
// No speaker positions are invented for discrete multichannel routing.
export function rewriteFormats(text, arrayName, channels, expectedCount) {
  assert.ok(Number.isInteger(channels) && channels >= 1 && channels <= 64);
  return once(text, new RegExp(`(KSDATAFORMAT_WAVEFORMATEXTENSIBLE ${arrayName}\\[\\] =\\n\\{)([\\s\\S]*?)(\\n\\};)`, 'g'),
    (_, prefix, body, suffix) => {
      let count = 0;
      const result = body.replace(/WAVE_FORMAT_EXTENSIBLE,([\s\S]*?)STATICGUIDOF\(KSDATAFORMAT_SUBTYPE_PCM\)/g, (_, fields) => {
        const clean = fields.replace(/\/\/[^\n]*/g, '').trim();
        const match = clean.match(/^(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*sizeof\(WAVEFORMATEXTENSIBLE\)\s*-\s*sizeof\(WAVEFORMATEX\)\s*\},\s*(\d+)\s*,\s*(KSAUDIO_SPEAKER_\w+|0)\s*,\s*$/);
        assert.ok(match, `Unrecognized PCM initializer in ${arrayName}`);
        const [, oldChannels, rate, oldBytes, oldAlign, bits, valid] = match;
        assert.equal(Number(oldAlign), Number(oldChannels) * Number(bits) / 8);
        assert.equal(Number(oldBytes), Number(rate) * Number(oldAlign));
        assert.ok(Number(bits) === 16 || Number(bits) === 32);
        const align = channels * Number(bits) / 8;
        const mask = channels === 1 ? 'KSAUDIO_SPEAKER_MONO' : channels === 2 ? 'KSAUDIO_SPEAKER_STEREO' : '0';
        count++;
        return `WAVE_FORMAT_EXTENSIBLE,\n                ${channels}, ${rate}, ${Number(rate) * align}, ${align}, ${bits},\n                sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)\n            },\n            ${valid}, ${mask}, STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM)`;
      });
      assert.equal(count, expectedCount, `Format table size changed: ${arrayName}`);
      return prefix + result + suffix;
    });
}

export function profileSpeaker(text, channels) {
  for (const name of ['SPEAKER_DEVICE_MAX_CHANNELS', 'SPEAKER_HOST_MAX_CHANNELS', 'SPEAKER_OFFLOAD_MAX_CHANNELS']) text = define(text, name, channels);
  text = define(text, 'SPEAKER_MAX_INPUT_OFFLOAD_STREAMS', 0);
  text = define(text, 'SPEAKER_MAX_OUTPUT_LOOPBACK_STREAMS', 0);
  for (const [name, count] of [['SpeakerAudioEngineSupportedDeviceFormats', 5], ['SpeakerHostPinSupportedDeviceFormats', 6], ['SpeakerOffloadPinSupportedDeviceFormats', 2]]) text = rewriteFormats(text, name, channels, count);
  return text;
}
export function profileCapture(text, channels) {
  for (const name of ['MICARRAY_RAW_CHANNELS', 'MICARRAY_PROCESSED_CHANNELS', 'MICARRAY_DEVICE_MAX_CHANNELS']) text = define(text, name, channels);
  text = rewriteFormats(text, 'MicArrayPinSupportedDeviceFormats', channels, 9);
  // Preserve pin IDs so upstream physical connections remain valid, but do not
  // allow opening the unbridged sample keyword/tone-generator path.
  return once(text, /(\/\/ Keyword Detector Streaming Pin[^\n]*\n\s*\{)\s*1,\s*1,/g,
    (_, prefix) => `${prefix}\n        0,\n        0,`);
}

export function profilePairs(text) {
  text = once(text, /(PENDPOINT_MINIPAIR  g_RenderEndpoints\[\] =\s*\{)[\s\S]*?\n\};/g,
    (_, prefix) => `${prefix}\n    &SpeakerMiniports,\n};`);
  text = once(text, /(PENDPOINT_MINIPAIR  g_CaptureEndpoints\[\] =\s*\{)[\s\S]*?\n\};/g,
    (_, prefix) => `${prefix}\n    &MicArray1Miniports,\n};`);
  // There is no DSP offload processor or microphone-array geometry in this path.
  return once(text, /(ENDPOINT_MINIPAIR SpeakerMiniports =[\s\S]*?)ENDPOINT_OFFLOAD_SUPPORTED,/g,
    (_, prefix) => `${prefix}ENDPOINT_NO_FLAGS,`);
}
