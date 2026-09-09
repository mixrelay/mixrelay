/* Executes the production lifetime/refresh implementation with only Core Audio
 * object operations replaced. No real devices, permissions, or defaults change. */
#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <Foundation/Foundation.h>
#include <assert.h>
#include <stdio.h>
#include <dispatch/dispatch.h>

static OSStatus stop_result, unregister_result, listener_result, aggregate_result, tap_result, query_result;
static AudioStreamBasicDescription fixture_format;
static unsigned unregister_calls;
static dispatch_semaphore_t block_unregister;
static void (*during_query)(void);
static OSStatus TestStop(AudioObjectID device, AudioDeviceIOProcID proc) {
    (void)device; (void)proc; return stop_result;
}
static OSStatus TestUnregister(AudioObjectID device, AudioDeviceIOProcID proc) {
    (void)device; (void)proc; ++unregister_calls;
    if (block_unregister) dispatch_semaphore_wait(block_unregister, DISPATCH_TIME_FOREVER);
    return unregister_result;
}
static OSStatus TestRemoveListener(AudioObjectID object, const AudioObjectPropertyAddress *address,
    AudioObjectPropertyListenerProc callback, void *context) {
    (void)object; (void)address; (void)callback; (void)context; return listener_result;
}
static OSStatus TestAggregate(AudioObjectID object) { (void)object; return aggregate_result; }
static OSStatus TestTap(AudioObjectID object) { (void)object; return tap_result; }
static OSStatus TestQuery(AudioObjectID object, const AudioObjectPropertyAddress *address,
    UInt32 qualifier_size, const void *qualifier, UInt32 *size, void *data) {
    (void)object; (void)qualifier_size; (void)qualifier;
    assert(address->mSelector == kAudioTapPropertyFormat);
    assert(*size == sizeof(fixture_format));
    memcpy(data, &fixture_format, sizeof(fixture_format));
    if (during_query) during_query();
    return query_result;
}
static OSStatus TestCreateIO(AudioObjectID object, AudioDeviceIOProc proc, void *context, AudioDeviceIOProcID *id) {
    (void)object; (void)proc; (void)context; *id = (AudioDeviceIOProcID)(uintptr_t)3; return noErr;
}
static OSStatus TestStart(AudioObjectID object, AudioDeviceIOProcID proc) { (void)object; (void)proc; return noErr; }
#define AudioDeviceStop TestStop
#define AudioDeviceDestroyIOProcID TestUnregister
#define AudioObjectRemovePropertyListener TestRemoveListener
#define AudioHardwareDestroyAggregateDevice TestAggregate
#define AudioHardwareDestroyProcessTap TestTap
#define AudioObjectGetPropertyData TestQuery
#define AudioDeviceCreateIOProcID TestCreateIO
#define AudioDeviceStart TestStart
#include "../src/TNProcessTap.m"

static TNProcessTapSession *Fixture(void) {
    TNProcessTapSession *session = calloc(1, sizeof(*session));
    tn_callback_clock_init(&session->clock);
    session->tap_id = 1; session->aggregate_device_id = 2;
    session->io_proc_id = (AudioDeviceIOProcID)(uintptr_t)3;
    session->format = (AudioStreamBasicDescription){
        .mSampleRate = 48000, .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        .mBytesPerPacket = 8, .mFramesPerPacket = 1, .mBytesPerFrame = 8,
        .mChannelsPerFrame = 2, .mBitsPerChannel = 32,
    };
    fixture_format = session->format;
    session->ring = tn_audio_ring_create(256, 2);
    assert(session->ring);
    session->format_listener_registered = true;
    atomic_store(&session->running, true);
    atomic_store(&session->format_generation, 1);
    TNStoreSampleRate(session, 48000);
    return session;
}

static void Invoke(TNProcessTapSession *session, AudioBufferList *data) {
    TNProcessTapIOProc(2, NULL, data, NULL, NULL, NULL, session);
}

static TNProcessTapSession *changing_session;
static void ChangeDuringQuery(void) {
    during_query = NULL;
    TNFormatChanged(1, 0, NULL, changing_session);
}

