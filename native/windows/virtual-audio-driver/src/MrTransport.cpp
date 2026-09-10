#include <ntifs.h>
#include "MrTransport.h"
#include "MixRelayRing.h"
#include "../include/MixRelayRatePolicy.h"
static_assert(ULONG(mixrelay::RateState::Stable) == MIXRELAY_RATE_STABLE &&
    ULONG(mixrelay::RateState::Pending) == MIXRELAY_RATE_PENDING &&
    ULONG(mixrelay::RateState::RollbackPending) == MIXRELAY_RATE_ROLLBACK_PENDING &&
    ULONG(mixrelay::RateState::TimedOut) == MIXRELAY_RATE_TIMED_OUT &&
    ULONG(mixrelay::RateState::Cancelled) == MIXRELAY_RATE_CANCELLED, "rate state ABI");
namespace {
constexpr ULONG Tag = 'oINT';
struct Mapping {
    PMDL mdl; void* kernel; void* user; PEPROCESS process; PFILE_OBJECT file;
    HANDLE handles[2]; PKEVENT events[2];
    ULONG channels[2], rate, capacity;
    SIZE_T bytes, offsets[2];
    ULONGLONG renderWrite, captureRead, captureClock, peer[2];
};
KSPIN_LOCK pcmLock;
KMUTEX controlLock;
Mapping* active;
bool deviceOnline;
ULONG streamChannels[2], streamRates[2];
mixrelay::RatePolicy ratePolicy;
struct RateListener { void (*notify)(void*); void* context; } rateListeners[2]{};
KEVENT policyWake;
PETHREAD policyThread;
bool policyStopping;
PEPROCESS policyOwner;
void Lock() { KeWaitForSingleObject(&controlLock, Executive, KernelMode, FALSE, nullptr); }
void UnlockControl() { KeReleaseMutex(&controlLock, FALSE); }
void ReleasePolicyOwner() {
    if (policyOwner && ratePolicy.state != mixrelay::RateState::Pending) {
        ObDereferenceObject(policyOwner); policyOwner = nullptr;
    }
}
void NotifyRateChange() {
    // Listener removal is serialized by controlLock, so contexts remain alive.
    // GenerateEventList signals PortCls entries; it never calls our policy back.
    for (auto listener : rateListeners) if (listener.notify) listener.notify(listener.context);
}
void PolicyWorker(void*) {
    for (;;) {
        Lock();
        const auto now = KeQueryInterruptTime();
        if (ratePolicy.expire(now)) {
            ratePolicy.observe(streamRates[0], streamRates[1]);
            ReleasePolicyOwner(); NotifyRateChange();
        }
        const bool stop = policyStopping;
        LARGE_INTEGER timeout{};
        const bool pending = ratePolicy.state == mixrelay::RateState::Pending;
        if (pending) timeout.QuadPart = -LONGLONG(ratePolicy.deadline > now ? ratePolicy.deadline - now : 1);
        UnlockControl();
        if (stop) break;
        // No periodic idle polling: wait for a transaction or its exact deadline.
        KeWaitForSingleObject(&policyWake, Executive, KernelMode, FALSE, pending ? &timeout : nullptr);
    }
    PsTerminateSystemThread(STATUS_SUCCESS);
}
void Destroy(Mapping* m) {
    if (!m) return;
    if (m->user) {
        KAPC_STATE state;
        KeStackAttachProcess(m->process, &state);
        MmUnmapLockedPages(m->user, m->mdl);
        KeUnstackDetachProcess(&state);
    }
    if (m->kernel) MmUnmapLockedPages(m->kernel, m->mdl);
    if (m->mdl) { MmFreePagesFromMdl(m->mdl); ExFreePool(m->mdl); }
    for (ULONG i = 0; i < 2; ++i) {
        if (m->events[i]) ObDereferenceObject(m->events[i]);
        if (m->handles[i]) ZwClose(m->handles[i]);
    }
    if (m->process) ObDereferenceObject(m->process);
    ExFreePoolWithTag(m, Tag);
}
void ProcessNotification(PEPROCESS process, HANDLE, PPS_CREATE_NOTIFY_INFO created) {
    if (created) return;
    // A duplicated file handle can outlive its mapping owner. CLEANUP alone is
    // therefore insufficient: detach before the owner's address space dies.
    // The OS invokes this at PASSIVE_LEVEL in the last exiting thread's context.
    Lock();
    if (policyOwner == process && ratePolicy.cancel(ratePolicy.owner)) {
        ratePolicy.observe(streamRates[0], streamRates[1]);
        ReleasePolicyOwner(); NotifyRateChange(); KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
    }
    KIRQL irql; KeAcquireSpinLock(&pcmLock, &irql);
    auto m = active;
    if (m && m->process == process) active = nullptr; else m = nullptr;
    KeReleaseSpinLock(&pcmLock, irql);
    Destroy(m); UnlockControl();
}
}
NTSTATUS MrTransportInitialize() {
    KeInitializeSpinLock(&pcmLock); KeInitializeMutex(&controlLock, 0);
    return PsSetCreateProcessNotifyRoutineEx(ProcessNotification, FALSE);
}
NTSTATUS MrTransportEnableRatePolicy() {
    KeInitializeEvent(&policyWake, SynchronizationEvent, FALSE);
    HANDLE handle{};
    OBJECT_ATTRIBUTES attributes;
    InitializeObjectAttributes(&attributes, nullptr, OBJ_KERNEL_HANDLE, nullptr, nullptr);
    auto status = PsCreateSystemThread(&handle, THREAD_ALL_ACCESS, &attributes, nullptr, nullptr, PolicyWorker, nullptr);
    if (!NT_SUCCESS(status)) return status;
    status = ObReferenceObjectByHandle(handle, SYNCHRONIZE, *PsThreadType, KernelMode,
        reinterpret_cast<void**>(&policyThread), nullptr);
    if (!NT_SUCCESS(status)) {
        Lock(); policyStopping = true; UnlockControl();
        KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
        ZwWaitForSingleObject(handle, FALSE, nullptr);
    }
    ZwClose(handle); return status;
}
void MrTransportRateListener(bool capture, void (*notify)(void*), void* context) {
    Lock(); auto& listener = rateListeners[capture ? 1 : 0];
    if (notify || listener.context == context) listener = {notify, notify ? context : nullptr};
    UnlockControl();
}
ULONG MrTransportPreferredRate() { Lock(); auto value = ratePolicy.selected(); UnlockControl(); return value; }
ULONG MrTransportForcedRate() { Lock(); auto value = ratePolicy.forced(); UnlockControl(); return value; }
NTSTATUS MrTransportRatePolicy(PIRP irp, PIO_STACK_LOCATION s) {
    if (!policyThread) return STATUS_NOT_SUPPORTED;
    if (KeGetCurrentIrql() != PASSIVE_LEVEL || irp->RequestorMode != UserMode ||
        IoGetRequestorProcess(irp) != PsGetCurrentProcess()) return STATUS_INVALID_DEVICE_REQUEST;
    if (s->Parameters.DeviceIoControl.InputBufferLength != sizeof(MIXRELAY_RATE_REQUEST) ||
        s->Parameters.DeviceIoControl.OutputBufferLength < sizeof(MIXRELAY_RATE_STATUS)) return STATUS_BUFFER_TOO_SMALL;
    const auto request = *static_cast<MIXRELAY_RATE_REQUEST*>(irp->AssociatedIrp.SystemBuffer);
    if (request.Version != MIXRELAY_RATE_POLICY_VERSION || request.Size != sizeof(request) ||
        request.Reserved || request.Operation > MIXRELAY_RATE_CONFIRM_CONFIG) return STATUS_INVALID_PARAMETER;
    Lock(); NTSTATUS status = STATUS_SUCCESS;
    const auto owner = reinterpret_cast<ULONGLONG>(s->FileObject);
    const auto before = ratePolicy.generation;
    const auto stateBefore = ratePolicy.state;
    // Expiry precedes acknowledgement; an overdue reply cannot beat the worker
    // and revive a generation whose monotonic deadline has already elapsed.
    ratePolicy.expire(KeQueryInterruptTime());
    if (!deviceOnline) status = STATUS_DEVICE_NOT_READY;
    else if (request.Operation == MIXRELAY_RATE_BEGIN) {
        auto result = ratePolicy.begin(request.SampleRate, request.ExpectedGeneration, KeQueryInterruptTime(),
            request.TimeoutMs, owner, rateListeners[0].notify && rateListeners[1].notify, active != nullptr);
        if (result == mixrelay::RateResult::Invalid) status = STATUS_INVALID_PARAMETER;
        else if (result == mixrelay::RateResult::Conflict) status = STATUS_REVISION_MISMATCH;
        else if (result == mixrelay::RateResult::Busy) status = STATUS_DEVICE_BUSY;
        else if (result == mixrelay::RateResult::NoListener) status = STATUS_NOT_SUPPORTED;
        if (NT_SUCCESS(status) && ratePolicy.generation != before) {
            policyOwner = PsGetCurrentProcess(); ObReferenceObject(policyOwner);
        }
    } else if (request.Operation == MIXRELAY_RATE_CANCEL) {
        if (request.ExpectedGeneration != ratePolicy.generation) status = STATUS_REVISION_MISMATCH;
        else if (!ratePolicy.cancel(owner)) status = STATUS_ACCESS_DENIED;
    } else if (request.Operation == MIXRELAY_RATE_CONFIRM_CONFIG) {
        const auto result = ratePolicy.confirm(request.SampleRate, request.ExpectedGeneration,
            owner, streamRates[0], streamRates[1]);
        if (result == mixrelay::RateResult::Invalid) status = STATUS_INVALID_PARAMETER;
        else if (result == mixrelay::RateResult::Conflict) status = STATUS_REVISION_MISMATCH;
        else if (result == mixrelay::RateResult::Unauthorized) status = STATUS_ACCESS_DENIED;
        else if (result == mixrelay::RateResult::Busy) status = STATUS_DEVICE_BUSY;
    }
    if (ratePolicy.expire(KeQueryInterruptTime()) || ratePolicy.generation != before) {
        ratePolicy.observe(streamRates[0], streamRates[1]);
        ReleasePolicyOwner(); NotifyRateChange(); KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
    }
    ReleasePolicyOwner();
    if (stateBefore != ratePolicy.state) KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
    if (NT_SUCCESS(status)) {
        MIXRELAY_RATE_STATUS result{MIXRELAY_RATE_POLICY_VERSION, sizeof(MIXRELAY_RATE_STATUS),
            ULONG(ratePolicy.state), ratePolicy.preferred, ratePolicy.requested, streamRates[0], streamRates[1],
            0, ratePolicy.generation, ratePolicy.deadline};
        RtlCopyMemory(irp->AssociatedIrp.SystemBuffer, &result, sizeof(result));
        irp->IoStatus.Information = sizeof(result);
    }
    UnlockControl(); return status;
}
NTSTATUS MrTransportMap(PIRP irp, PIO_STACK_LOCATION s) {
    if (KeGetCurrentIrql() != PASSIVE_LEVEL || irp->RequestorMode != UserMode ||
        IoGetRequestorProcess(irp) != PsGetCurrentProcess()) return STATUS_INVALID_DEVICE_REQUEST;
    if (s->Parameters.DeviceIoControl.InputBufferLength != sizeof(MIXRELAY_TRANSPORT_REQUEST) ||
        s->Parameters.DeviceIoControl.OutputBufferLength < sizeof(MIXRELAY_TRANSPORT_DESCRIPTOR)) return STATUS_BUFFER_TOO_SMALL;
    const auto r = *static_cast<MIXRELAY_TRANSPORT_REQUEST*>(irp->AssociatedIrp.SystemBuffer);
    if (r.Version != MIXRELAY_TRANSPORT_VERSION || r.Size != sizeof(r) ||
        r.RenderChannels > 64 || r.CaptureChannels > 64 || (!r.RenderChannels && !r.CaptureChannels) ||
        r.CapacityFrames < 256 || r.CapacityFrames > MIXRELAY_MAX_CAPACITY_FRAMES ||
        (r.SampleRate != 44100 && r.SampleRate != 48000 && r.SampleRate != 96000)) return STATUS_INVALID_PARAMETER;
    Lock();
    if (!deviceOnline) { UnlockControl(); return STATUS_DEVICE_NOT_READY; }
    if (policyThread && !ratePolicy.mappingAllowed()) { UnlockControl(); return STATUS_DEVICE_BUSY; }
    if (active || (streamChannels[0] && (streamChannels[0] != r.RenderChannels || streamRates[0] != r.SampleRate)) ||
        (streamChannels[1] && (streamChannels[1] != r.CaptureChannels || streamRates[1] != r.SampleRate))) {
        UnlockControl(); return STATUS_DEVICE_BUSY;
    }
    auto m = static_cast<Mapping*>(ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(Mapping), Tag));
    if (!m) { UnlockControl(); return STATUS_INSUFFICIENT_RESOURCES; }
    m->channels[0] = r.RenderChannels; m->channels[1] = r.CaptureChannels;
    m->capacity = r.CapacityFrames; m->rate = r.SampleRate;
    // All arithmetic is bounded by validated 64 channels / 16384 frames. Never
    // derive a kernel copy length or address from the user-writable header.
    m->offsets[0] = PAGE_SIZE;
    m->offsets[1] = PAGE_SIZE + SIZE_T(m->capacity) * m->channels[0] * 4;
    m->bytes = (m->offsets[1] + SIZE_T(m->capacity) * m->channels[1] * 4 + PAGE_SIZE - 1) & ~(SIZE_T(PAGE_SIZE) - 1);
    m->file = s->FileObject; m->process = PsGetCurrentProcess(); ObReferenceObject(m->process);
    PHYSICAL_ADDRESS low{}, high{}, skip{}; high.QuadPart = MAXLONGLONG;
    m->mdl = MmAllocatePagesForMdlEx(low, high, skip, m->bytes, MmCached, MM_ALLOCATE_FULLY_REQUIRED);
    NTSTATUS status = STATUS_INSUFFICIENT_RESOURCES;
    HANDLE userEvents[2]{};
    if (m->mdl) m->kernel = MmMapLockedPagesSpecifyCache(m->mdl, KernelMode, MmCached, nullptr, FALSE, NormalPagePriority | MdlMappingNoExecute);
    if (m->kernel) {
        RtlZeroMemory(m->kernel, m->bytes);
        // User mapping may raise even with BugCheckOnFailure disabled.
        __try { m->user = MmMapLockedPagesSpecifyCache(m->mdl, UserMode, MmCached, nullptr, FALSE, NormalPagePriority | MdlMappingNoExecute); }
        __except (EXCEPTION_EXECUTE_HANDLER) { status = GetExceptionCode(); }
        if (m->user) status = STATUS_SUCCESS;
    }
    for (ULONG i = 0; NT_SUCCESS(status) && i < 2; ++i) {
        OBJECT_ATTRIBUTES a; InitializeObjectAttributes(&a, nullptr, OBJ_KERNEL_HANDLE, nullptr, nullptr);
        status = ZwCreateEvent(&m->handles[i], EVENT_ALL_ACCESS, &a, SynchronizationEvent, FALSE);
        if (NT_SUCCESS(status)) status = ObReferenceObjectByHandle(m->handles[i], EVENT_ALL_ACCESS, *ExEventObjectType,
            KernelMode, reinterpret_cast<void**>(&m->events[i]), nullptr);
        if (NT_SUCCESS(status)) status = ObOpenObjectByPointer(m->events[i], 0, nullptr, SYNCHRONIZE,
            *ExEventObjectType, KernelMode, &userEvents[i]);
    }
    if (NT_SUCCESS(status)) {
        auto h = static_cast<MIXRELAY_SHARED_AUDIO_HEADER*>(m->kernel);
        h->Magic = MIXRELAY_TRANSPORT_MAGIC; h->Version = MIXRELAY_TRANSPORT_VERSION;
        h->HeaderSize = sizeof(*h); h->CapacityFrames = m->capacity;
        h->RenderChannels = m->channels[0]; h->CaptureChannels = m->channels[1]; h->SampleRate = m->rate;
        h->State = MIXRELAY_TRANSPORT_ONLINE;
        h->RenderSamplesOffset = m->offsets[0]; h->CaptureSamplesOffset = m->offsets[1]; h->MappingBytes = m->bytes;
        MIXRELAY_TRANSPORT_DESCRIPTOR result{MIXRELAY_TRANSPORT_VERSION, sizeof(result), m->bytes,
            reinterpret_cast<ULONGLONG>(m->user), reinterpret_cast<ULONGLONG>(userEvents[0]), reinterpret_cast<ULONGLONG>(userEvents[1])};
        RtlCopyMemory(irp->AssociatedIrp.SystemBuffer, &result, sizeof(result)); irp->IoStatus.Information = sizeof(result);
        KIRQL irql; KeAcquireSpinLock(&pcmLock, &irql); active = m; KeReleaseSpinLock(&pcmLock, irql);
    } else { for (auto event : userEvents) if (event) ZwClose(event); Destroy(m); }
    UnlockControl(); return status;
}
void MrTransportCleanup(PFILE_OBJECT file) {
    Lock(); KIRQL irql; KeAcquireSpinLock(&pcmLock, &irql);
    Mapping* m = active;
    if (m && (!file || m->file == file)) active = nullptr; else m = nullptr;
    KeReleaseSpinLock(&pcmLock, irql);
    if (policyThread && ratePolicy.cancel(file ? reinterpret_cast<ULONGLONG>(file) : ratePolicy.owner)) {
        ratePolicy.observe(streamRates[0], streamRates[1]);
        ReleasePolicyOwner(); NotifyRateChange(); KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
    }
    // Detaching under the PCM lock drains every old DPC reference before unmap.
    Destroy(m); UnlockControl();
}
void MrTransportShutdown() {
    if (policyThread) {
        Lock(); policyStopping = true; UnlockControl();
        KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE);
        KeWaitForSingleObject(policyThread, Executive, KernelMode, FALSE, nullptr);
        ObDereferenceObject(policyThread); policyThread = nullptr;
    }
    // Removal waits for in-flight exit callbacks; never hold controlLock here.
    PsSetCreateProcessNotifyRoutineEx(ProcessNotification, TRUE);
    MrTransportCleanup(nullptr);
    if (policyOwner) { ObDereferenceObject(policyOwner); policyOwner = nullptr; }
}
void MrTransportSetOnline(bool online) {
    Lock();
    KIRQL irql; KeAcquireSpinLock(&pcmLock, &irql);
    deviceOnline = online;
    if (active) {
        auto header = static_cast<MIXRELAY_SHARED_AUDIO_HEADER*>(active->kernel);
        InterlockedExchange(&header->State, online ? MIXRELAY_TRANSPORT_ONLINE : MIXRELAY_TRANSPORT_OFFLINE);
        for (auto event : active->events) KeSetEvent(event, IO_NO_INCREMENT, FALSE);
    }
    KeReleaseSpinLock(&pcmLock, irql);
    // PnP removal invalidates the stream, not the user's pointer. Keep pages
    // owned until file cleanup or owner exit, so a waking reader can see State.
    UnlockControl();
}
NTSTATUS MrTransportReserveStream(bool capture, ULONG channels, ULONG rate) {
    const ULONG side = capture ? 1 : 0; Lock(); NTSTATUS status = STATUS_DEVICE_BUSY;
    if (deviceOnline && !streamChannels[side] &&
        (!policyThread || ratePolicy.accepts(rate, streamRates[1-side])) &&
        (!active || (active->channels[side] == channels && active->rate == rate))) {
        streamChannels[side] = channels; streamRates[side] = rate; status = STATUS_SUCCESS;
        if (policyThread) { ratePolicy.observe(streamRates[0], streamRates[1]); ReleasePolicyOwner(); KeSetEvent(&policyWake, IO_NO_INCREMENT, FALSE); }
    }
    UnlockControl(); return status;
}
void MrTransportReleaseStream(bool capture) { Lock(); streamChannels[capture ? 1 : 0] = 0; streamRates[capture ? 1 : 0] = 0; UnlockControl(); }
void MrTransportPcm(bool capture, void* pcm, ULONG frames, ULONG channels, ULONG rate) {
    MrTransportPcmNative(capture, pcm, frames, channels, rate, {4, 0});
}
void MrTransportPcmNative(bool capture, void* pcm, ULONG frames, ULONG channels,
    ULONG rate, mixrelay::PcmFormat format) {
    // Callers validate format at stream creation, before a DPC can use it.
    NT_ASSERT(format.valid() && channels && channels <= MIXRELAY_MAX_CHANNELS);
    KIRQL irql; KeAcquireSpinLock(&pcmLock, &irql); auto m = active;
    if (!deviceOnline || !m || m->channels[capture ? 1 : 0] != channels || m->rate != rate) {
        if (capture) RtlZeroMemory(pcm, SIZE_T(frames) * channels * format.bytes);
        KeReleaseSpinLock(&pcmLock, irql); return;
    }
    auto h = static_cast<MIXRELAY_SHARED_AUDIO_HEADER*>(m->kernel);
    const ULONGLONG owned = capture ? m->captureRead : m->renderWrite;
    const auto peer = ULONGLONG(InterlockedCompareExchange64(capture ? &h->CaptureWriteFrame : &h->RenderReadFrame, 0, 0));
    const auto plan = mixrelay::Plan(capture, owned, peer, m->peer[capture ? 1 : 0], m->capacity, frames);
    if (plan.valid) m->peer[capture ? 1 : 0] = peer;
    const ULONG count = plan.count();
    auto ring = static_cast<UCHAR*>(m->kernel) + m->offsets[capture ? 1 : 0];
    if (!format.validBits) mixrelay::Copy(capture, ring, pcm, plan, frames, channels);
    else mixrelay::CopyPcm(capture, ring, pcm, plan, frames, channels, format);
    if (capture) {
        m->captureRead += count; InterlockedExchange64(&h->CaptureReadFrame, LONG64(m->captureRead));
        m->captureClock += frames;
        InterlockedExchange64(&h->CaptureClockFrame, LONG64(m->captureClock));
        if (count != frames) InterlockedIncrement64(&h->CaptureUnderruns);
    } else {
        m->renderWrite += count; InterlockedExchange64(&h->RenderWriteFrame, LONG64(m->renderWrite));
        if (count != frames) InterlockedIncrement64(&h->RenderOverruns);
    }
    KeSetEvent(m->events[capture ? 1 : 0], IO_NO_INCREMENT, FALSE); KeReleaseSpinLock(&pcmLock, irql);
}
