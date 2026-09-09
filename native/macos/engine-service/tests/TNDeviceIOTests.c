/* Exercise the actual callback implementation against interleaved and planar
 * CoreAudio buffer layouts without opening microphones or changing devices. */
#include <CoreAudio/CoreAudio.h>
static OSStatus test_stop(AudioDeviceID device, AudioDeviceIOProcID proc);
static OSStatus test_destroy(AudioDeviceID device, AudioDeviceIOProcID proc);
static OSStatus test_property(AudioObjectID object, const AudioObjectPropertyAddress *address,
                              UInt32 qualifier_size, const void *qualifier, UInt32 *size, void *data);
static OSStatus test_property_size(AudioObjectID object, const AudioObjectPropertyAddress *address,
                                   UInt32 qualifier_size, const void *qualifier, UInt32 *size);
#define AudioDeviceStop test_stop
#define AudioDeviceDestroyIOProcID test_destroy
#define AudioObjectGetPropertyData test_property
#define AudioObjectGetPropertyDataSize test_property_size
#include "../src/TNDeviceIO.c"
#undef AudioDeviceStop
#undef AudioDeviceDestroyIOProcID
#undef AudioObjectGetPropertyData
#undef AudioObjectGetPropertyDataSize
#include <assert.h>
#include <stdio.h>

static bool teardown_fault;
static OSStatus destroy_result;
static uint32_t stop_calls, destroy_calls;
static bool default_override;
static uint32_t default_input_queries, default_output_queries;
static bool format_override;
static double stream_rate;
static bool transport_override, transport_failure;
static UInt32 transport_type;
static OSStatus test_property_size(AudioObjectID object, const AudioObjectPropertyAddress *address,
                                   UInt32 qualifier_size, const void *qualifier, UInt32 *size) {
    if (format_override && address->mSelector == kAudioDevicePropertyStreams) {
        *size = sizeof(AudioStreamID); return noErr;
    }
    return AudioObjectGetPropertyDataSize(object, address, qualifier_size, qualifier, size);
}
static OSStatus test_property(AudioObjectID object, const AudioObjectPropertyAddress *address,
                              UInt32 qualifier_size, const void *qualifier, UInt32 *size, void *data) {
    if (transport_override && address->mSelector == kAudioDevicePropertyTransportType) {
        if (transport_failure) return kAudioHardwareUnknownPropertyError;
        *(UInt32 *)data = transport_type;
        return noErr;
    }
    if (format_override && address->mSelector == kAudioDevicePropertyStreams) {
        *(AudioStreamID *)data = 234; return noErr;
    }
    if (format_override && address->mSelector == kAudioStreamPropertyVirtualFormat) {
        *(AudioStreamBasicDescription *)data = (AudioStreamBasicDescription){
            .mSampleRate = stream_rate, .mFormatID = kAudioFormatLinearPCM,
            .mFormatFlags = kAudioFormatFlagIsFloat, .mBitsPerChannel = 32,
            .mChannelsPerFrame = 2, .mBytesPerFrame = 2 * sizeof(float)};
        return noErr;
    }
    if (default_override && object == kAudioObjectSystemObject &&
        (address->mSelector == kAudioHardwarePropertyDefaultInputDevice ||
         address->mSelector == kAudioHardwarePropertyDefaultOutputDevice)) {
        assert(*size == sizeof(AudioDeviceID));
        *(AudioDeviceID *)data = 234;
        if (address->mSelector == kAudioHardwarePropertyDefaultInputDevice) ++default_input_queries;
        else ++default_output_queries;
        return noErr;
    }
    return AudioObjectGetPropertyData(object, address, qualifier_size, qualifier, size, data);
}
static OSStatus test_stop(AudioDeviceID device, AudioDeviceIOProcID proc) {
    if (!teardown_fault) return AudioDeviceStop(device, proc);
    ++stop_calls;
    return kAudioHardwareUnspecifiedError;
}
static OSStatus test_destroy(AudioDeviceID device, AudioDeviceIOProcID proc) {
    if (!teardown_fault) return AudioDeviceDestroyIOProcID(device, proc);
    ++destroy_calls;
    return destroy_result;
}

