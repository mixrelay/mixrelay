#pragma once
#include <portcls.h>
#include "TnTransport.h"
#include <ks.h>
#include <ksmedia.h>
NTSTATUS TnBridgeInitialize(PDRIVER_OBJECT driver);
void TnBridgeShutdown();
NTSTATUS TnBridgeReserve(bool capture, const WAVEFORMATEX* wave, tonenari::PcmFormat* format);
#ifdef TONENARI_SYSVAD_RATE_POLICY
// These helpers select existing descriptor entries, never synthesize unsupported
// PCM formats. Enabled only in the separately qualified policy overlay.
PKSDATAFORMAT TnBridgeDefaultFormat(PKSDATAFORMAT current,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count);
NTSTATUS TnBridgeIntersect(PKSDATARANGE client, PKSDATARANGE mine,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count,
    ULONG size, PVOID output, PULONG needed);
#endif
