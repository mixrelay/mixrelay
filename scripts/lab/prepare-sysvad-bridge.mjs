// Reproducible lab overlay, not an in-place modification of the reference checkout.
// Only pinned files are transformed; upstream license/notices remain in the copy.
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { validateProfile, profileSpeaker, profileCapture, profilePairs } from './sysvad-endpoint-profile.mjs';
import { ratePolicyHeader, ratePolicyMiniport, ratePolicySpeaker, ratePolicyCapture, ratePolicyCaptureFormats } from './sysvad-rate-policy.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const args = process.argv.slice(2);
const ratePolicy = args.includes('--rate-policy');
const positional = args.filter(arg => arg !== '--rate-policy');
const [referenceArg, destinationArg, profileArg] = positional;
if (!referenceArg || !destinationArg) throw Error('Usage: prepare-sysvad-bridge.mjs REFERENCE NEW_DESTINATION');
if (positional.length > 3 || args.length !== positional.length + Number(ratePolicy)) throw Error('Too many arguments');
const profile = profileArg ? validateProfile(JSON.parse(readFileSync(profileArg, 'utf8'))) : null;
const reference = resolve(referenceArg), destination = resolve(destinationArg);
if (existsSync(destination)) throw Error('Destination exists; evidence must not be overwritten');
const pins = {
  'adapter.cpp': 'b94bd1c8e30029825e29124578d8825112e14bbddfd22621e62bea6ddb3940de',
  'EndpointsCommon/minwavertstream.cpp': '6cf955ca772134994ca3cc1539f77e8a290097af58ff6ed518ea65b7e256e5a1',
  'EndpointsCommon/minwavertstream.h': '280c35485add5d3b01843db3b6809d7b89a8918fcf3c7b4404ef0e17d13b9ecd',
  'TabletAudioSample/TabletAudioSample.vcxproj': 'a0bafd517a110772035ef0c29284d7bf0eb5c67a21db68fbc45f4defbe937fc4',
  'TabletAudioSample/ComponentizedAudioSample.inx': '08bd75a1fe504d3314be035e0a9826a3fa1e3c41f5839a2485632e13958637b7',
  'EndpointsCommon/speakerwavtable.h': '92e52516fb07c36e0562b0ea749e80d467b47342aab6446cd7834edf2bd6697d',
  'EndpointsCommon/micarraywavtable.h': 'cfc239cb95839bc7dc2cf8d1795d0f5e98da6eb2584ace6e06082edae483dbd5',
  'TabletAudioSample/minipairs.h': '8ff48ebf7f3bbd47d869d5b04385cd38e9c6c5407b778fd459b076b5ced61e9a',
};
if (ratePolicy) Object.assign(pins, {
  'EndpointsCommon/minwavert.cpp': '66d1a0fe09bae192c87e0db04dc1c0983049676b50184e93e787e1987c88f587',
  'EndpointsCommon/minwavert.h': '4da9d715852fe6308c33ca442d176877490b0ab0d9f185fd5fe774b346976ba6',
});
for (const [name, hash] of Object.entries(pins)) {
  if (createHash('sha256').update(readFileSync(join(reference,'audio/sysvad',name))).digest('hex') !== hash)
    throw Error(`Upstream changed: ${name}`);
}
mkdirSync(destination, {recursive:true});
cpSync(join(reference,'audio/sysvad'), join(destination,'audio/sysvad'), {recursive:true, filter:p=>!p.endsWith('.DS_Store')});
cpSync(join(reference,'wil'), join(destination,'wil'), {recursive:true, filter:p=>!p.endsWith('.git')});
cpSync(join(reference,'LICENSE'), join(destination,'LICENSE'));
cpSync(join(repo,'scripts/lab/sysvad-build.props'), join(destination,'Directory.Build.props'));
const base = join(destination,'audio/sysvad');
cpSync(join(repo,'native/windows/virtual-audio-driver/include'), join(base,'include'), {recursive:true});
cpSync(join(repo,'native/windows/virtual-audio-driver/src/TnTransport.cpp'), join(base,'TnTransport.cpp'));
let transportHeader = readFileSync(join(repo,'native/windows/virtual-audio-driver/src/TnTransport.h'),'utf8');
writeFileSync(join(base,'TnTransport.h'),transportHeader.replaceAll('../include/','include/'));
for (const file of ['TnBridge.cpp','TnBridge.h','TnBridgeDiagnostics.h']) cpSync(join(repo,'native/windows/sysvad-bridge',file),join(base,file));
// This relative include differs only because TnTransport.cpp now sits at sysvad/.
const transport = readFileSync(join(base,'TnTransport.cpp'),'utf8');
writeFileSync(join(base,'TnTransport.cpp'),transport
  .replace('"ToneNariRing.h"','"include/ToneNariRing.h"')
  .replace('"../include/ToneNariRatePolicy.h"','"include/ToneNariRatePolicy.h"'));
