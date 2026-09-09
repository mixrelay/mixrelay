// Opt-in transformation of the hash-pinned SysVAD PortCls implementation.
// The normal bridge overlay deliberately never enables this negotiation policy.
function once(text, from, to) {
  if (text.split(from).length !== 2) throw Error(`Rate policy anchor not unique: ${from}`);
  return text.replace(from, to);
}

export function ratePolicyHeader(text) {
  if (text.includes('TnPolicyPin')) throw Error('Rate policy already applied');
  text = '#include "TnBridge.h"\n' + text;
  return once(text, '    PPORTEVENTS                         m_pPortEvents;', `    PPORTEVENTS                         m_pPortEvents;

    bool TnPolicyPin(ULONG pin) {
        return pin < m_pMiniportPair->WaveDescriptor->PinCount &&
            ((m_DeviceType == eSpeakerDevice && IsSystemRenderPin(pin)) ||
             (m_DeviceType == eMicArrayDevice1 && IsSystemCapturePin(pin)));
    }
    static void TnRateChanged(void* context) {
        auto self = static_cast<CMiniportWaveRT*>(context);
        // The policy lock owns callback lifetime. Never query policy from this
        // callback: that would reenter the same lock. PortCls owns event entries.
        for (ULONG pin = 0; pin < self->m_pMiniportPair->WaveDescriptor->PinCount; ++pin)
            if (self->TnPolicyPin(pin) && self->m_pPortEvents)
                self->GenerateEventList(const_cast<GUID*>(&KSEVENTSETID_PinCapsChange),
                    KSEVENT_PINCAPS_FORMATCHANGE, TRUE, pin, FALSE, ULONG(-1));
    }`);
}

export function ratePolicyMiniport(text) {
  text = once(text, '    if (m_pPortEvents)\n    {\n        m_pPortEvents->Release();', `    if (m_pPortEvents)
    {
        if (m_DeviceType == eSpeakerDevice || m_DeviceType == eMicArrayDevice1)
            TnTransportRateListener(m_DeviceType == eMicArrayDevice1, nullptr, this);
        m_pPortEvents->Release();`);
  text = once(text, '    // Format change handler is only required for bluetooth HFP implementation', `    if (m_pPortEvents && (m_DeviceType == eSpeakerDevice || m_DeviceType == eMicArrayDevice1))
        TnTransportRateListener(m_DeviceType == eMicArrayDevice1, TnRateChanged, this);

    // Format change handler is only required for bluetooth HFP implementation`);
  text = once(text, '    cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);', `    if (TnPolicyPin(_ulPin)) {
        if (!_pDataFormat || _pDataFormat->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX))
            return STATUS_INVALID_PARAMETER;
        const auto forced = TnTransportForcedRate();
        const auto format = reinterpret_cast<PKSDATAFORMAT_WAVEFORMATEX>(_pDataFormat);
        if (format->WaveFormatEx.nSamplesPerSec != 44100 && format->WaveFormatEx.nSamplesPerSec != 48000)
            return STATUS_NO_MATCH;
        if (forced && format->WaveFormatEx.nSamplesPerSec != forced) return STATUS_NO_MATCH;
    }
    cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);`);
  text = once(text, '    cbMinSize = modeInfo->DefaultFormat->FormatSize;', `    auto proposedDefault = modeInfo->DefaultFormat;
    if (TnPolicyPin(kspPin->PinId)) {
        PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats = nullptr;
        const auto count = GetPinSupportedDeviceFormats(kspPin->PinId, &formats);
        proposedDefault = TnBridgeDefaultFormat(proposedDefault, formats, count);
        if (!proposedDefault) return STATUS_NO_MATCH;
    }
    cbMinSize = proposedDefault->FormatSize;`);
  text = once(text, 'RtlCopyMemory(PropertyRequest->Value, modeInfo->DefaultFormat, modeInfo->DefaultFormat->FormatSize);',
    'RtlCopyMemory(PropertyRequest->Value, proposedDefault, proposedDefault->FormatSize);');
  text = once(text, '    UNREFERENCED_PARAMETER(PinId);\n    UNREFERENCED_PARAMETER(ResultantFormat);', `    if (TnPolicyPin(PinId)) {
        PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats = nullptr;
        const auto count = GetPinSupportedDeviceFormats(PinId, &formats);
        return TnBridgeIntersect(ClientDataRange, MyDataRange, formats, count,
            OutputBufferLength, ResultantFormat, ResultantFormatLength);
    }
    UNREFERENCED_PARAMETER(PinId);
    UNREFERENCED_PARAMETER(ResultantFormat);`);
  return text;
}