static void failed_teardown_retains_callback_context(void) {
    TNDeviceIO *io = tn_device_io_create("unit", false, 2, 48000);
    assert(io);
    io->device = 123;
    io->proc = (AudioDeviceIOProcID)(uintptr_t)1;
    teardown_fault = true;
    destroy_result = kAudioHardwareUnspecifiedError;
    assert(!tn_device_io_disconnect(io));
    assert(io->proc && atomic_load(&io->invalid));
    assert(!tn_device_io_connect(io));
    /* Even destructor failure must leave live registered context allocated. */
    assert(!tn_device_io_destroy(io));
    float output[128];
    for (uint32_t i = 0; i < 128; ++i) output[i] = 1;
    AudioBufferList list = {1, {{2, sizeof(output), output}}};
    process(123, NULL, NULL, NULL, &list, NULL, io);
    for (uint32_t i = 0; i < 128; ++i) assert(output[i] == 0);
    assert(tn_device_io_ticks(io) == 0);
    destroy_result = noErr;
    assert(tn_device_io_destroy(io));
    assert(stop_calls == 3 && destroy_calls == 3);
    /* A confirmed deleted object owns no remaining registration. */
    io = tn_device_io_create("unit", true, 2, 48000);
    io->proc = (AudioDeviceIOProcID)(uintptr_t)1;
    destroy_result = kAudioHardwareBadDeviceError;
    assert(tn_device_io_destroy(io));
    teardown_fault = false;
}

static void clock_identity_and_buffer_timestamps(void) {
    transport_override = true;
    for (uint32_t input = 0; input < 2; ++input) {
        TNDeviceIO *io = tn_device_io_create("unit", input != 0, 2, 48000);
        assert(io);
        assert(!tn_device_io_clock_device(io));
        io->device = 123;
        io->proc = (AudioDeviceIOProcID)(uintptr_t)1;
        transport_type = kAudioDeviceTransportTypeUSB;
        assert(tn_device_io_clock_device(io) == 123);
        transport_type = kAudioDeviceTransportTypeAggregate;
        assert(!tn_device_io_clock_device(io));
        transport_type = kAudioDeviceTransportTypeUnknown;
        assert(!tn_device_io_clock_device(io));
        transport_failure = true;
        assert(!tn_device_io_clock_device(io));
        transport_failure = false;
        float samples[128] = {0};
        AudioBufferList list = {1, {{2, sizeof(samples), samples}}};
        AudioTimeStamp in = {.mSampleTime = 100, .mHostTime = 1000,
            .mFlags = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid};
        AudioTimeStamp out = in;
        out.mSampleTime = 200; out.mHostTime = 2000;
        process(123, NULL, &list, &in, &list, &out, io);
        TNCallbackClockSnapshot clock;
        assert(tn_callback_clock_snapshot(&io->clock, &clock));
        assert(clock.valid && clock.sample_time == (input ? 100 : 200));
        assert(clock.host_time == (input ? 1000U : 2000U));
        in.mSampleTime += 128; out.mSampleTime += 128;
        in.mHostTime += 1000; out.mHostTime += 1000;
        process(123, NULL, &list, &in, &list, &out, io);
        assert(tn_device_io_clock_discontinuities(io) == 1);
        io->proc = NULL; /* fixture never registered a real callback */
        assert(tn_device_io_destroy(io));
    }
    transport_override = false;
}

static void default_selectors_detect_endpoint_changes(void) {
    default_override = true;
    for (uint32_t input = 0; input < 2; ++input) {
        TNDeviceIO *io = tn_device_io_create("", input != 0, 2, 48000);
        assert(io);
        io->device = 123;
        assert(!tn_device_io_alive(io));
        assert(tn_device_io_destroy(io));
    }
    assert(default_input_queries == 1 && default_output_queries == 1);
    default_override = false;
}

