#include "TNDeviceIO.h"
#include "TNAggregateGroup.h"
#include "TNPcmRing.h"
#include "../../common/TNCallbackClock.h"
#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

enum { TN_DEVICE_MAX_FRAMES = 4096, TN_DEVICE_MAX_AGGREGATE_CHANNELS = 2048 };
struct TNDeviceIO {
    char *uid;
    bool input, prime_output;
    uint32_t channels, rate;
    /* Immutable group ownership belongs to the Rust Native Arc, which outlives
     * registration teardown. Offsets are resolved before starting an IOProc. */
    TNAggregateGroup *group;
    uint32_t channel_offset, total_channels;
    AudioDeviceID device;
    AudioDeviceIOProcID proc;
    TNPcmRing ring;
    TNPcmRing graph_ring;
    float *scratch;
    _Atomic uint64_t ticks;
    _Atomic uint64_t native_frames;
    _Atomic uint64_t graph_frames, output_epoch;
    _Atomic uint32_t native_quantum, graph_quantum;
    uint64_t callback_epoch; /* native output consumer only */
    _Atomic bool invalid;
    uint64_t read_epoch; /* native-ring worker consumer only */
    uint64_t graph_read_epoch; /* graph consumer only */
    TNCallbackClock clock;
    void *render_context;
    TNRenderHook render_hook;
};

static bool get(AudioObjectID id, AudioObjectPropertySelector selector,
                AudioObjectPropertyScope scope, UInt32 size, void *out) {
    AudioObjectPropertyAddress address = {selector, scope, kAudioObjectPropertyElementMain};
    UInt32 actual = size;
    return AudioObjectGetPropertyData(id, &address, 0, NULL, &actual, out) == noErr && actual == size;
}

