#pragma once
// Version 2: locked pages mapped into the requesting process, never kernel addresses.
#if !defined(_NTDDK_) && !defined(_WDMDDK_)
#include <windows.h>
#include <winioctl.h>
#endif
#define TONENARI_TRANSPORT_VERSION 2u
#define TONENARI_TRANSPORT_MAGIC 0x4F494E54u
#define TONENARI_MAX_CHANNELS 64u
#define TONENARI_MAX_CAPACITY_FRAMES 16384u
#define TONENARI_TRANSPORT_OFFLINE 0u
#define TONENARI_TRANSPORT_ONLINE 1u
#define TONENARI_CONTROL_PATH L"\\\\.\\ToneNariAudio"
#define IOCTL_TONENARI_GET_TRANSPORT CTL_CODE(FILE_DEVICE_SOUND, 0x800, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA)
// Control-policy ABI is independent of the unchanged v2 PCM mapping ABI.
#define TONENARI_RATE_POLICY_VERSION 1u
#define IOCTL_TONENARI_RATE_POLICY CTL_CODE(FILE_DEVICE_SOUND, 0x802, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA)
#define TONENARI_RATE_QUERY 0u
#define TONENARI_RATE_BEGIN 1u
#define TONENARI_RATE_CANCEL 2u
#define TONENARI_RATE_CONFIRM_CONFIG 3u
#define TONENARI_RATE_STABLE 0u
#define TONENARI_RATE_PENDING 1u
#define TONENARI_RATE_ROLLBACK_PENDING 2u
#define TONENARI_RATE_TIMED_OUT 3u
#define TONENARI_RATE_CANCELLED 4u
typedef struct TONENARI_RATE_REQUEST {
    ULONG Version, Size, Operation, SampleRate;
    ULONGLONG ExpectedGeneration;
    ULONG TimeoutMs, Reserved;
} TONENARI_RATE_REQUEST;
typedef struct TONENARI_RATE_STATUS {
    ULONG Version, Size, State, PreferredRate, RequestedRate, RenderRate, CaptureRate, Reserved;
    ULONGLONG Generation, Deadline100ns;
} TONENARI_RATE_STATUS;
static_assert(sizeof(TONENARI_RATE_REQUEST) == 32, "rate policy request ABI");
static_assert(sizeof(TONENARI_RATE_STATUS) == 48, "rate policy status ABI");
typedef struct TONENARI_TRANSPORT_REQUEST {
    ULONG Version, Size, RenderChannels, CaptureChannels, SampleRate, CapacityFrames;
} TONENARI_TRANSPORT_REQUEST;
typedef struct TONENARI_TRANSPORT_DESCRIPTOR {
    ULONG Version, Size;
    ULONGLONG MappingBytes, UserAddress, RenderReadyEvent, CaptureReadyEvent;
} TONENARI_TRANSPORT_DESCRIPTOR;
typedef struct DECLSPEC_ALIGN(8) TONENARI_SHARED_AUDIO_HEADER {
    ULONG Magic, Version, HeaderSize, CapacityFrames;
    ULONG RenderChannels, CaptureChannels, SampleRate;
    volatile LONG State;
    volatile LONG64 RenderWriteFrame, RenderReadFrame, CaptureWriteFrame, CaptureReadFrame;
    ULONGLONG RenderSamplesOffset, CaptureSamplesOffset, MappingBytes;
    volatile LONG64 RenderOverruns, CaptureUnderruns, CaptureClockFrame;
} TONENARI_SHARED_AUDIO_HEADER;
static_assert(sizeof(TONENARI_TRANSPORT_REQUEST) == 24, "request ABI");
static_assert(sizeof(TONENARI_TRANSPORT_DESCRIPTOR) == 40, "descriptor ABI");
static_assert(sizeof(TONENARI_SHARED_AUDIO_HEADER) == 112, "shared header ABI");
