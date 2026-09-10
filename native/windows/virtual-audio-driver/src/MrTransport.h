#pragma once
#include <ntddk.h>
#include "../include/MixRelayTransport.h"
#include "../include/MixRelayPcm.h"
// Map/cleanup/reservation are PASSIVE_LEVEL; PCM is bounded, nonpageable DPC work.
NTSTATUS MrTransportInitialize();
// Product driver enables policy explicitly. Legacy SysVAD mappings keep v2 PCM
// behavior and do not pretend to implement a format-change notification path.
NTSTATUS MrTransportEnableRatePolicy();
NTSTATUS MrTransportRatePolicy(PIRP irp, PIO_STACK_LOCATION stack);
void MrTransportRateListener(bool capture, void (*notify)(void*), void* context);
ULONG MrTransportPreferredRate();
ULONG MrTransportForcedRate();
NTSTATUS MrTransportMap(PIRP irp, PIO_STACK_LOCATION stack);
void MrTransportCleanup(PFILE_OBJECT file);
void MrTransportShutdown();
void MrTransportSetOnline(bool online);
NTSTATUS MrTransportReserveStream(bool capture, ULONG channels, ULONG rate);
void MrTransportReleaseStream(bool capture);
void MrTransportPcm(bool capture, void* pcm, ULONG frames, ULONG channels, ULONG rate);
void MrTransportPcmNative(bool capture, void* pcm, ULONG frames, ULONG channels,
    ULONG rate, mixrelay::PcmFormat format);
