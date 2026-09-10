#pragma once
#include <portcls.h>
#include "MrTransport.h"
#include <ks.h>
#include <ksmedia.h>
NTSTATUS MrBridgeInitialize(PDRIVER_OBJECT driver);
void MrBridgeShutdown();
NTSTATUS MrBridgeReserve(bool capture, const WAVEFORMATEX* wave, mixrelay::PcmFormat* format);
#ifdef MIXRELAY_SYSVAD_RATE_POLICY
// These helpers select existing descriptor entries, never synthesize unsupported
// PCM formats. Enabled only in the separately qualified policy overlay.
PKSDATAFORMAT MrBridgeDefaultFormat(PKSDATAFORMAT current,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count);
NTSTATUS MrBridgeIntersect(PKSDATARANGE client, PKSDATARANGE mine,
    PKSDATAFORMAT_WAVEFORMATEXTENSIBLE formats, ULONG count,
    ULONG size, PVOID output, PULONG needed);
#endif
