#pragma once
// Version 2: locked pages mapped into the requesting process, never kernel addresses.
#if !defined(_NTDDK_) && !defined(_WDMDDK_)
#include <windows.h>
#include <winioctl.h>
#endif
#define MIXRELAY_TRANSPORT_VERSION 2u
#define MIXRELAY_TRANSPORT_MAGIC 0x4F49524Du
#define MIXRELAY_MAX_CHANNELS 64u
#define MIXRELAY_MAX_CAPACITY_FRAMES 16384u
#define MIXRELAY_TRANSPORT_OFFLINE 0u
#define MIXRELAY_TRANSPORT_ONLINE 1u
#define MIXRELAY_CONTROL_PATH L"\\\\.\\MixRelayAudio"
#define IOCTL_MIXRELAY_GET_TRANSPORT CTL_CODE(FILE_DEVICE_SOUND, 0x800, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA)
// Control-policy ABI is independent of the unchanged v2 PCM mapping ABI.
#define MIXRELAY_RATE_POLICY_VERSION 1u
#define IOCTL_MIXRELAY_RATE_POLICY CTL_CODE(FILE_DEVICE_SOUND, 0x802, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA)
#define MIXRELAY_RATE_QUERY 0u
#define MIXRELAY_RATE_BEGIN 1u
#define MIXRELAY_RATE_CANCEL 2u
#define MIXRELAY_RATE_CONFIRM_CONFIG 3u
#define MIXRELAY_RATE_STABLE 0u
#define MIXRELAY_RATE_PENDING 1u
#define MIXRELAY_RATE_ROLLBACK_PENDING 2u
#define MIXRELAY_RATE_TIMED_OUT 3u
#define MIXRELAY_RATE_CANCELLED 4u
typedef struct MIXRELAY_RATE_REQUEST {
    ULONG Version, Size, Operation, SampleRate;
    ULONGLONG ExpectedGeneration;
    ULONG TimeoutMs, Reserved;
} MIXRELAY_RATE_REQUEST;
typedef struct MIXRELAY_RATE_STATUS {
    ULONG Version, Size, State, PreferredRate, RequestedRate, RenderRate, CaptureRate, Reserved;
    ULONGLONG Generation, Deadline100ns;
} MIXRELAY_RATE_STATUS;
static_assert(sizeof(MIXRELAY_RATE_REQUEST) == 32, "rate policy request ABI");
static_assert(sizeof(MIXRELAY_RATE_STATUS) == 48, "rate policy status ABI");
typedef struct MIXRELAY_TRANSPORT_REQUEST {
    ULONG Version, Size, RenderChannels, CaptureChannels, SampleRate, CapacityFrames;
} MIXRELAY_TRANSPORT_REQUEST;
typedef struct MIXRELAY_TRANSPORT_DESCRIPTOR {
    ULONG Version, Size;
    ULONGLONG MappingBytes, UserAddress, RenderReadyEvent, CaptureReadyEvent;
} MIXRELAY_TRANSPORT_DESCRIPTOR;
typedef struct DECLSPEC_ALIGN(8) MIXRELAY_SHARED_AUDIO_HEADER {
    ULONG Magic, Version, HeaderSize, CapacityFrames;
    ULONG RenderChannels, CaptureChannels, SampleRate;
    volatile LONG State;
    volatile LONG64 RenderWriteFrame, RenderReadFrame, CaptureWriteFrame, CaptureReadFrame;
    ULONGLONG RenderSamplesOffset, CaptureSamplesOffset, MappingBytes;
    volatile LONG64 RenderOverruns, CaptureUnderruns, CaptureClockFrame;
} MIXRELAY_SHARED_AUDIO_HEADER;
static_assert(sizeof(MIXRELAY_TRANSPORT_REQUEST) == 24, "request ABI");
static_assert(sizeof(MIXRELAY_TRANSPORT_DESCRIPTOR) == 40, "descriptor ABI");
static_assert(sizeof(MIXRELAY_SHARED_AUDIO_HEADER) == 112, "shared header ABI");
