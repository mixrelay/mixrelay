#pragma once
#include <ntddk.h>
#include "../include/ToneNariTransport.h"
#include "../include/ToneNariPcm.h"
// Map/cleanup/reservation are PASSIVE_LEVEL; PCM is bounded, nonpageable DPC work.
NTSTATUS TnTransportInitialize();
// Product driver enables policy explicitly. Legacy SysVAD mappings keep v2 PCM
// behavior and do not pretend to implement a format-change notification path.
NTSTATUS TnTransportEnableRatePolicy();
NTSTATUS TnTransportRatePolicy(PIRP irp, PIO_STACK_LOCATION stack);
void TnTransportRateListener(bool capture, void (*notify)(void*), void* context);
ULONG TnTransportPreferredRate();
ULONG TnTransportForcedRate();
NTSTATUS TnTransportMap(PIRP irp, PIO_STACK_LOCATION stack);
void TnTransportCleanup(PFILE_OBJECT file);
void TnTransportShutdown();
void TnTransportSetOnline(bool online);
NTSTATUS TnTransportReserveStream(bool capture, ULONG channels, ULONG rate);
void TnTransportReleaseStream(bool capture);
void TnTransportPcm(bool capture, void* pcm, ULONG frames, ULONG channels, ULONG rate);
void TnTransportPcmNative(bool capture, void* pcm, ULONG frames, ULONG channels,
    ULONG rate, tonenari::PcmFormat format);
