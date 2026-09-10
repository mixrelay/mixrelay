// Product-owned, non-offloaded WaveRT render/capture pair. PortCls owns the
// system-facing KS machinery; this file owns stream memory, clock and teardown.
#define INITGUID
#include <portcls.h>
#include <stdunk.h>
#include <ksmedia.h>
#include <wdmsec.h>
#include "MrTransport.h"
#include "../include/MixRelayRatePolicy.h"

namespace {
constexpr ULONG Tag = 'wINT';
constexpr ULONG MaxBufferBytes = 1024 * 1024;
NTSTATUS Format(PKSDATAFORMAT data, ULONG* channels, ULONG* rate) {
    if (!data || data->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX) ||
        !IsEqualGUIDAligned(data->MajorFormat, KSDATAFORMAT_TYPE_AUDIO) ||
        !IsEqualGUIDAligned(data->SubFormat, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) ||
        !IsEqualGUIDAligned(data->Specifier, KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)) return STATUS_NO_MATCH;
    auto wf = &reinterpret_cast<PKSDATAFORMAT_WAVEFORMATEX>(data)->WaveFormatEx;
    if (wf->wFormatTag == WAVE_FORMAT_EXTENSIBLE) {
        if (data->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEXTENSIBLE) || wf->cbSize < 22) return STATUS_NO_MATCH;
        auto ext = reinterpret_cast<WAVEFORMATEXTENSIBLE*>(wf);
        if (ext->Samples.wValidBitsPerSample != 32 || !IsEqualGUIDAligned(ext->SubFormat, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)) return STATUS_NO_MATCH;
    } else if (wf->wFormatTag != WAVE_FORMAT_IEEE_FLOAT) return STATUS_NO_MATCH;
    if (!wf->nChannels || wf->nChannels > 64 || wf->wBitsPerSample != 32 ||
        wf->nBlockAlign != wf->nChannels * 4 || wf->nAvgBytesPerSec != wf->nSamplesPerSec * wf->nBlockAlign ||
        (wf->nSamplesPerSec != 44100 && wf->nSamplesPerSec != 48000 && wf->nSamplesPerSec != 96000)) return STATUS_NO_MATCH;
    *channels = wf->nChannels; *rate = wf->nSamplesPerSec; return STATUS_SUCCESS;
}