static void mixed_stream_subset_case(void) {
    /* Real devices may expose several interleaved streams rather than one
     * packed buffer or N mono buffers. Select four of six hardware channels. */
    TNDeviceIO *in = tn_device_io_create("unit", true, 4, 48000);
    TNDeviceIO *out = tn_device_io_create("unit", false, 4, 48000);
    assert(in && out);
    AudioBufferList *list = calloc(1, sizeof(AudioBufferList) + 2 * sizeof(AudioBuffer));
    assert(list);
    float hardware[64 * 6], graph[64 * 4];
    const uint32_t widths[] = {1, 2, 3};
    list->mNumberBuffers = 3;
    uint32_t channel = 0;
    for (uint32_t b = 0; b < 3; ++b) {
        float *stream = hardware + 64 * channel;
        list->mBuffers[b] = (AudioBuffer){widths[b], 64 * widths[b] * sizeof(float), stream};
        for (uint32_t f = 0; f < 64; ++f) for (uint32_t c = 0; c < widths[b]; ++c)
            stream[f * widths[b] + c] = (float)(channel + c + 1) / 128;
        channel += widths[b];
    }
    in->read_epoch = 1;
    for (uint32_t i = 0; i < 3; ++i) process(0, NULL, list, NULL, NULL, NULL, in);
    assert(tn_device_io_read(in, graph, 64, 1) == 64);
    for (uint32_t f = 0; f < 64; ++f) for (uint32_t c = 0; c < 4; ++c)
        assert(graph[f * 4 + c] == (float)(c + 1) / 128);
    for (uint32_t i = 0; i < 3; ++i) assert(tn_device_io_write(out, graph, 64) == 64);
    process(0, NULL, NULL, NULL, list, NULL, out);
    channel = 0;
    for (uint32_t b = 0; b < 3; ++b) {
        float *stream = list->mBuffers[b].mData;
        for (uint32_t f = 0; f < 64; ++f) for (uint32_t c = 0; c < widths[b]; ++c)
            assert(stream[f * widths[b] + c] == (channel + c < 4 ? (float)(channel + c + 1) / 128 : 0));
        channel += widths[b];
    }
    free(list);
    assert(tn_device_io_destroy(in) && tn_device_io_destroy(out));
}

static void pcm_case(uint32_t channels, bool planar) {
    TNDeviceIO *in = tn_device_io_create("unit", true, channels, 48000);
    TNDeviceIO *out = tn_device_io_create("unit", false, channels, 48000);
    assert(in && out);
    const uint32_t frames = 64;
    uint32_t count = planar ? channels : 1;
    AudioBufferList *list = calloc(1, sizeof(AudioBufferList) + (count - 1) * sizeof(AudioBuffer));
    float *source = calloc((size_t)frames * channels, sizeof(float));
    float *copy = calloc((size_t)frames * channels, sizeof(float));
    assert(list && source && copy);
    list->mNumberBuffers = count;
    for (uint32_t b = 0; b < count; ++b) {
        list->mBuffers[b] = (AudioBuffer){planar ? 1 : channels, frames * (planar ? 1 : channels) * sizeof(float), source + (planar ? b * frames : 0)};
    }
    for (uint32_t f = 0; f < frames; ++f) for (uint32_t c = 0; c < channels; ++c)
        source[planar ? c * frames + f : f * channels + c] = (float)(c + 1) / 128;
    in->read_epoch = 1;
    for (uint32_t i = 0; i < 3; ++i) process(0, NULL, list, NULL, NULL, NULL, in);
    assert(tn_device_io_read(in, copy, frames, 1) == frames);
    for (uint32_t f = 0; f < frames; ++f) for (uint32_t c = 0; c < channels; ++c)
        assert(copy[f * channels + c] == (float)(c + 1) / 128);
    /* A recovered input starts a new epoch and discards all queued old PCM. */
    assert(tn_device_io_read(in, copy, frames, 2) == 0);
    for (uint32_t i = 0; i < frames * channels; ++i) assert(copy[i] == 0);
    for (uint32_t f = 0; f < frames; ++f) for (uint32_t c = 0; c < channels; ++c)
        copy[f * channels + c] = (float)(c + 1) / 128;
    for (uint32_t i = 0; i < 3; ++i) assert(tn_device_io_write(out, copy, frames) == frames);
    process(0, NULL, NULL, NULL, list, NULL, out);
    for (uint32_t f = 0; f < frames; ++f) for (uint32_t c = 0; c < channels; ++c)
        assert(source[planar ? c * frames + f : f * channels + c] == copy[f * channels + c]);
    out->prime_output = true;
    process(0, NULL, NULL, NULL, list, NULL, out);
    for (uint32_t i = 0; i < frames * channels; ++i) assert(source[i] == 0);
    /* Invalid callback width is silenced and surfaced to the recovery owner. */
    list->mBuffers[0].mNumberChannels = 0;
    process(0, NULL, NULL, NULL, list, NULL, out);
    assert(atomic_load(&out->invalid));
    uint64_t ticks = tn_device_io_ticks(out);
    list->mBuffers[0].mNumberChannels = planar ? 1 : channels;
    for (uint32_t i = 0; i < frames * channels; ++i) source[i] = 1;
    process(0, NULL, NULL, NULL, list, NULL, out);
    assert(tn_device_io_ticks(out) == ticks);
    for (uint32_t i = 0; i < frames * channels; ++i) assert(source[i] == 0);
    free(source); free(copy); free(list);
    tn_device_io_destroy(in); tn_device_io_destroy(out);
}
static void native_rate_is_observed_not_forced(void) {
    format_override = true;
    TNDeviceIO *io = tn_device_io_create("unit", true, 2, 48000);
    uint32_t rate = 0;
    stream_rate = 44100;
    assert(native_format(io, &rate) && rate == 44100);
    stream_rate = 96000;
    assert(native_format(io, &rate) && rate == 96000);
    stream_rate = NAN;
    assert(!native_format(io, &rate));
    stream_rate = 48000.5;
    assert(!native_format(io, &rate));
    stream_rate = 0;
    assert(!native_format(io, &rate));
    assert(tn_device_io_destroy(io));
    format_override = false;
}

