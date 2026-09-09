#include "TNEndpointTransport.h"

#include <CoreAudio/CoreAudio.h>

#include <stdatomic.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static const CFStringRef TN_DEVICE_UID =
    CFSTR("io.tonenari.audio.endpoint.default");
static const Float64 TN_SAMPLE_RATE = 48000.0;
static const uint32_t TN_CHANNELS = 2U;
static const uint64_t TN_WARMUP_FRAMES = 2048U;
static const useconds_t TN_TEST_DURATION_US = 3000000U;

typedef struct TNSmokeState {
    _Atomic uint64_t first_output_frame;
    _Atomic uint64_t compared_samples;
    _Atomic uint64_t matching_samples;
    _Atomic uint64_t nonzero_samples;
    _Atomic uint64_t invalid_callbacks;
    float expected_gain;
} TNSmokeState;

static float tn_test_sample(uint64_t frame, uint32_t channel) {
    const uint64_t code =
        (frame * UINT64_C(17) + (uint64_t)channel * UINT64_C(131)) &
        UINT64_C(1023);
    return ((float)code - 511.5F) / 2048.0F;
}

static bool tn_sample_frame(const AudioTimeStamp *timestamp,
                            uint64_t *frame) {
    if (timestamp == NULL || frame == NULL ||
        (timestamp->mFlags & kAudioTimeStampSampleTimeValid) == 0U ||
        timestamp->mSampleTime < 0.0) {
        return false;
    }
    const uint64_t converted = (uint64_t)timestamp->mSampleTime;
    if ((Float64)converted != timestamp->mSampleTime) {
        return false;
    }
    *frame = converted;
    return true;
}

static bool tn_interleaved_stereo_buffer(const AudioBufferList *buffers,
                                         uint32_t frame_count,
                                         float **samples) {
    if (buffers == NULL || samples == NULL || buffers->mNumberBuffers != 1U ||
        buffers->mBuffers[0].mNumberChannels != TN_CHANNELS ||
        buffers->mBuffers[0].mData == NULL ||
        buffers->mBuffers[0].mDataByteSize <
            frame_count * TN_CHANNELS * sizeof(float)) {
        return false;
    }
    *samples = buffers->mBuffers[0].mData;
    return true;
}

static OSStatus tn_smoke_io(AudioDeviceID device,
                            const AudioTimeStamp *now,
                            const AudioBufferList *input_data,
                            const AudioTimeStamp *input_time,
                            AudioBufferList *output_data,
                            const AudioTimeStamp *output_time,
                            void *context) {
    (void)device;
    (void)now;
    TNSmokeState *const state = context;
    uint32_t frame_count = 0U;
    if (output_data != NULL && output_data->mNumberBuffers == 1U) {
        frame_count = output_data->mBuffers[0].mDataByteSize /
                      (TN_CHANNELS * (uint32_t)sizeof(float));
    } else if (input_data != NULL && input_data->mNumberBuffers == 1U) {
        frame_count = input_data->mBuffers[0].mDataByteSize /
                      (TN_CHANNELS * (uint32_t)sizeof(float));
    }

    uint64_t output_frame = 0U;
    float *output_samples = NULL;
    if (!tn_sample_frame(output_time, &output_frame) ||
        !tn_interleaved_stereo_buffer(
            output_data, frame_count, &output_samples)) {
        atomic_fetch_add_explicit(
            &state->invalid_callbacks, UINT64_C(1), memory_order_relaxed);
        return noErr;
    }
    uint64_t expected_first = UINT64_MAX;
    (void)atomic_compare_exchange_strong_explicit(
        &state->first_output_frame,
        &expected_first,
        output_frame,
        memory_order_relaxed,
        memory_order_relaxed);
    for (uint32_t frame = 0U; frame < frame_count; ++frame) {
        for (uint32_t channel = 0U; channel < TN_CHANNELS; ++channel) {
            output_samples[(size_t)frame * TN_CHANNELS + channel] =
                tn_test_sample(output_frame + frame, channel);
        }
    }

    uint64_t input_frame = 0U;
    float *input_samples = NULL;
    if (!tn_sample_frame(input_time, &input_frame) ||
        !tn_interleaved_stereo_buffer(
            input_data, frame_count, &input_samples)) {
        atomic_fetch_add_explicit(
            &state->invalid_callbacks, UINT64_C(1), memory_order_relaxed);
        return noErr;
    }
    const uint64_t first_output = atomic_load_explicit(
        &state->first_output_frame, memory_order_relaxed);
    const uint64_t comparison_start =
        first_output + TN_ENDPOINT_ENGINE_LATENCY_FRAMES + TN_WARMUP_FRAMES;
    if (first_output == UINT64_MAX || input_frame < comparison_start) {
        return noErr;
    }

    uint64_t compared = 0U;
    uint64_t matching = 0U;
    uint64_t nonzero = 0U;
    for (uint32_t frame = 0U; frame < frame_count; ++frame) {
        const uint64_t source_frame =
            input_frame + frame - TN_ENDPOINT_ENGINE_LATENCY_FRAMES;
        for (uint32_t channel = 0U; channel < TN_CHANNELS; ++channel) {
            const float actual =
                input_samples[(size_t)frame * TN_CHANNELS + channel];
            const float expected =
                tn_test_sample(source_frame, channel) * state->expected_gain;
            ++compared;
            if (fabsf(actual - expected) <= 0.000001F) {
                ++matching;
            }
            if (actual != 0.0F) {
                ++nonzero;
            }
        }
    }
    atomic_fetch_add_explicit(
        &state->compared_samples, compared, memory_order_relaxed);
    atomic_fetch_add_explicit(
        &state->matching_samples, matching, memory_order_relaxed);
    atomic_fetch_add_explicit(
        &state->nonzero_samples, nonzero, memory_order_relaxed);
    return noErr;
}