static bool resolve(TNDeviceIO *io) {
    if (io->group) return tn_aggregate_group_binding(io->group, io->uid, io->input, io->channels,
        &io->device, &io->channel_offset, &io->total_channels);
    if (!io->uid[0]) {
        return get(kAudioObjectSystemObject, io->input ? kAudioHardwarePropertyDefaultInputDevice : kAudioHardwarePropertyDefaultOutputDevice,
                   kAudioObjectPropertyScopeGlobal, sizeof(io->device), &io->device) && io->device != kAudioObjectUnknown;
    }
    CFStringRef uid = CFStringCreateWithCString(NULL, io->uid, kCFStringEncodingUTF8);
    if (!uid) return false;
    AudioObjectPropertyAddress address = {kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = sizeof(io->device);
    bool ok = AudioObjectGetPropertyData(kAudioObjectSystemObject, &address,
        sizeof(uid), &uid, &size, &io->device) == noErr && size == sizeof(io->device);
    CFRelease(uid);
    return ok && io->device != kAudioObjectUnknown;
}

/* Do not change the user's hardware rate, and do not reinterpret integer PCM
 * as Float32. Every stream must match; unsupported formats fail preparation. */
static bool native_format(TNDeviceIO *io, uint32_t *native_rate) {
    uint32_t limit = io->group ? TN_DEVICE_MAX_AGGREGATE_CHANNELS : 64;
    AudioObjectPropertyScope scope = io->input ? kAudioDevicePropertyScopeInput : kAudioDevicePropertyScopeOutput;
    AudioObjectPropertyAddress address = {kAudioDevicePropertyStreams, scope, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(io->device, &address, 0, NULL, &size) != noErr ||
        !size || size % sizeof(AudioStreamID) || size > limit * sizeof(AudioStreamID)) return false;
    AudioStreamID streams[TN_DEVICE_MAX_AGGREGATE_CHANNELS];
    if (AudioObjectGetPropertyData(io->device, &address, 0, NULL, &size, streams) != noErr) return false;
    uint32_t channels = 0;
    uint32_t rate = 0;
    for (UInt32 i = 0; i < size / sizeof(AudioStreamID); ++i) {
        AudioStreamBasicDescription format = {0};
        if (!get(streams[i], kAudioStreamPropertyVirtualFormat, kAudioObjectPropertyScopeGlobal, sizeof(format), &format) ||
            format.mFormatID != kAudioFormatLinearPCM || !(format.mFormatFlags & kAudioFormatFlagIsFloat) ||
            (format.mFormatFlags & kAudioFormatFlagIsBigEndian) || format.mBitsPerChannel != 32 ||
            !isfinite(format.mSampleRate) || format.mSampleRate < 8000 || format.mSampleRate > 384000 ||
            format.mSampleRate != (uint32_t)format.mSampleRate ||
            (rate && format.mSampleRate != rate) || !format.mChannelsPerFrame || format.mChannelsPerFrame > limit ||
            format.mBytesPerFrame != sizeof(float) * ((format.mFormatFlags & kAudioFormatFlagIsNonInterleaved) ? 1 : format.mChannelsPerFrame)) return false;
        channels += format.mChannelsPerFrame;
        rate = (uint32_t)format.mSampleRate;
    }
    *native_rate = rate;
    return channels >= io->channel_offset + io->channels && channels <= limit &&
        (!io->group || channels == io->total_channels);
}

static OSStatus process(AudioDeviceID device, const AudioTimeStamp *now,
                        const AudioBufferList *input, const AudioTimeStamp *input_time,
                        AudioBufferList *output, const AudioTimeStamp *output_time, void *context) {
    (void)device; (void)now;
    TNDeviceIO *io = context;
    /* Each IOProc contributes its own output buffers. Input-only clients must
     * contribute silence, never leave uninitialized bytes in a duplex device. */
    if (output) for (UInt32 b = 0; b < output->mNumberBuffers; ++b)
        if (output->mBuffers[b].mData) memset(output->mBuffers[b].mData, 0, output->mBuffers[b].mDataByteSize);
    /* A layout failure is terminal for this registration. Do not reinterpret a
     * later buffer or replay queued output before the control owner reconnects. */
    if (atomic_load_explicit(&io->invalid, memory_order_acquire)) return noErr;
    const AudioBufferList *buffers = io->input ? input : output;
    uint32_t limit = io->group ? TN_DEVICE_MAX_AGGREGATE_CHANNELS : 64;
    if (!buffers || !buffers->mNumberBuffers || buffers->mNumberBuffers > limit) return noErr;
    uint32_t frames = 0, width = 0;
    for (UInt32 b = 0; b < buffers->mNumberBuffers; ++b) {
        const AudioBuffer *buffer = &buffers->mBuffers[b];
        if (!buffer->mNumberChannels || buffer->mNumberChannels > limit ||
            buffer->mDataByteSize % (sizeof(float) * buffer->mNumberChannels)) goto invalid;
        if (!buffer->mData) {
            /* Disabled streams retain their channel positions. Only a selected
             * channel requires backing PCM; Rust validates its complete maps. */
            if (width < io->channel_offset + io->channels &&
                width + buffer->mNumberChannels > io->channel_offset) goto invalid;
            width += buffer->mNumberChannels;
            continue;
        }
        uint32_t count = buffer->mDataByteSize / (sizeof(float) * buffer->mNumberChannels);
        if (!count || count > TN_DEVICE_MAX_FRAMES || (frames && count != frames)) goto invalid;
        frames = count;
        width += buffer->mNumberChannels;
    }
    if (width < io->channel_offset + io->channels || width > limit ||
        (io->group && width != io->total_channels)) goto invalid;
    tn_callback_clock_record(&io->clock, io->input ? input_time : output_time, frames);
    atomic_store_explicit(&io->native_quantum, frames, memory_order_relaxed);
    if (io->render_hook && io->render_hook(io->render_context, input, output, frames)) {
        atomic_fetch_add_explicit(&io->ticks, 1, memory_order_relaxed);
        atomic_fetch_add_explicit(&io->native_frames, frames, memory_order_release);
        return noErr;
    }
    if (!io->input) {
        uint64_t epoch = atomic_load_explicit(&io->output_epoch, memory_order_acquire);
        if (io->prime_output || io->callback_epoch != epoch) {
            tn_pcm_ring_discard(&io->ring); io->prime_output = false; io->callback_epoch = epoch;
        }
        tn_pcm_ring_read_available(&io->ring, io->scratch, frames);
    }
    uint32_t channel = 0;
    for (UInt32 b = 0; b < buffers->mNumberBuffers && channel < io->channel_offset + io->channels; ++b) {
        const AudioBuffer *buffer = &buffers->mBuffers[b];
        float *samples = buffer->mData;
        for (uint32_t c = 0; c < buffer->mNumberChannels && channel + c < io->channel_offset + io->channels; ++c) {
            if (channel + c < io->channel_offset) continue;
            uint32_t selected = channel + c - io->channel_offset;
            for (uint32_t f = 0; f < frames; ++f) {
                if (io->input) io->scratch[(size_t)f * io->channels + selected] = samples[(size_t)f * buffer->mNumberChannels + c];
                else samples[(size_t)f * buffer->mNumberChannels + c] = io->scratch[(size_t)f * io->channels + selected];
            }
        }
        channel += buffer->mNumberChannels;
    }
    if (io->input) tn_pcm_ring_write(&io->ring, io->scratch, frames);
    atomic_fetch_add_explicit(&io->ticks, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&io->native_frames, frames, memory_order_release);
    return noErr;
invalid:
    atomic_store_explicit(&io->invalid, true, memory_order_release);
    return noErr;
}

TNDeviceIO *tn_device_io_create(const char *uid, bool input, uint32_t channels, uint32_t rate) {
    if (!uid || !channels || channels > 64 || !rate) return NULL;
    TNDeviceIO *io = calloc(1, sizeof(*io));
    if (!io) return NULL;
    io->input = input; io->channels = channels; io->rate = rate;
    tn_callback_clock_init(&io->clock);
    atomic_init(&io->ticks, 0); atomic_init(&io->invalid, false);
    atomic_init(&io->native_frames, 0);
    atomic_init(&io->graph_frames, 0); atomic_init(&io->output_epoch, 0);
    atomic_init(&io->native_quantum, 0); atomic_init(&io->graph_quantum, 0);
    io->uid = strdup(uid);
    io->scratch = calloc((size_t)TN_DEVICE_MAX_FRAMES * channels, sizeof(float));
    if (!io->uid || !io->scratch || !tn_pcm_ring_init(&io->ring, channels, 16384) ||
        !tn_pcm_ring_init(&io->graph_ring, channels, 16384)) { tn_device_io_destroy(io); return NULL; }
    return io;
}
uint32_t tn_device_buffer_size(uint32_t device) {
    UInt32 size = 0;
    return get(device, kAudioDevicePropertyBufferFrameSize, kAudioObjectPropertyScopeGlobal,
        sizeof(size), &size) ? size : 0;
}
/* Snapshot native facts only. Rust validates requests and owns write/readback
 * policy. Missing range/settable data still permits an unchanged client size. */
bool tn_device_io_buffer_info(TNDeviceIO *io, TNDeviceBufferInfo *info) {
    if (!io || !info) return false;
    memset(info, 0, sizeof(*info));
    TNDeviceIO selected = {.uid = io->uid, .input = io->input, .group = io->group, .channels = io->channels};
    if (!resolve(&selected)) return false;
    info->device = selected.device;
    info->frames = tn_device_buffer_size(selected.device);
    AudioValueRange range = {0};
    if (get(selected.device, kAudioDevicePropertyBufferFrameSizeRange, kAudioObjectPropertyScopeGlobal, sizeof(range), &range)) {
        info->minimum = range.mMinimum; info->maximum = range.mMaximum;
    }
    AudioObjectPropertyAddress address = {kAudioDevicePropertyBufferFrameSize,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    Boolean settable = false;
    if (AudioObjectIsPropertySettable(selected.device, &address, &settable) == noErr)
        info->settable = settable ? 1 : 0;
    return info->frames != 0;
}
bool tn_device_buffer_write(uint32_t device, uint32_t frames) {
    AudioObjectPropertyAddress address = {kAudioDevicePropertyBufferFrameSize,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    return AudioObjectSetPropertyData(device, &address, 0, NULL, sizeof(frames), &frames) == noErr;
}
TNDeviceIO *tn_device_io_create_group(TNAggregateGroup *group, const char *uid,
    bool input, uint32_t channels, uint32_t rate) {
    TNDeviceIO *io = tn_device_io_create(uid, input, channels, rate);
    if (io) io->group = group;
    return io;
}
bool tn_device_io_connect(TNDeviceIO *io) {
    return tn_device_io_connect_status(io, 0) == 0;
}
int32_t tn_device_io_connect_status(TNDeviceIO *io, uint32_t expected_device) {
    uint32_t native_rate = 0;
    if (io->proc) return -1;
    if (!resolve(io)) return -2;
    if (!native_format(io, &native_rate)) return -3;
    if (expected_device && io->device != expected_device) return -5;
    /* Keep hardware unchanged. The worker bridges this actual stream rate to
     * the graph rate; control reconnects if the native format later changes. */
    io->rate = native_rate;
    io->prime_output = true;
    /* Previous IOProc is drained before a reconnect; preserve the discontinuity
     * counter but do not compare timestamps from two registration epochs. */
    io->clock.primed = false;
    atomic_store_explicit(&io->invalid, false, memory_order_release);
    OSStatus status = AudioDeviceCreateIOProcID(io->device, process, io, &io->proc);
    if (status != noErr) return status;
    status = AudioDeviceStart(io->device, io->proc);
    if (status != noErr) { tn_device_io_disconnect(io); return status; }
    return 0;
}
bool tn_device_io_disconnect(TNDeviceIO *io) {
    if (io->proc) {
        /* Latch silence even if a vendor refuses teardown. Successful destroy
         * unregisters/drains; a deleted object no longer owns an IOProc. Other
         * errors retain the original ID and context for the owner's retry. */
        atomic_store_explicit(&io->invalid, true, memory_order_release);
        AudioDeviceStop(io->device, io->proc);
        OSStatus status = AudioDeviceDestroyIOProcID(io->device, io->proc);
        if (status != noErr && status != kAudioHardwareBadDeviceError &&
            status != kAudioHardwareBadObjectError) return false;
        io->proc = NULL;
    }
    return true;
}
bool tn_device_io_destroy(TNDeviceIO *io) {
    if (!io) return true;
    if (!tn_device_io_disconnect(io)) return false;
    tn_pcm_ring_clear(&io->ring);
    tn_pcm_ring_clear(&io->graph_ring);
    free(io->scratch); free(io->uid); free(io);
    return true;
}
void tn_device_io_render_hook(TNDeviceIO *io, void *context, TNRenderHook hook) {
    io->render_context = context; io->render_hook = hook;
}
bool tn_device_io_direct_binding(TNDeviceIO *io, uint32_t *device, uint32_t *offset, uint32_t *total) {
    /* Resolve immutable selection on control, never read mutable connection
     * fields while a device worker may reconnect. */
    TNDeviceIO selected = {.uid = io->uid, .input = io->input, .group = io->group, .channels = io->channels};
    if (!resolve(&selected)) return false;
    *device = selected.device; *offset = selected.channel_offset;
    *total = selected.total_channels;
    return true;
}
bool tn_device_io_alive(TNDeviceIO *io) {
    UInt32 alive = 0;
    uint32_t rate = 0;
    if (io->group && !tn_aggregate_group_ready(io->group)) return false;
    if (!io->uid[0]) {
        AudioDeviceID selected = kAudioObjectUnknown;
        /* Default selectors follow the system's current endpoint even when the
         * old hardware remains alive. Fixed UID selections never migrate. */
        if (!get(kAudioObjectSystemObject, io->input ? kAudioHardwarePropertyDefaultInputDevice : kAudioHardwarePropertyDefaultOutputDevice,
                 kAudioObjectPropertyScopeGlobal, sizeof(selected), &selected) ||
            selected != io->device) return false;
    }
    return io->proc && !atomic_load_explicit(&io->invalid, memory_order_acquire) &&
        get(io->device, kAudioDevicePropertyDeviceIsAlive, kAudioObjectPropertyScopeGlobal, sizeof(alive), &alive) && alive && native_format(io, &rate) && rate == io->rate;
}
uint64_t tn_device_io_ticks(TNDeviceIO *io) { return atomic_load_explicit(&io->ticks, memory_order_acquire); }
uint64_t tn_device_io_native_frames(TNDeviceIO *io) { return atomic_load_explicit(&io->native_frames, memory_order_acquire); }
uint32_t tn_device_io_native_rate(TNDeviceIO *io) { return io->rate; }
uint32_t tn_device_io_clock_device(TNDeviceIO *io) {
    if (io->group) return io->proc && tn_aggregate_group_ready(io->group) ? io->device : 0;
    UInt32 transport = 0;
    return io->proc && get(io->device, kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal, sizeof(transport), &transport) &&
        transport != kAudioDeviceTransportTypeUnknown &&
        transport != kAudioDeviceTransportTypeAggregate ? io->device : 0;
}
uint64_t tn_device_io_clock_discontinuities(TNDeviceIO *io) {
    return atomic_load(&io->clock.discontinuities);
}
uint64_t tn_device_io_graph_frames(TNDeviceIO *io) { return atomic_load_explicit(&io->graph_frames, memory_order_acquire); }
uint64_t tn_device_io_overflows(TNDeviceIO *io) {
    return atomic_load_explicit(&io->ring.overflows, memory_order_relaxed) +
        atomic_load_explicit(&io->graph_ring.overflows, memory_order_relaxed);
}
uint32_t tn_device_io_quantum(TNDeviceIO *io, bool graph) {
    return atomic_load_explicit(graph ? &io->graph_quantum : &io->native_quantum, memory_order_relaxed);
}
void tn_device_io_output_epoch(TNDeviceIO *io, uint64_t epoch) { atomic_store_explicit(&io->output_epoch, epoch, memory_order_release); }
uint32_t tn_device_io_queued(TNDeviceIO *io, bool graph) {
    TNPcmRing *ring = graph ? &io->graph_ring : &io->ring;
    uint64_t written = atomic_load_explicit(&ring->written, memory_order_acquire);
    uint64_t read = atomic_load_explicit(&ring->read, memory_order_acquire);
    uint64_t available = written - read;
    return available <= ring->capacity ? (uint32_t)available : 0;
}
void tn_device_io_worker_discard(TNDeviceIO *io) {
    /* Only the worker's consumer cursor is moved. Never reset the graph or
     * callback cursor while the opposite producer may publish concurrently. */
    tn_pcm_ring_discard(io->input ? &io->ring : &io->graph_ring);
}
uint32_t tn_device_io_graph_read(TNDeviceIO *io, float *samples, uint32_t frames, uint64_t epoch) {
    if (!io->input || frames > TN_DEVICE_MAX_FRAMES) return 0;
    atomic_store_explicit(&io->graph_quantum, frames, memory_order_relaxed);
    atomic_fetch_add_explicit(&io->graph_frames, frames, memory_order_release);
    if (io->graph_read_epoch != epoch) { tn_pcm_ring_discard(&io->graph_ring); io->graph_read_epoch = epoch; }
    return tn_pcm_ring_read_available(&io->graph_ring, samples, frames);
}
uint32_t tn_device_io_graph_write(TNDeviceIO *io, const float *samples, uint32_t frames) {
    if (io->input || frames > TN_DEVICE_MAX_FRAMES) return 0;
    atomic_store_explicit(&io->graph_quantum, frames, memory_order_relaxed);
    atomic_fetch_add_explicit(&io->graph_frames, frames, memory_order_release);
    return tn_pcm_ring_write(&io->graph_ring, samples, frames);
}
uint32_t tn_device_io_worker_read(TNDeviceIO *io, float *samples, uint32_t frames) {
    /* SRC owns priming. Do not add two native quantums of latency in its source
     * queue or fabricate a valid silent block while waiting for real frames. */
    TNPcmRing *ring = io->input ? &io->ring : &io->graph_ring;
    if (!frames || frames > TN_DEVICE_MAX_FRAMES) return 0;
    return tn_pcm_ring_read_available(ring, samples, frames);
}
uint32_t tn_device_io_worker_write(TNDeviceIO *io, const float *samples, uint32_t frames) {
    return frames <= TN_DEVICE_MAX_FRAMES ? tn_pcm_ring_write(io->input ? &io->graph_ring : &io->ring, samples, frames) : 0;
}
uint32_t tn_device_io_read(TNDeviceIO *io, float *samples, uint32_t frames, uint64_t epoch) {
    if (!io->input || frames > TN_DEVICE_MAX_FRAMES) return 0;
    if (io->read_epoch != epoch) { tn_pcm_ring_discard(&io->ring); io->read_epoch = epoch; }
    return tn_pcm_ring_read(&io->ring, samples, frames);
}
uint32_t tn_device_io_write(TNDeviceIO *io, const float *samples, uint32_t frames) {
    return !io->input && frames <= TN_DEVICE_MAX_FRAMES ? tn_pcm_ring_write(&io->ring, samples, frames) : 0;
}
