#include "TnBridge.h"
#include <wdmsec.h>
#include "TnBridgeDiagnostics.h"

namespace {
PDEVICE_OBJECT control;
PDRIVER_DISPATCH previous[IRP_MJ_MAXIMUM_FUNCTION + 1];
bool initialized;
KSPIN_LOCK traceLock;
TnBridgeTrace trace{1,0,{}};
UNICODE_STRING link = RTL_CONSTANT_STRING(L"\\DosDevices\\ToneNariSysvadBridge");
const GUID controlClass{0xcfbdd818,0x91aa,0x45f1,{0x89,0x17,0x37,0x70,0x98,0xba,0xaf,0x93}};
NTSTATUS Dispatch(PDEVICE_OBJECT device, PIRP irp) {
    const auto stack = IoGetCurrentIrpStackLocation(irp);
    if (device != control) {
        if (stack->MajorFunction == IRP_MJ_PNP &&
            (stack->MinorFunction == IRP_MN_STOP_DEVICE ||
             stack->MinorFunction == IRP_MN_REMOVE_DEVICE ||
             stack->MinorFunction == IRP_MN_SURPRISE_REMOVAL)) TnTransportSetOnline(false);
        return previous[stack->MajorFunction](device, irp);
    }
    irp->IoStatus.Information = 0;
    NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
    if (stack->MajorFunction == IRP_MJ_CREATE || stack->MajorFunction == IRP_MJ_CLOSE)
        status = STATUS_SUCCESS;
    else if (stack->MajorFunction == IRP_MJ_CLEANUP) {
        TnTransportCleanup(stack->FileObject); status = STATUS_SUCCESS;
    } else if (stack->MajorFunction == IRP_MJ_DEVICE_CONTROL &&
        stack->Parameters.DeviceIoControl.IoControlCode == IOCTL_TN_BRIDGE_TRACE) {
        status=STATUS_BUFFER_TOO_SMALL;
        if (stack->Parameters.DeviceIoControl.OutputBufferLength >= sizeof(trace)) {
            KIRQL irql; KeAcquireSpinLock(&traceLock,&irql);
            RtlCopyMemory(irp->AssociatedIrp.SystemBuffer,&trace,sizeof(trace));
            KeReleaseSpinLock(&traceLock,irql);
            irp->IoStatus.Information=sizeof(trace); status=STATUS_SUCCESS;
        }
    } else if (stack->MajorFunction == IRP_MJ_DEVICE_CONTROL &&
        stack->Parameters.DeviceIoControl.IoControlCode == IOCTL_TONENARI_GET_TRANSPORT)
        status = TnTransportMap(irp, stack);
#ifdef TONENARI_SYSVAD_RATE_POLICY
    else if (stack->MajorFunction == IRP_MJ_DEVICE_CONTROL &&
        stack->Parameters.DeviceIoControl.IoControlCode == IOCTL_TONENARI_RATE_POLICY) {
        // Both pinned endpoint tables contain 44.1/48 kHz. Capture does not
        // contain 96 kHz; do not accept a policy impossible to negotiate.
        const auto request = static_cast<TONENARI_RATE_REQUEST*>(irp->AssociatedIrp.SystemBuffer);
        if (stack->Parameters.DeviceIoControl.InputBufferLength >= sizeof(*request) &&
            request && request->Operation == TONENARI_RATE_BEGIN &&
            request->SampleRate != 44100 && request->SampleRate != 48000)
            status = STATUS_NOT_SUPPORTED;
        else status = TnTransportRatePolicy(irp, stack);
    }
#endif
    irp->IoStatus.Status = status;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}
}