static AudioDeviceID tn_find_device(void) {
    AudioObjectPropertyAddress address = {
        .mSelector = kAudioHardwarePropertyTranslateUIDToDevice,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain,
    };
    AudioDeviceID device = kAudioObjectUnknown;
    UInt32 size = sizeof(device);
    CFStringRef uid = TN_DEVICE_UID;
    if (AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            sizeof(uid),
            &uid,
            &size,
            &device) != noErr) {
        return kAudioObjectUnknown;
    }
    return device;
}

static bool tn_set_sample_rate(AudioDeviceID device) {
    AudioObjectPropertyAddress address = {
        .mSelector = kAudioDevicePropertyNominalSampleRate,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain,
    };
    return AudioObjectSetPropertyData(
               device,
               &address,
               0U,
               NULL,
               sizeof(TN_SAMPLE_RATE),
               &TN_SAMPLE_RATE) == noErr;
}

static bool tn_validate_latency(AudioDeviceID device) {
    AudioObjectPropertyAddress address = {
        .mSelector = kAudioDevicePropertyLatency,
        .mScope = kAudioObjectPropertyScopeInput,
        .mElement = kAudioObjectPropertyElementMain,
    };
    UInt32 latency = 0U;
    UInt32 size = sizeof(latency);
    return AudioObjectGetPropertyData(
               device, &address, 0U, NULL, &size, &latency) == noErr &&
           latency == TN_ENDPOINT_ENGINE_LATENCY_FRAMES;
}

int main(int argc, char **argv) {
    float expected_gain = 1.0F;
    if (argc == 2 && strcmp(argv[1], "--half-gain") == 0) {
        expected_gain = 0.5F;
    } else if (argc != 1) {
        fputs("usage: TNInstalledDuplexSmoke [--half-gain]\n", stderr);
        return 1;
    }
    const AudioDeviceID device = tn_find_device();
    if (device == kAudioObjectUnknown) {
        fputs("ToneNari device is not registered with Core Audio\n", stderr);
        return 2;
    }
    if (!tn_set_sample_rate(device) || !tn_validate_latency(device)) {
        fputs("ToneNari device format or latency contract is invalid\n", stderr);
        return 3;
    }

    TNSmokeState state = {0};
    atomic_init(&state.first_output_frame, UINT64_MAX);
    state.expected_gain = expected_gain;
    AudioDeviceIOProcID io_proc = NULL;
    if (AudioDeviceCreateIOProcID(
            device, tn_smoke_io, &state, &io_proc) != noErr) {
        fputs("Could not create ToneNari device IO callback\n", stderr);
        return 4;
    }
    if (AudioDeviceStart(device, io_proc) != noErr) {
        (void)AudioDeviceDestroyIOProcID(device, io_proc);
        fputs("Could not start ToneNari device IO\n", stderr);
        return 5;
    }
    (void)usleep(TN_TEST_DURATION_US);
    const OSStatus stop_result = AudioDeviceStop(device, io_proc);
    const OSStatus destroy_result =
        AudioDeviceDestroyIOProcID(device, io_proc);

    const uint64_t compared = atomic_load_explicit(
        &state.compared_samples, memory_order_relaxed);
    const uint64_t matching = atomic_load_explicit(
        &state.matching_samples, memory_order_relaxed);
    const uint64_t nonzero = atomic_load_explicit(
        &state.nonzero_samples, memory_order_relaxed);
    const uint64_t invalid = atomic_load_explicit(
        &state.invalid_callbacks, memory_order_relaxed);
    printf(
        "ToneNari duplex samples: expected_gain=%.1f compared=%llu matching=%llu nonzero=%llu invalid_callbacks=%llu\n",
        (double)expected_gain,
        (unsigned long long)compared,
        (unsigned long long)matching,
        (unsigned long long)nonzero,
        (unsigned long long)invalid);
    if (stop_result != noErr || destroy_result != noErr ||
        compared < (uint64_t)TN_SAMPLE_RATE || matching != compared ||
        nonzero != compared || invalid != 0U) {
        return 6;
    }
    puts("ToneNari installed Core Audio duplex smoke passed");
    return 0;
}