static void three_owner_queues_and_epoch_case(uint32_t channels) {
    const uint32_t frames = 64;
    TNDeviceIO *input = tn_device_io_create("unit", true, channels, 44100);
    TNDeviceIO *output = tn_device_io_create("unit", false, channels, 96000);
    float source[64 * 64], scratch[64 * 64], destination[64 * 64];
    for (uint32_t i = 0; i < frames * channels; ++i) source[i] = (float)(i % channels + 1) / 128;
    AudioBufferList buffers = {1, {{channels, frames * channels * sizeof(float), source}}};
    /* Native callback publishes only native FIFO. Worker does not consume graph
     * cursor, and graph never consumes native FIFO. No synthetic priming data. */
    process(0, NULL, &buffers, NULL, NULL, NULL, input);
    assert(tn_device_io_native_frames(input) == frames);
    assert(tn_device_io_queued(input, true) == 0);
    assert(tn_device_io_worker_read(input, scratch, frames) == frames);
    assert(!memcmp(source, scratch, frames * channels * sizeof(float)));
    assert(tn_device_io_worker_read(input, scratch, frames) == 0);
    /* Graph observes the new epoch before the worker produces fresh SRC PCM. */
    assert(tn_device_io_graph_read(input, destination, frames, 1) == 0);
    for (uint32_t i = 0; i < 3; ++i) assert(tn_device_io_worker_write(input, source, frames) == frames);
    assert(tn_device_io_graph_read(input, destination, frames, 1) == frames);
    assert(!memcmp(source, destination, frames * channels * sizeof(float)));
    assert(tn_device_io_graph_read(input, destination, frames, 2) == 0);
    assert(tn_device_io_queued(input, true) == 0);
    assert(tn_device_io_graph_frames(input) == frames * 3);
    assert(tn_device_io_graph_write(output, source, frames) == frames);
    assert(tn_device_io_queued(output, false) == 0);
    assert(tn_device_io_worker_read(output, scratch, frames) == frames);
    assert(!memcmp(source, scratch, frames * channels * sizeof(float)));
    for (uint32_t i = 0; i < 3; ++i) assert(tn_device_io_worker_write(output, scratch, frames) == frames);
    buffers.mBuffers[0].mData = destination;
    process(0, NULL, NULL, NULL, &buffers, NULL, output);
    assert(!memcmp(source, destination, frames * channels * sizeof(float)));
    tn_device_io_output_epoch(output, 1);
    process(0, NULL, NULL, NULL, &buffers, NULL, output);
    for (uint32_t i = 0; i < frames * channels; ++i) assert(destination[i] == 0);
    assert(tn_device_io_queued(output, false) == 0);
    assert(tn_device_io_quantum(output, false) == frames);
    assert(tn_device_io_quantum(output, true) == frames);
    assert(tn_device_io_native_frames(output) == frames * 2);
    assert(tn_device_io_graph_write(output, source, frames) == frames);
    tn_device_io_worker_discard(output);
    assert(tn_device_io_queued(output, true) == 0);
    assert(tn_device_io_destroy(input) && tn_device_io_destroy(output));
}