class Stream final : public IMiniportWaveRTStreamNotification, public CUnknown {
    PPORTWAVERTSTREAM port;
    const bool capture;
    const ULONG channels, rate;
    PMDL mdl{}; UCHAR* buffer{}; ULONG bytes{}, packetFrames{};
    KSPIN_LOCK lock; KTIMER timer; KDPC dpc;
    PKEVENT events[16]{};
    KSSTATE state{KSSTATE_STOP};
    ULONGLONG position{}, base{}, start{}, frequency{};
    void Update() {
        if (state != KSSTATE_RUN || !buffer) return;
        const auto now = ULONGLONG(KeQueryPerformanceCounter(nullptr).QuadPart);
        const auto elapsed = now >= start ? now - start : 0;
        const auto goal = base + (elapsed / frequency) * rate + ((elapsed % frequency) * rate) / frequency;
        if (goal <= position) return;
        const ULONG capacity = bytes / (channels * 4);
        const auto previous = position;
        // A delayed DPC handles at most one DMA-buffer worth, never an unbounded
        // time backlog. Position still reports the actual sample clock.
        ULONGLONG firstFrame = max(position, goal > capacity ? goal - capacity : 0);
        ULONG remaining = ULONG(goal - firstFrame);
        while (remaining) {
            const ULONG offset = ULONG(firstFrame % capacity);
            const ULONG count = min(remaining, capacity - offset);
            MrTransportPcm(capture, buffer + SIZE_T(offset) * channels * 4, count, channels, rate);
            firstFrame += count; remaining -= count;
        }
        position = goal;
        if (packetFrames && position / packetFrames != previous / packetFrames)
            for (auto event : events) if (event) KeSetEvent(event, IO_NO_INCREMENT, FALSE);
    }
    static void Tick(KDPC*, void* context, void*, void*) {
        auto self = static_cast<Stream*>(context);
        KeAcquireSpinLockAtDpcLevel(&self->lock); self->Update(); KeReleaseSpinLockFromDpcLevel(&self->lock);
    }
    void StopTimer() {
        KeCancelTimer(&timer); KeRemoveQueueDpc(&dpc);
        // Lifecycle only, at PASSIVE_LEVEL. Never free stream/event/DMA storage
        // while a queued or currently executing callback can still reference it.
        KeFlushQueuedDpcs();
    }
public:
    Stream(PPORTWAVERTSTREAM p, bool c, ULONG ch, ULONG r) : CUnknown(nullptr), port(p), capture(c), channels(ch), rate(r) {
        port->AddRef(); KeInitializeSpinLock(&lock); KeInitializeTimer(&timer); KeInitializeDpc(&dpc, Tick, this);
        LARGE_INTEGER f; KeQueryPerformanceCounter(&f); frequency = ULONGLONG(f.QuadPart);
    }
    ~Stream() override {
        StopTimer();
        if (buffer) port->UnmapAllocatedPages(buffer, mdl);
        if (mdl) port->FreePagesFromMdl(mdl);
        port->Release(); MrTransportReleaseStream(capture);
    }
    NTSTATUS QueryInterface(REFIID id, void** out) override { return GetOuterUnknown()->QueryInterface(id, out); }
    ULONG AddRef() override { return GetOuterUnknown()->AddRef(); }
    ULONG Release() override { return GetOuterUnknown()->Release(); }
    NTSTATUS NonDelegatingQueryInterface(REFIID id, void** out) override {
        if (!out) return STATUS_INVALID_PARAMETER;
        *out = nullptr;
        if (IsEqualGUIDAligned(id, IID_IUnknown) || IsEqualGUIDAligned(id, IID_IMiniportWaveRTStream))
            *out = static_cast<IMiniportWaveRTStream*>(this);
        else if (IsEqualGUIDAligned(id, IID_IMiniportWaveRTStreamNotification))
            *out = static_cast<IMiniportWaveRTStreamNotification*>(this);
        else return STATUS_NOINTERFACE;
        AddRef(); return STATUS_SUCCESS;
    }
    NTSTATUS SetFormat(PKSDATAFORMAT data) override {
        ULONG ch{}, r{}; auto status = Format(data, &ch, &r);
        return NT_SUCCESS(status) && ch == channels && r == rate ? STATUS_SUCCESS : STATUS_NO_MATCH;
    }
    NTSTATUS SetState(KSSTATE next) override {
        if (next < KSSTATE_STOP || next > KSSTATE_RUN) return STATUS_INVALID_PARAMETER;
        KIRQL irql; KeAcquireSpinLock(&lock, &irql);
        Update();
        if (next == KSSTATE_RUN && !buffer) { KeReleaseSpinLock(&lock, irql); return STATUS_INVALID_DEVICE_STATE; }
        if (next == KSSTATE_RUN && state != KSSTATE_RUN) {
            base = position; start = ULONGLONG(KeQueryPerformanceCounter(nullptr).QuadPart);
        }
        state = next;
        if (next == KSSTATE_STOP) position = base = 0;
        KeReleaseSpinLock(&lock, irql);
        if (next == KSSTATE_RUN) { LARGE_INTEGER due; due.QuadPart = -10000; KeSetTimerEx(&timer, due, 1, &dpc); }
        else StopTimer();
        return STATUS_SUCCESS;
    }
    NTSTATUS GetPosition(PKSAUDIO_POSITION result) override {
        KIRQL irql; KeAcquireSpinLock(&lock, &irql); Update();
        result->PlayOffset = result->WriteOffset = bytes ? (position * channels * 4) % bytes : 0;
        KeReleaseSpinLock(&lock, irql); return STATUS_SUCCESS;
    }
    NTSTATUS AllocateAudioBuffer(ULONG requested, PMDL* result, ULONG* actual, ULONG* offset, MEMORY_CACHING_TYPE* caching) override {
        return AllocateBufferWithNotification(2, requested, result, actual, offset, caching);
    }
    NTSTATUS AllocateBufferWithNotification(ULONG notifications, ULONG requested, PMDL* result, ULONG* actual, ULONG* offset, MEMORY_CACHING_TYPE* caching) override {
        *result = nullptr; *actual = *offset = 0; *caching = MmCached;
        if (mdl || !notifications || notifications > 64 || requested > MaxBufferBytes) return STATUS_INVALID_PARAMETER;
        const ULONG alignment = channels * 4 * notifications;
        const ULONG length = requested / alignment * alignment;
        if (!length) return STATUS_INVALID_PARAMETER;
        PHYSICAL_ADDRESS high; high.QuadPart = MAXLONGLONG;
        mdl = port->AllocatePagesForMdl(high, length);
        if (!mdl) return STATUS_INSUFFICIENT_RESOURCES;
        buffer = static_cast<UCHAR*>(port->MapAllocatedPages(mdl, MmCached));
        if (!buffer) { port->FreePagesFromMdl(mdl); mdl = nullptr; return STATUS_INSUFFICIENT_RESOURCES; }
        RtlZeroMemory(buffer, length); bytes = length; packetFrames = length / (channels * 4 * notifications);
        *result = mdl; *actual = length; return STATUS_SUCCESS;
    }
    void FreeAudioBuffer(PMDL supplied, ULONG) override {
        SetState(KSSTATE_STOP);
        if (supplied != mdl) return;
        if (buffer) port->UnmapAllocatedPages(buffer, mdl);
        if (mdl) port->FreePagesFromMdl(mdl);
        buffer = nullptr; mdl = nullptr; bytes = packetFrames = 0;
    }
    void FreeBufferWithNotification(PMDL supplied, ULONG size) override { FreeAudioBuffer(supplied, size); }
    void GetHWLatency(KSRTAUDIO_HWLATENCY* result) override { RtlZeroMemory(result, sizeof(*result)); }
    NTSTATUS GetPositionRegister(KSRTAUDIO_HWREGISTER*) override { return STATUS_NOT_SUPPORTED; }
    NTSTATUS GetClockRegister(KSRTAUDIO_HWREGISTER*) override { return STATUS_NOT_SUPPORTED; }
    NTSTATUS RegisterNotificationEvent(PKEVENT event) override {
        if (!event) return STATUS_INVALID_PARAMETER;
        KIRQL irql; KeAcquireSpinLock(&lock, &irql);
        NTSTATUS status = STATUS_INSUFFICIENT_RESOURCES;
        for (auto entry : events) if (entry == event) { KeReleaseSpinLock(&lock, irql); return STATUS_OBJECT_NAME_COLLISION; }
        for (auto& entry : events) if (!entry) { entry = event; status = STATUS_SUCCESS; break; }
        KeReleaseSpinLock(&lock, irql); return status;
    }
    NTSTATUS UnregisterNotificationEvent(PKEVENT event) override {
        if (!event) return STATUS_INVALID_PARAMETER;
        KIRQL irql; KeAcquireSpinLock(&lock, &irql); NTSTATUS status = STATUS_NOT_FOUND;
        for (auto& entry : events) if (entry == event) { entry = nullptr; status = STATUS_SUCCESS; }
        KeReleaseSpinLock(&lock, irql); return status;
    }
};