function once(text, from, to) {
  if (text.split(from).length !== 2) throw Error(`Overlay anchor not unique: ${from}`);
  return text.replace(from,to);
}
function edit(file, fn, encoding='utf8') {
  const path=join(base,file); const text=readFileSync(path,encoding).replaceAll('\r\n','\n');
  writeFileSync(path,fn(text).replaceAll('\n','\r\n'),encoding);
}
edit('adapter.cpp', s=>{
  // Keep PUT_GUIDS_HERE before PortCls headers: moving this include to the top
  // suppresses upstream GUID definitions and breaks native linking.
  s = once(s,'#include "minipairs.h"','#include "minipairs.h"\n#include "TnBridge.h"');
  s = once(s,'    ReleaseRegistryStringBuffer();\n\n    if (DriverObject == NULL)',
    '    TnBridgeShutdown();\n    ReleaseRegistryStringBuffer();\n\n    if (DriverObject == NULL)');
  s = once(s,'    DriverObject->DriverUnload = DriverUnload;',
    '    DriverObject->DriverUnload = DriverUnload;\n    ntStatus = TnBridgeInitialize(DriverObject);\n    if (!NT_SUCCESS(ntStatus)) goto Done;');
  s = once(s,'    return ntStatus;\n} // StartDevice','    if (NT_SUCCESS(ntStatus)) TnTransportSetOnline(true);\n    return ntStatus;\n} // StartDevice');
  return s;
});
edit('EndpointsCommon/minwavertstream.h',s=>once(s,'protected:\n    PPORTWAVERTSTREAM',
  'protected:\n    bool m_tnBridge = false;\n    volatile LONG m_tnProtected = 0;\n    tonenari::PcmFormat m_tnFormat{};\n    PPORTWAVERTSTREAM'));
// The header needs the format declaration independently of include order.
edit('EndpointsCommon/minwavertstream.h',s=>'#include "TnBridge.h"\n'+s);
edit('EndpointsCommon/minwavertstream.cpp',s=>{
  s = once(s,'    KeFlushQueuedDpcs();\n\n#ifdef SYSVAD_BTH_BYPASS',
    '    KeFlushQueuedDpcs();\n    if (m_tnBridge) { TnTransportReleaseStream(m_bCapture); m_tnBridge = false; }\n\n#ifdef SYSVAD_BTH_BYPASS');
  s = once(s,'    RtlCopyMemory(m_pWfExt, pWfEx, sizeof(WAVEFORMATEX) + pWfEx->cbSize);',
    `    RtlCopyMemory(m_pWfExt, pWfEx, sizeof(WAVEFORMATEX) + pWfEx->cbSize);
    // Route only the system pins of the named diagnostic pair. Offload, loopback
    // and keyword pins retain their upstream behavior and cannot share this ring.
    if ((m_pMiniport->m_DeviceType == eSpeakerDevice && m_pMiniport->IsSystemRenderPin(Pin_)) ||
        (m_pMiniport->m_DeviceType == eMicArrayDevice1 && m_pMiniport->IsSystemCapturePin(Pin_))) {
        ntStatus = TnBridgeReserve(m_bCapture, pWfEx, &m_tnFormat);
        if (!NT_SUCCESS(ntStatus)) return ntStatus;
        m_tnBridge = true;
    }`);
  s = s.replaceAll('!g_DoNotCreateDataFiles','(!m_tnBridge && !g_DoNotCreateDataFiles)');
  s = once(s,'    if (m_bCapture)\n    {\n        ReadRegistrySettings();',
    '    if (m_bCapture && !m_tnBridge)\n    {\n        ReadRegistrySettings();');
  s = once(s,'        if ((!m_tnBridge && !g_DoNotCreateDataFiles))\n        {\n            // Read from buffer',
    '        if (m_tnBridge || !g_DoNotCreateDataFiles)\n        {\n            // Read from buffer');
  s = once(s,'    // Increment presentation position even after last buffer is rendered.',
    `    // Preserve fractional frame bytes across 44.1 kHz updates; never split a
    // sample/container at this transport boundary.
    if (m_tnBridge) {
        const ULONG remainder = ByteDisplacement % m_pWfExt->Format.nBlockAlign;
        ByteDisplacement -= remainder;
        m_byteDisplacementCarryForward += remainder * 1000;
    }
    // Increment presentation position even after last buffer is rendered.`);
  s = once(s,'            m_ToneGenerator.GenerateSine(m_pDmaBuffer + bufferOffset, runWrite);',
    `        if (m_tnBridge) TnTransportPcmNative(true, m_pDmaBuffer + bufferOffset,
            runWrite / m_pWfExt->Format.nBlockAlign, m_pWfExt->Format.nChannels,
            m_pWfExt->Format.nSamplesPerSec, m_tnFormat);
        else m_ToneGenerator.GenerateSine(m_pDmaBuffer + bufferOffset, runWrite);`);
  s = once(s,'        m_SaveData.WriteData(m_pDmaBuffer + bufferOffset, runWrite);',
    `        if (m_tnBridge && !InterlockedCompareExchange(&m_tnProtected, 0, 0)) TnTransportPcmNative(false, m_pDmaBuffer + bufferOffset,
            runWrite / m_pWfExt->Format.nBlockAlign, m_pWfExt->Format.nChannels,
            m_pWfExt->Format.nSamplesPerSec, m_tnFormat);
        else if (!m_tnBridge) m_SaveData.WriteData(m_pDmaBuffer + bufferOffset, runWrite);`);
  s = once(s,'    DPF_ENTER(("[CMiniportWaveRT::SetContentId]"));',
    `    DPF_ENTER(("[CMiniportWaveRT::SetContentId]"));
    // Fail closed: a shared diagnostic map is not a protected audio path.
    if (m_tnBridge) {
        InterlockedExchange(&m_tnProtected, drmRights->CopyProtect ? 1 : 0);
        if (drmRights->CopyProtect) return STATUS_NOT_SUPPORTED;
    }`);
  return s;
});
edit('TabletAudioSample/TabletAudioSample.vcxproj',s=>{
  s = once(s,'    <ClCompile Include="..\\adapter.cpp" />',
    '    <ClCompile Include="..\\adapter.cpp" />\n    <ClCompile Include="..\\TnBridge.cpp" />\n    <ClCompile Include="..\\TnTransport.cpp" />');
  // The owner-exit callback requires FORCE_INTEGRITY; test signing alone is not
  // sufficient for PsSetCreateProcessNotifyRoutineEx registration.
  return once(s,'</Project>',`  <ItemDefinitionGroup><Link><AdditionalDependencies>wdmsec.lib;%(AdditionalDependencies)</AdditionalDependencies><AdditionalOptions>/INTEGRITYCHECK %(AdditionalOptions)</AdditionalOptions></Link></ItemDefinitionGroup>\n</Project>`);
});
edit('TabletAudioSample/ComponentizedAudioSample.inx',s=>s
  .replaceAll('sysvad_ComponentizedAudioSample','tonenari_SysvadBridge')
  .replaceAll('sysvad_componentizedaudiosample','tonenari_sysvadbridge')
  .replaceAll('Virtual Audio Device (WDM) - Tablet Sample','ToneNari SysVAD Bridge LAB'), 'utf16le');