const eventDeclaration = `NTSTATUS CMiniportWaveRT_EventHandler_PinCapsChange(_In_ PPCEVENT_REQUEST EventRequest);\n`;
const event = `    {
        &KSEVENTSETID_PinCapsChange,
        KSEVENT_PINCAPS_FORMATCHANGE,
        KSEVENT_TYPE_ENABLE | KSEVENT_TYPE_BASICSUPPORT,
        CMiniportWaveRT_EventHandler_PinCapsChange,
    },`;
export function ratePolicySpeaker(text) {
  return once(text, 'DEFINE_PCAUTOMATION_TABLE_PROP(AutomationSpeakerWaveFilter, PropertiesSpeakerWaveFilter);',
    `${eventDeclaration}static PCEVENT_ITEM EventsToneNariSpeaker[] = {\n${event}\n};
DEFINE_PCAUTOMATION_TABLE_PROP_EVENT(AutomationSpeakerWaveFilter, PropertiesSpeakerWaveFilter, EventsToneNariSpeaker);`);
}
export function ratePolicyCapture(text) {
  if (text.includes('KSEVENT_PINCAPS_FORMATCHANGE')) throw Error('Rate policy already applied');
  text = once(text, 'static\nPCEVENT_ITEM EventsMicArrayWaveFilter[] =\n{',
    `${eventDeclaration}static\nPCEVENT_ITEM EventsMicArrayWaveFilter[] =\n{\n${event}`);
  return text;
}

export function ratePolicyCaptureFormats(text) {
  // Raw mode defaults to the LAST entry (48k/32-bit). Add a real 44.1k
  // counterpart before it, preserving all default-format references. Otherwise
  // raw capture would fail policy negotiation despite processed mode supporting
  // 44.1k. The transport already converts this exact signed PCM container.
  const start = text.indexOf('KSDATAFORMAT_WAVEFORMATEXTENSIBLE MicArrayPinSupportedDeviceFormats[] =');
  const end = text.indexOf('\n};', start);
  if (start < 0 || end < 0) throw Error('Missing capture format table');
  const table = text.slice(start, end);
  const entries = [...table.matchAll(/\n    \{\n        \{/g)];
  if (entries.length !== 9) throw Error('Capture table changed or already extended');
  const last = entries.at(-1).index;
  const raw = table.slice(last);
  let changed = 0;
  const counterpart = raw.replace(/WAVE_FORMAT_EXTENSIBLE,([\s\S]*?)sizeof\(WAVEFORMATEXTENSIBLE\)/, (_, fields) => {
    const clean = fields.replace(/\/\/[^\n]*/g, '').trim();
    const match = clean.match(/^(\d+)\s*,\s*48000\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*32\s*,\s*$/);
    if (!match || Number(match[3]) !== Number(match[1]) * 4 || Number(match[2]) !== 48000 * Number(match[3]))
      throw Error('Unrecognized raw PCM format');
    changed++;
    return `WAVE_FORMAT_EXTENSIBLE,\n                ${match[1]}, 44100, ${44100 * Number(match[3])}, ${match[3]}, 32,\n                sizeof(WAVEFORMATEXTENSIBLE)`;
  });
  if (changed !== 1) throw Error('Raw format not transformed');
  text = text.slice(0, start) + table.slice(0, last) + counterpart + raw + text.slice(end);
  const from = /MICARRAY_RAW_SAMPLE_RATE,\s*\n\s*MICARRAY_RAW_SAMPLE_RATE/g;
  if ([...text.matchAll(from)].length !== 1) throw Error('Raw range anchor changed');
  return text.replace(from, '44100,\n        MICARRAY_RAW_SAMPLE_RATE');
}