class Miniport final : public IMiniportWaveRT, public IMiniportTopology, public CUnknown {
    const bool capture, topology;
    KSDATARANGE_AUDIO audio{};
    KSDATARANGE bridge{};
    PKSDATARANGE hostRanges[1]{}, bridgeRanges[1]{};
    PCPIN_DESCRIPTOR pins[2]{};
    PCCONNECTION_DESCRIPTOR connection{};
    PCFILTER_DESCRIPTOR filter{};
    GUID categories[3]{};
public:
    Miniport(bool c, bool t) : CUnknown(nullptr), capture(c), topology(t) {
        audio.DataRange.FormatSize = sizeof(audio); audio.DataRange.MajorFormat = KSDATAFORMAT_TYPE_AUDIO;
        audio.DataRange.SubFormat = KSDATAFORMAT_SUBTYPE_IEEE_FLOAT; audio.DataRange.Specifier = KSDATAFORMAT_SPECIFIER_WAVEFORMATEX;
        audio.MaximumChannels = 64; audio.MinimumBitsPerSample = audio.MaximumBitsPerSample = 32;
        audio.MinimumSampleFrequency = 44100; audio.MaximumSampleFrequency = 96000;
        bridge.FormatSize = sizeof(bridge); bridge.MajorFormat = KSDATAFORMAT_TYPE_AUDIO;
        bridge.SubFormat = KSDATAFORMAT_SUBTYPE_ANALOG; bridge.Specifier = KSDATAFORMAT_SPECIFIER_NONE;
        hostRanges[0] = &audio.DataRange; bridgeRanges[0] = &bridge;
        for (ULONG i = 0; i < 2; ++i) {
            auto& pin = pins[i];
            pin.MaxGlobalInstanceCount = !t && i == 0 ? 1 : 0;
            pin.MaxFilterInstanceCount = pin.MaxGlobalInstanceCount;
            pin.KsPinDescriptor.DataRangesCount = 1;
            pin.KsPinDescriptor.DataRanges = !t && i == 0 ? hostRanges : bridgeRanges;
            const bool incoming = c ? i == 1 : i == 0;
            pin.KsPinDescriptor.DataFlow = incoming ? KSPIN_DATAFLOW_IN : KSPIN_DATAFLOW_OUT;
            pin.KsPinDescriptor.Communication = !t && i == 0 ? KSPIN_COMMUNICATION_SINK : KSPIN_COMMUNICATION_NONE;
            pin.KsPinDescriptor.Category = t && i == 1 ? (c ? &KSNODETYPE_MICROPHONE : &KSNODETYPE_SPEAKER) : &KSCATEGORY_AUDIO;
        }
        connection = {PCFILTER_NODE, c ? 1u : 0u, PCFILTER_NODE, c ? 0u : 1u};
        filter.PinSize = sizeof(PCPIN_DESCRIPTOR); filter.PinCount = 2; filter.Pins = pins;
        filter.NodeSize = sizeof(PCNODE_DESCRIPTOR);
        filter.ConnectionCount = 1; filter.Connections = &connection;
        // Publish explicit KS categories, matching the INF interface classes.
        // EndpointBuilder discovers the device through KSCATEGORY_AUDIO, not
        // merely through a successful PnP start or our private control device.
        categories[0] = KSCATEGORY_AUDIO;
        categories[1] = t ? KSCATEGORY_TOPOLOGY : (c ? KSCATEGORY_CAPTURE : KSCATEGORY_RENDER);
        categories[2] = KSCATEGORY_REALTIME;
        filter.CategoryCount = t ? 2 : 3; filter.Categories = categories;
    }
    NTSTATUS QueryInterface(REFIID id, void** out) override { return GetOuterUnknown()->QueryInterface(id, out); }
    ULONG AddRef() override { return GetOuterUnknown()->AddRef(); }
    ULONG Release() override { return GetOuterUnknown()->Release(); }
    NTSTATUS NonDelegatingQueryInterface(REFIID id, void** out) override {
        if (!out) return STATUS_INVALID_PARAMETER; *out = nullptr;
        if (IsEqualGUIDAligned(id, IID_IUnknown) || IsEqualGUIDAligned(id, IID_IMiniport))
            *out = topology ? static_cast<IMiniport*>(static_cast<IMiniportTopology*>(this)) : static_cast<IMiniport*>(static_cast<IMiniportWaveRT*>(this));
        else if (!topology && IsEqualGUIDAligned(id, IID_IMiniportWaveRT)) *out = static_cast<IMiniportWaveRT*>(this);
        else if (topology && IsEqualGUIDAligned(id, IID_IMiniportTopology)) *out = static_cast<IMiniportTopology*>(this);
        else return STATUS_NOINTERFACE;
        AddRef(); return STATUS_SUCCESS;
    }
    NTSTATUS GetDescription(PPCFILTER_DESCRIPTOR* out) override { *out = &filter; return STATUS_SUCCESS; }
    NTSTATUS DataRangeIntersection(ULONG pin, PKSDATARANGE client, PKSDATARANGE, ULONG size, void* out, ULONG* needed) override {
        if (!needed || !client) return STATUS_INVALID_PARAMETER;
        *needed = sizeof(KSDATAFORMAT_WAVEFORMATEXTENSIBLE);
        if (topology || pin != 0 || client->FormatSize < sizeof(KSDATARANGE_AUDIO)) return STATUS_NO_MATCH;
        if ((!IsEqualGUIDAligned(client->MajorFormat, KSDATAFORMAT_TYPE_AUDIO) &&
             !IsEqualGUIDAligned(client->MajorFormat, KSDATAFORMAT_TYPE_WILDCARD)) ||
            (!IsEqualGUIDAligned(client->SubFormat, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) &&
             !IsEqualGUIDAligned(client->SubFormat, KSDATAFORMAT_SUBTYPE_WILDCARD)) ||
            (!IsEqualGUIDAligned(client->Specifier, KSDATAFORMAT_SPECIFIER_WAVEFORMATEX) &&
             !IsEqualGUIDAligned(client->Specifier, KSDATAFORMAT_SPECIFIER_WILDCARD))) return STATUS_NO_MATCH;
        auto range = reinterpret_cast<PKSDATARANGE_AUDIO>(client);
        const auto rate = mixrelay::IntersectRate(range->MinimumSampleFrequency, range->MaximumSampleFrequency,
            48000);
        if (!range->MaximumChannels || range->MinimumBitsPerSample > 32 || range->MaximumBitsPerSample < 32 || !rate) return STATUS_NO_MATCH;
        if (!size) return STATUS_BUFFER_OVERFLOW;
        if (size < *needed) return STATUS_BUFFER_TOO_SMALL;
        if (!out) return STATUS_INVALID_PARAMETER;
        KSDATAFORMAT_WAVEFORMATEXTENSIBLE result{};
        result.DataFormat = audio.DataRange; result.DataFormat.FormatSize = sizeof(result);
        auto& wf = result.WaveFormatExt.Format;
        wf.wFormatTag = WAVE_FORMAT_EXTENSIBLE; wf.nChannels = USHORT(min(range->MaximumChannels, 2ul));
        wf.nSamplesPerSec = rate; wf.wBitsPerSample = 32; wf.nBlockAlign = wf.nChannels * 4;
        wf.nAvgBytesPerSec = wf.nSamplesPerSec * wf.nBlockAlign; wf.cbSize = 22;
        result.WaveFormatExt.Samples.wValidBitsPerSample = 32;
        result.WaveFormatExt.dwChannelMask = wf.nChannels == 1 ? KSAUDIO_SPEAKER_MONO : KSAUDIO_SPEAKER_STEREO;
        result.WaveFormatExt.SubFormat = KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
        RtlCopyMemory(out, &result, sizeof(result)); return STATUS_SUCCESS;
    }
    NTSTATUS Init(PUNKNOWN, PRESOURCELIST, PPORTWAVERT) override { return STATUS_SUCCESS; }
    NTSTATUS Init(PUNKNOWN, PRESOURCELIST, PPORTTOPOLOGY) override { return STATUS_SUCCESS; }
    NTSTATUS NewStream(PMINIPORTWAVERTSTREAM* out, PPORTWAVERTSTREAM port, ULONG pin, BOOLEAN c, PKSDATAFORMAT data) override {
        *out = nullptr;
        if (topology || pin != 0 || bool(c) != capture) return STATUS_INVALID_PARAMETER;
        ULONG channels{}, rate{}; auto status = Format(data, &channels, &rate);
        if (!NT_SUCCESS(status)) return status;
        status = MrTransportReserveStream(capture, channels, rate);
        if (!NT_SUCCESS(status)) return status;
        auto stream = new (NonPagedPoolNx, Tag) Stream(port, capture, channels, rate);
        if (!stream) { MrTransportReleaseStream(capture); return STATUS_INSUFFICIENT_RESOURCES; }
        stream->NonDelegatingAddRef(); *out = stream; return STATUS_SUCCESS;
    }
    NTSTATUS GetDeviceDescription(PDEVICE_DESCRIPTION out) override {
        RtlZeroMemory(out, sizeof(*out)); out->Version = DEVICE_DESCRIPTION_VERSION;
        out->InterfaceType = Internal; out->ScatterGather = TRUE; out->Dma64BitAddresses = TRUE;
        out->MaximumLength = MaxBufferBytes; return STATUS_SUCCESS;
    }
};

