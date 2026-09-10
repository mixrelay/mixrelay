import assert from 'node:assert/strict';
import test from 'node:test';
import { ratePolicyHeader, ratePolicyMiniport, ratePolicySpeaker, ratePolicyCapture, ratePolicyCaptureFormats } from './sysvad-rate-policy.mjs';

test('policy lifetime unregisters the exact callback owner before PortCls release', () => {
  const header = ratePolicyHeader('    PPORTEVENTS                         m_pPortEvents;');
  assert.match(header, /IsSystemRenderPin\(pin\)/);
  assert.match(header, /IsSystemCapturePin\(pin\)/);
  assert.doesNotMatch(header, /MrTransport(?:Forced|Preferred)Rate/);
  assert.match(header, /KSEVENT_PINCAPS_FORMATCHANGE, TRUE, pin/);
  assert.throws(() => ratePolicyHeader(''));
  assert.throws(() => ratePolicyHeader(header));
});

const miniport = `    if (m_pPortEvents)
    {
        m_pPortEvents->Release();
    // Format change handler is only required for bluetooth HFP implementation
    cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);
    cbMinSize = modeInfo->DefaultFormat->FormatSize;
RtlCopyMemory(PropertyRequest->Value, modeInfo->DefaultFormat, modeInfo->DefaultFormat->FormatSize);
    UNREFERENCED_PARAMETER(PinId);
    UNREFERENCED_PARAMETER(ResultantFormat);`;
test('actual miniport negotiation selects descriptor entries without editing shared mode tables', () => {
  const result = ratePolicyMiniport(miniport);
  assert.ok(result.indexOf('nullptr, this)') < result.indexOf('m_pPortEvents->Release()'));
  assert.match(result, /MrBridgeDefaultFormat\(proposedDefault, formats, count\)/);
  assert.match(result, /MrBridgeIntersect\(ClientDataRange, MyDataRange/);
  assert.match(result, /forced && format->WaveFormatEx.nSamplesPerSec != forced/);
  assert.doesNotMatch(result, /modeInfo->DefaultFormat\s*=/);
  assert.throws(() => ratePolicyMiniport(miniport + miniport));
  assert.throws(() => ratePolicyMiniport(result));
});

test('both filter descriptors advertise the existing PortCls formatchange handler', () => {
  const speaker = ratePolicySpeaker('DEFINE_PCAUTOMATION_TABLE_PROP(AutomationSpeakerWaveFilter, PropertiesSpeakerWaveFilter);');
  const capture = ratePolicyCapture('static\nPCEVENT_ITEM EventsMicArrayWaveFilter[] =\n{\n originalSoundDetector\n};');
  for (const text of [speaker, capture]) {
    assert.match(text, /KSEVENT_PINCAPS_FORMATCHANGE/);
    assert.match(text, /CMiniportWaveRT_EventHandler_PinCapsChange/);
  }
  assert.match(capture, /originalSoundDetector/);
  assert.throws(() => ratePolicySpeaker(speaker));
  assert.throws(() => ratePolicyCapture(capture));
});

test('raw capture gains an actual 44.1k descriptor while its last/default entry stays 48k', () => {
  for (const channels of [1, 2, 16, 64]) {
    const entry = `\n    {\n        {},\n        {{ WAVE_FORMAT_EXTENSIBLE,
                ${channels}, 48000, ${channels * 192000}, ${channels * 4}, 32,
                sizeof(WAVEFORMATEXTENSIBLE)-sizeof(WAVEFORMATEX)
        },32,0,STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM)}\n    },`;
    const table = 'KSDATAFORMAT_WAVEFORMATEXTENSIBLE MicArrayPinSupportedDeviceFormats[] =\n{' +
      entry.repeat(9) + '\n};\n MICARRAY_RAW_SAMPLE_RATE,\n MICARRAY_RAW_SAMPLE_RATE';
    const result = ratePolicyCaptureFormats(table);
    assert.equal((result.match(/WAVE_FORMAT_EXTENSIBLE/g) ?? []).length, 10);
    assert.ok(result.includes(`${channels}, 44100, ${channels * 176400}, ${channels * 4}, 32,`));
    assert.ok(result.lastIndexOf(`${channels}, 48000`) > result.indexOf(`${channels}, 44100`));
    assert.match(result, /44100,\n        MICARRAY_RAW_SAMPLE_RATE/);
    assert.throws(() => ratePolicyCaptureFormats(result));
    assert.throws(() => ratePolicyCaptureFormats(table.replaceAll(`${channels * 192000}`, '0')));
  }
});