static void aggregate_channel_slice(uint32_t offset, uint32_t channels, bool input) {
    TNDeviceIO *io = tn_device_io_create("slice", input, channels, 48000);
    assert(io && offset + channels <= 128);
    // A non-dereferenced group marker enables the callback's aggregate layout
    // contract. Native group ownership/readback is covered by the live probe.
    io->group = (TNAggregateGroup *)(uintptr_t)1;
    io->channel_offset = offset; io->total_channels = 128;
    float first[64 * 4], second[64 * 4], selected[64 * 4];
    for (unsigned frame = 0; frame < 4; ++frame) {
        for (unsigned c = 0; c < 64; ++c) {
            first[frame * 64 + c] = (float)(frame * 128 + c);
            second[frame * 64 + c] = (float)(frame * 128 + c + 64);
        }
        for (unsigned c = 0; c < channels; ++c) selected[frame * channels + c] = (float)(frame * 128 + offset + c);
    }
    struct { UInt32 count; AudioBuffer buffers[2]; } list = {2, {{64, sizeof(first), first}, {64, sizeof(second), second}}};
    if (!input) assert(tn_pcm_ring_write(&io->ring, selected, 4) == 4);
    process(0, NULL, input ? (AudioBufferList *)&list : NULL, NULL,
        input ? NULL : (AudioBufferList *)&list, NULL, io);
    assert(!atomic_load(&io->invalid));
    if (input) {
        float read[64 * 4];
        assert(tn_device_io_worker_read(io, read, 4) == 4);
        assert(!memcmp(read, selected, 4 * channels * sizeof(float)));
    } else for (unsigned frame = 0; frame < 4; ++frame) for (unsigned c = 0; c < 128; ++c) {
        float value = c < 64 ? first[frame * 64 + c] : second[frame * 64 + c - 64];
        float expected = c >= offset && c < offset + channels ? (float)(frame * 128 + c) : 0;
        assert(value == expected);
    }
    // Member disappearance must mute, not route this slice into a new device.
    list.count = 1;
    process(0, NULL, input ? (AudioBufferList *)&list : NULL, NULL,
        input ? NULL : (AudioBufferList *)&list, NULL, io);
    assert(atomic_load(&io->invalid));
    assert(tn_device_io_destroy(io));
}

int main(void) {
    const uint32_t offsets[] = {1, 13, 33, 64};
    const uint32_t slices[] = {1, 2, 7, 32, 64};
    for (unsigned offset = 0; offset < 4; ++offset) for (unsigned slice = 0; slice < 5; ++slice) {
        aggregate_channel_slice(offsets[offset], slices[slice], true);
        aggregate_channel_slice(offsets[offset], slices[slice], false);
    }
    native_rate_is_observed_not_forced();
    for (uint32_t channels = 1; channels <= 64; ++channels) three_owner_queues_and_epoch_case(channels);
    failed_teardown_retains_callback_context();
    clock_identity_and_buffer_timestamps();
    default_selectors_detect_endpoint_changes();
    mixed_stream_subset_case();
    const uint32_t widths[] = {1, 2, 8, 16, 32, 64};
    for (uint32_t repeat = 0; repeat < 32; ++repeat)
        for (uint32_t i = 0; i < sizeof(widths)/sizeof(widths[0]); ++i) {
            pcm_case(widths[i], false); pcm_case(widths[i], true);
        }
    assert(!tn_device_io_create("unit", true, 0, 48000));
    assert(!tn_device_io_create("unit", true, 65, 48000));
    TNDeviceIO *missing = tn_device_io_create("io.tonenari.test.nonexistent", true, 2, 48000);
    assert(missing && !tn_device_io_connect(missing));
    assert(tn_device_io_connect_status(missing, 0) == -2);
    tn_device_io_destroy(missing);
    puts("PASS CoreAudio native rate validation, 64 three-owner SPSC/epoch cases, teardown faults, default selector changes, mixed-stream subsets and 384 callback/epoch/layout ownership cases");
}