// One stream per direction is the v2 transport's real capacity. PortCls must
// report that capacity too, rather than promising the sample's 6/4 streams.
edit('EndpointsCommon/speakerwavtable.h',s=>once(s,
  '#define SPEAKER_MAX_INPUT_SYSTEM_STREAMS            6',
  '#define SPEAKER_MAX_INPUT_SYSTEM_STREAMS            1'));
edit('EndpointsCommon/micarraywavtable.h',s=>once(s,
  '#define MICARRAY_MAX_INPUT_STREAMS              4',
  '#define MICARRAY_MAX_INPUT_STREAMS              1'));
if (profile) {
  edit('EndpointsCommon/speakerwavtable.h', s => profileSpeaker(s, profile.renderChannels));
  edit('EndpointsCommon/micarraywavtable.h', s => profileCapture(s, profile.captureChannels));
  edit('TabletAudioSample/minipairs.h', profilePairs);
  // A distinct build artifact; never change or install over the accepted LAB pair
  // merely because a profile compiles. PnP/format/PCM qualification comes next.
  writeFileSync(join(destination, 'TONENARI-ENDPOINT-PROFILE.json'), JSON.stringify(profile, null, 2) + '\n');
}
if (ratePolicy) {
  // Distinct artifact opt-in; the accepted LAB source/install remains unchanged.
  edit('TnBridge.h', s => '#define TONENARI_SYSVAD_RATE_POLICY 1\n' + s);
  edit('EndpointsCommon/minwavert.h', ratePolicyHeader);
  edit('EndpointsCommon/minwavert.cpp', ratePolicyMiniport);
  edit('EndpointsCommon/speakerwavtable.h', ratePolicySpeaker);
  edit('EndpointsCommon/micarraywavtable.h', ratePolicyCapture);
  edit('EndpointsCommon/micarraywavtable.h', ratePolicyCaptureFormats);
  cpSync(join(repo, 'native/windows/virtual-audio-driver/tests'), join(base, 'ToneNariTests'), { recursive: true });
  const probe = readFileSync(join(repo, 'native/windows/sysvad-bridge/rate-policy-probe.cpp'), 'utf8');
  writeFileSync(join(base, 'rate-policy-probe.cpp'), probe.replace('../virtual-audio-driver/include/', 'include/'));
  writeFileSync(join(destination, 'TONENARI-RATE-POLICY.json'), JSON.stringify({
    version: 1, pcmTransportVersion: 2, rates: [44100, 48000],
    nativeReadbackRequired: true, qualified: false,
  }, null, 2) + '\n');
}
writeFileSync(join(destination,'TONENARI-LAB-OVERLAY.md'),
  'SysVAD diagnostic bridge, NOT a distribution package. Upstream MS-PL retained.\n'+
  (profile ? 'Profile selects only Speakers/system render and MicArray1/system capture; offload, loopback and keyword instances are disabled. Not runtime configuration or a shipping INF.\n' :
  'Only Speakers/system render and MicArray1/system capture are bridged; other endpoints remain sample behavior.\n')+
  'Reproduce with scripts/lab/prepare-sysvad-bridge.mjs. Source provenance: infra/lab/sysvad-baseline.json.\n');
console.log(destination);