static void BufferTimestampContinuity(void) {
    TNProcessTapSession *session = Fixture();
    float samples[128] = {0};
    AudioBufferList list = {1, {{2, sizeof(samples), samples}}};
    AudioTimeStamp time = {.mSampleTime = 0, .mHostTime = 100,
        .mFlags = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid};
    TNProcessTapIOProc(2, NULL, &list, &time, NULL, NULL, session);
    TNProcessTapStats stats = tn_process_tap_stats(session);
    assert(stats.clock_valid && stats.sample_time == 0 && stats.host_time == 100);
    assert(stats.clock_discontinuities == 0 && stats.captured_frames == 64);
    time.mSampleTime = 128; time.mHostTime = 200;
    TNProcessTapIOProc(2, NULL, &list, &time, NULL, NULL, session);
    stats = tn_process_tap_stats(session);
    assert(stats.clock_discontinuities == 1 && stats.format_generation == 1);
    TNProcessTapIOProc(2, NULL, &list, NULL, NULL, NULL, session);
    stats = tn_process_tap_stats(session);
    assert(!stats.clock_valid && stats.clock_discontinuities == 2);
    assert(TNDestroyOwnedResources(session)); free(session);
}

int main(void) {
    BufferTimestampContinuity();
    @autoreleasepool {
        /* Each failed dependency retains its exact owning identifier/storage.
         * Eventual successful unregister is the only route to ring reclamation. */
        for (unsigned iteration = 0; iteration < 1000; ++iteration) {
            TNProcessTapSession *session = Fixture();
            TNAudioRing *ring = session->ring;
            stop_result = unregister_result = -1;
            assert(tn_process_tap_stop(session).code == TN_PROCESS_TAP_CORE_AUDIO_FAILURE);
            assert(atomic_load(&session->running));
            assert(!TNDestroyOwnedResources(session));
            assert(session->ring == ring && session->io_proc_id != NULL);
            unregister_result = noErr; listener_result = -1;
            assert(!TNDestroyOwnedResources(session));
            assert(session->io_proc_id == NULL && session->format_listener_registered);
            listener_result = noErr; aggregate_result = -1;
            assert(!TNDestroyOwnedResources(session));
            assert(session->aggregate_device_id == 2 && session->ring == ring);
            aggregate_result = noErr; tap_result = -1;
            assert(!TNDestroyOwnedResources(session));
            assert(session->tap_id == 1 && session->ring == ring);
            tap_result = stop_result = noErr;
            assert(TNDestroyOwnedResources(session));
            assert(session->ring == NULL);
            free(session);
        }

        TNProcessTapSession *session = Fixture();
        float samples[] = {0.25f, -0.5f, 0.75f, -1.0f};
        AudioBufferList data = {.mNumberBuffers = 1,
            .mBuffers = {{.mNumberChannels = 2, .mDataByteSize = sizeof(samples), .mData = samples}}};
        Invoke(session, &data);
        assert(tn_process_tap_stats(session).available_frames == 2);
        TNAudioRing *ring = session->ring;
        TNFormatChanged(1, 0, NULL, session);
        Invoke(session, &data);
        assert(tn_process_tap_read(session, samples, 2) == 0);
        fixture_format.mSampleRate = 44100;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        assert(session->ring == ring && tn_process_tap_stats(session).available_frames == 0);
        assert(tn_process_tap_stats(session).sample_rate == 44100);
        assert(tn_process_tap_stats(session).format_generation == 2);
        assert(tn_process_tap_stats(session).running);
        TNFormatChanged(1, 0, NULL, session);
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        assert(tn_process_tap_stats(session).format_generation == 3);

        /* Malformed/changed ABL geometry must not be read as the old layout. */
        data.mBuffers[0].mNumberChannels = 1;
        Invoke(session, &data);
        assert(tn_process_tap_stats(session).available_frames == 0);
        fixture_format.mChannelsPerFrame = 1; fixture_format.mBytesPerFrame = 4;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_UNSUPPORTED_FORMAT);
        assert(!TNDeliveryAllowed(session) && !tn_process_tap_stats(session).running);
        assert(tn_process_tap_start(session).code == TN_PROCESS_TAP_RESOURCE_BUSY);
        assert(session->ring == ring && session->format.mChannelsPerFrame == 2);
        fixture_format = session->format;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        assert(tn_process_tap_stats(session).running);
        query_result = -1;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_CORE_AUDIO_FAILURE);
        assert(!TNDeliveryAllowed(session));
        query_result = noErr;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        assert(TNDestroyOwnedResources(session)); free(session);

        /* Clock credit includes valid frames rejected only for ring capacity,
         * but never malformed/gated buffers. Refresh drains without resetting
         * these lifetime counters, preventing invented credit across epochs. */
        session = Fixture();
        float full_ring[256 * 2] = {0};
        AudioBufferList full_data = {.mNumberBuffers = 1,
            .mBuffers = {{.mNumberChannels = 2, .mDataByteSize = sizeof(full_ring), .mData = full_ring}}};
        Invoke(session, &full_data);
        Invoke(session, &full_data);
        TNProcessTapStats overflow = tn_process_tap_stats(session);
        assert(overflow.captured_frames == 256 && overflow.dropped_frames == 256);
        assert(overflow.captured_frames + overflow.dropped_frames == 512);
        full_data.mBuffers[0].mData = NULL;
        Invoke(session, &full_data);
        full_data.mBuffers[0].mData = full_ring;
        full_data.mBuffers[0].mNumberChannels = 1;
        Invoke(session, &full_data);
        full_data.mBuffers[0].mNumberChannels = 2;
        TNFormatChanged(1, 0, NULL, session);
        Invoke(session, &full_data);
        overflow = tn_process_tap_stats(session);
        assert(overflow.captured_frames + overflow.dropped_frames == 512);
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        overflow = tn_process_tap_stats(session);
        assert(overflow.available_frames == 0);
        assert(overflow.captured_frames + overflow.dropped_frames == 512);
        Invoke(session, &full_data);
        overflow = tn_process_tap_stats(session);
        assert(overflow.captured_frames == 512 && overflow.dropped_frames == 256);
        assert(overflow.captured_frames + overflow.dropped_frames == 768);
        assert(TNDestroyOwnedResources(session)); free(session);

        /* A listener arriving during a query must not be acknowledged by the
         * earlier format snapshot (including the missed-listener polling path). */
        session = Fixture();
        changing_session = session;
        fixture_format.mSampleRate = 44100;
        during_query = ChangeDuringQuery;
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_RESOURCE_BUSY);
        assert(!TNDeliveryAllowed(session));
        assert(tn_process_tap_refresh_format(session).code == TN_PROCESS_TAP_OK);
        assert(TNDeliveryAllowed(session) && tn_process_tap_stats(session).running);
        assert(TNDestroyOwnedResources(session)); free(session);

        session = Fixture();
        atomic_store(&session->active_callbacks, 1);
        assert(!TNDestroyOwnedResources(session));
        assert(session->ring != NULL);
        atomic_store(&session->active_callbacks, 0);
        atomic_store(&session->active_listeners, 1);
        assert(!TNDestroyOwnedResources(session));
        assert(session->ring != NULL);
        atomic_store(&session->active_listeners, 0);
        assert(TNDestroyOwnedResources(session)); free(session);

        /* Destroy returns even when the native unregister call is blocked.
         * The queued owner keeps memory until the mock native call can finish. */
        session = Fixture();
        block_unregister = dispatch_semaphore_create(0);
        tn_process_tap_destroy(session);
        assert(atomic_load(&session->retiring));
        assert(!TNDeliveryAllowed(session));
        dispatch_semaphore_signal(block_unregister);
        dispatch_sync(TNCleanupQueue(), ^{});
        block_unregister = nil;

        /* A transient native failure is actually retried by the owned worker,
         * not merely retained forever after a failed destructor attempt. */
        session = Fixture();
        session->owns_admission = true;
        atomic_fetch_add(&TNOwnedSessions, 1);
        unregister_result = -1;
        tn_process_tap_destroy(session);
        dispatch_sync(TNCleanupQueue(), ^{
            assert(atomic_load(&TNOwnedSessions) == 1);
            unregister_result = noErr;
        });
        dispatch_semaphore_t retry_finished = dispatch_semaphore_create(0);
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1500 * NSEC_PER_MSEC), TNCleanupQueue(), ^{
            assert(atomic_load(&TNOwnedSessions) == 0);
            dispatch_semaphore_signal(retry_finished);
        });
        assert(dispatch_semaphore_wait(retry_finished,
            dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)) == 0);
        printf("PASS: 1000 teardown failure/recovery cycles; format/rate invalidation; malformed ABL; bounded async destruction (%u unregister calls)\n", unregister_calls);
    }
    return 0;
}