NTSTATUS TnBridgeInitialize(PDRIVER_OBJECT driver) {
    KeInitializeSpinLock(&traceLock);
    auto status = TnTransportInitialize();
    if (!NT_SUCCESS(status)) return status;
#ifdef TONENARI_SYSVAD_RATE_POLICY
    status = TnTransportEnableRatePolicy();
    if (!NT_SUCCESS(status)) { TnTransportShutdown(); return status; }
#endif
    initialized = true;
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\ToneNariSysvadBridge");
    // Diagnostic controller is restricted to administrators/SYSTEM. Production
    // per-session authorization is a separate gate; no unrestricted global map.
    UNICODE_STRING acl = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");
    status = IoCreateDeviceSecure(driver, 0, &name, FILE_DEVICE_SOUND,
        FILE_DEVICE_SECURE_OPEN, FALSE, &acl, &controlClass, &control);
    if (NT_SUCCESS(status)) status = IoCreateSymbolicLink(&link, &name);
    if (!NT_SUCCESS(status)) {
        if (control) { IoDeleteDevice(control); control = nullptr; }
        TnTransportShutdown(); initialized = false;
        return status;
    }
    // Preserve PortCls and the upstream PnP handler for all audio IRPs. This
    // dispatch wrapper owns only the distinct diagnostic control device.
    for (ULONG i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; ++i) {
        previous[i] = driver->MajorFunction[i]; driver->MajorFunction[i] = Dispatch;
    }
    control->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
}
void TnBridgeShutdown() {
    if (!initialized) return;
    TnTransportSetOnline(false);
    TnTransportShutdown();
    IoDeleteSymbolicLink(&link);
    if (control) { IoDeleteDevice(control); control = nullptr; }
    initialized = false;
}
static NTSTATUS ReserveNative(bool capture, const WAVEFORMATEX* wave, tonenari::PcmFormat* format) {
    bool integer = wave->wFormatTag == WAVE_FORMAT_PCM;
    bool floating = wave->wFormatTag == WAVE_FORMAT_IEEE_FLOAT;
    ULONG validBits = wave->wBitsPerSample;
    if (wave->wFormatTag == WAVE_FORMAT_EXTENSIBLE) {
        if (wave->cbSize < sizeof(WAVEFORMATEXTENSIBLE)-sizeof(WAVEFORMATEX)) return STATUS_NOT_SUPPORTED;
        const auto ext = reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(wave);
        integer = IsEqualGUID(ext->SubFormat, KSDATAFORMAT_SUBTYPE_PCM);
        floating = IsEqualGUID(ext->SubFormat, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
        validBits = ext->Samples.wValidBitsPerSample;
    }
    *format = {ULONG(wave->wBitsPerSample/8), integer ? validBits : 0u};
    if ((!integer && !floating) || (integer && !validBits) || !format->valid() || (floating && wave->wBitsPerSample != 32) ||
        wave->wBitsPerSample % 8 || !wave->nChannels || wave->nChannels > 64 ||
        wave->nBlockAlign != wave->nChannels*format->bytes ||
        !wave->nSamplesPerSec || wave->nSamplesPerSec > 384000 ||
        wave->nAvgBytesPerSec != wave->nSamplesPerSec*wave->nBlockAlign)
        return STATUS_NOT_SUPPORTED;
    // Upstream has already validated its advertised formats. Do not reject its
    // 8/16/24/32 kHz discovery streams merely because controller mapping currently
    // supports fewer rates. Map validates the negotiated rate when it attaches.
    return TnTransportReserveStream(capture, wave->nChannels, wave->nSamplesPerSec);
}
NTSTATUS TnBridgeReserve(bool capture, const WAVEFORMATEX* wave, tonenari::PcmFormat* format) {
    const auto status=ReserveNative(capture,wave,format);
    KIRQL irql; KeAcquireSpinLock(&traceLock,&irql);
    trace.attempts[trace.count % 256]={capture ? 1u : 0u, wave->nChannels,
        wave->nSamplesPerSec,wave->wBitsPerSample,format->validBits,ULONG(status)};
    ++trace.count;
    KeReleaseSpinLock(&traceLock,irql);
    return status;
}

#ifdef TONENARI_SYSVAD_RATE_POLICY
PKSDATAFORMAT TnBridgeDefaultFormat(PKSDATAFORMAT current,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count) {
    if (!current || current->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX)) return nullptr;
    const auto original = reinterpret_cast<PKSDATAFORMAT_WAVEFORMATEX>(current);
    const auto forced = TnTransportForcedRate();
    const auto rate = forced ? forced : TnTransportPreferredRate();
    for (ULONG i = 0; i < count; ++i) {
        const auto& wave = formats[i].WaveFormatExt.Format;
        if (wave.nSamplesPerSec == rate && wave.nChannels == original->WaveFormatEx.nChannels &&
            wave.wBitsPerSample == original->WaveFormatEx.wBitsPerSample &&
            IsEqualGUID(formats[i].DataFormat.SubFormat, current->SubFormat))
            return &formats[i].DataFormat;
    }
    return nullptr;
}

static bool RangeAccepts(PKSDATARANGE raw, const KSDATAFORMAT_WAVEFORMATEXTENSIBLE& format) {
    if (!raw || raw->FormatSize < sizeof(KSDATARANGE_AUDIO)) return false;
    if ((!IsEqualGUID(raw->MajorFormat, KSDATAFORMAT_TYPE_WILDCARD) &&
         !IsEqualGUID(raw->MajorFormat, format.DataFormat.MajorFormat)) ||
        (!IsEqualGUID(raw->SubFormat, KSDATAFORMAT_SUBTYPE_WILDCARD) &&
         !IsEqualGUID(raw->SubFormat, format.DataFormat.SubFormat)) ||
        (!IsEqualGUID(raw->Specifier, KSDATAFORMAT_SPECIFIER_WILDCARD) &&
         !IsEqualGUID(raw->Specifier, format.DataFormat.Specifier))) return false;
    const auto range = reinterpret_cast<PKSDATARANGE_AUDIO>(raw);
    const auto& wave = format.WaveFormatExt.Format;
    return wave.nChannels <= range->MaximumChannels &&
        wave.wBitsPerSample >= range->MinimumBitsPerSample && wave.wBitsPerSample <= range->MaximumBitsPerSample &&
        wave.nSamplesPerSec >= range->MinimumSampleFrequency && wave.nSamplesPerSec <= range->MaximumSampleFrequency;
}

NTSTATUS TnBridgeIntersect(PKSDATARANGE client, PKSDATARANGE mine,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count,
    ULONG size, PVOID output, PULONG needed) {
    if (!needed || !client || !mine) return STATUS_INVALID_PARAMETER;
    *needed = 0;
    const auto forced = TnTransportForcedRate();
    const auto preferred = forced ? forced : TnTransportPreferredRate();
    // Prefer the policy rate, then allow explicitly requested supported formats
    // only outside a transaction. Always intersect BOTH native and client ranges.
    for (ULONG pass = 0; pass < (forced ? 1u : 2u); ++pass) {
        for (ULONG i = 0; i < count; ++i) {
            const auto& candidate = formats[i];
            const auto rate = candidate.WaveFormatExt.Format.nSamplesPerSec;
            if ((rate != 44100 && rate != 48000) || (pass == 0 && rate != preferred) ||
                !RangeAccepts(client, candidate) || !RangeAccepts(mine, candidate)) continue;
            *needed = sizeof(candidate);
            if (!size) return STATUS_BUFFER_OVERFLOW;
            if (size < *needed) return STATUS_BUFFER_TOO_SMALL;
            if (!output) return STATUS_INVALID_PARAMETER;
            RtlCopyMemory(output, &candidate, sizeof(candidate));
            return STATUS_SUCCESS;
        }
    }
    return STATUS_NO_MATCH;
}
#endif