PDEVICE_OBJECT controlDevice;
PDRIVER_DISPATCH original[IRP_MJ_MAXIMUM_FUNCTION + 1];
PDRIVER_UNLOAD originalUnload;
volatile LONG attached;
UNICODE_STRING symbolic = RTL_CONSTANT_STRING(L"\\DosDevices\\MixRelayAudio");
// Product identity, not a SysVAD sample/Contoso identity.
const GUID controlClass{0x67799e74,0x73be,0x481d,{0x89,0xb8,0xb7,0x09,0x74,0x7f,0x55,0x1c}};
// PcRegisterSubdevice retains Name for the entire device lifetime. Stack-local
// arrays in Start become dangling before PortCls publishes the KS interfaces.
WCHAR subdeviceNames[4][24]{L"WaveRender", L"TopologyRender", L"WaveCapture", L"TopologyCapture"};
NTSTATUS Dispatch(PDEVICE_OBJECT device, PIRP irp) {
    auto s = IoGetCurrentIrpStackLocation(irp);
    if (device != controlDevice) {
        if (s->MajorFunction == IRP_MJ_PNP && (s->MinorFunction == IRP_MN_STOP_DEVICE ||
            s->MinorFunction == IRP_MN_SURPRISE_REMOVAL || s->MinorFunction == IRP_MN_REMOVE_DEVICE)) {
            MrTransportSetOnline(false);
            if (s->MinorFunction == IRP_MN_REMOVE_DEVICE) InterlockedExchange(&attached, 0);
        }
        return original[s->MajorFunction](device, irp);
    }
    irp->IoStatus.Information = 0; NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
    if (s->MajorFunction == IRP_MJ_CREATE || s->MajorFunction == IRP_MJ_CLOSE) status = STATUS_SUCCESS;
    else if (s->MajorFunction == IRP_MJ_CLEANUP) { MrTransportCleanup(s->FileObject); status = STATUS_SUCCESS; }
    else if (s->MajorFunction == IRP_MJ_DEVICE_CONTROL && s->Parameters.DeviceIoControl.IoControlCode == IOCTL_MIXRELAY_GET_TRANSPORT)
        status = MrTransportMap(irp, s);
    irp->IoStatus.Status = status; IoCompleteRequest(irp, IO_NO_INCREMENT); return status;
}
NTSTATUS Install(PDEVICE_OBJECT device, PIRP irp, PRESOURCELIST resources, bool capture, bool topology, PWSTR name, PPORT* result) {
    *result = nullptr;
    auto mini = new (NonPagedPoolNx, Tag) Miniport(capture, topology);
    if (!mini) return STATUS_INSUFFICIENT_RESOURCES;
    mini->NonDelegatingAddRef(); PPORT port{};
    auto status = PcNewPort(&port, topology ? CLSID_PortTopology : CLSID_PortWaveRT);
    if (NT_SUCCESS(status)) status = port->Init(device, irp,
        topology ? static_cast<PUNKNOWN>(static_cast<IMiniportTopology*>(mini)) : static_cast<PUNKNOWN>(static_cast<IMiniportWaveRT*>(mini)), nullptr, resources);
    if (NT_SUCCESS(status)) status = PcRegisterSubdevice(device, name, port);
    mini->NonDelegatingRelease();
    if (NT_SUCCESS(status)) *result = port; else if (port) port->Release();
    return status;
}
NTSTATUS Start(PDEVICE_OBJECT device, PIRP irp, PRESOURCELIST resources) {
    PPORT ports[4]{};
    NTSTATUS status = STATUS_SUCCESS;
    for (ULONG i = 0; i < 4 && NT_SUCCESS(status); ++i) status = Install(device, irp, resources, i >= 2, bool(i % 2), subdeviceNames[i], &ports[i]);
    if (NT_SUCCESS(status)) status = PcRegisterPhysicalConnection(device, ports[0], 1, ports[1], 0);
    if (NT_SUCCESS(status)) status = PcRegisterPhysicalConnection(device, ports[3], 0, ports[2], 1);
    for (auto port : ports) if (port) port->Release();
    if (NT_SUCCESS(status)) MrTransportSetOnline(true);
    return status;
}
NTSTATUS Add(PDRIVER_OBJECT driver, PDEVICE_OBJECT physical) {
    if (InterlockedCompareExchange(&attached, 1, 0)) return STATUS_DEVICE_BUSY;
    auto status = PcAddAdapterDevice(driver, physical, Start, 4, 0);
    if (!NT_SUCCESS(status)) InterlockedExchange(&attached, 0);
    return status;
}
void Unload(PDRIVER_OBJECT driver) {
    MrTransportShutdown(); IoDeleteSymbolicLink(&symbolic);
    if (controlDevice) IoDeleteDevice(controlDevice);
    if (originalUnload) originalUnload(driver);
}
}
extern "C" DRIVER_INITIALIZE DriverEntry;
extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT driver, PUNICODE_STRING registry) {
    auto status = MrTransportInitialize();
    if (!NT_SUCCESS(status)) return status;
    status = PcInitializeAdapterDriver(driver, registry, Add);
    if (!NT_SUCCESS(status)) { MrTransportShutdown(); return status; }
    originalUnload = driver->DriverUnload;
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\MixRelayAudio");
    UNICODE_STRING security = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)");
    status = IoCreateDeviceSecure(driver, 0, &name, FILE_DEVICE_SOUND, FILE_DEVICE_SECURE_OPEN,
        FALSE, &security, &controlClass, &controlDevice);
    if (!NT_SUCCESS(status)) { MrTransportShutdown(); if (originalUnload) originalUnload(driver); return status; }
    status = IoCreateSymbolicLink(&symbolic, &name);
    if (!NT_SUCCESS(status)) {
        IoDeleteDevice(controlDevice); controlDevice = nullptr;
        MrTransportShutdown(); if (originalUnload) originalUnload(driver); return status;
    }
    for (ULONG i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; ++i) { original[i] = driver->MajorFunction[i]; driver->MajorFunction[i] = Dispatch; }
    controlDevice->Flags &= ~DO_DEVICE_INITIALIZING; driver->DriverUnload = Unload;
    return STATUS_SUCCESS;
}
