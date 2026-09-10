#pragma once
#if defined(_NTDDK_) || defined(_WDMDDK_)
#include "MrTransport.h"
#else
#include "MixRelayTransport.h"
#endif
// Bounded, read-only lab telemetry. No sample contents or kernel addresses.
#define IOCTL_TN_BRIDGE_TRACE CTL_CODE(FILE_DEVICE_SOUND, 0x801, METHOD_BUFFERED, FILE_READ_DATA)
struct MrBridgeAttempt { ULONG capture, channels, rate, bits, validBits, status; };
struct MrBridgeTrace { ULONG version, count; MrBridgeAttempt attempts[256]; };
