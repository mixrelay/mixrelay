#include "TNApplicationClock.h"
#include "TNEngineSourceRegistry.h"
#include <assert.h>
#include <stdlib.h>
#include <stdio.h>
#include <sys/mman.h>
/* Exercise the real producer mapping/epoch boundary without installing an XPC
 * service, creating a process tap, or touching the host's audio settings. */
#include "../../application-capture/src/TNApplicationSourceTransport.c"

static void test_clock_trailer_validation(void) {
    const size_t bytes = tn_application_clock_required_bytes(1024U, 2U);
    void *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
    assert(memory != MAP_FAILED);
    TNSharedAudioBuffer *buffer = tn_shared_audio_buffer_initialize(memory, bytes, 1024U, 2U);
    assert(buffer != NULL);
    TNApplicationClock *clock = tn_application_clock_initialize(memory, bytes, 1024U, 2U);
    assert(clock != NULL);
    assert(tn_shared_audio_buffer_attach(memory, bytes) == buffer);
    assert(tn_application_clock_attach(memory, tn_shared_audio_buffer_required_bytes(1024U, 2U), 1024U, 2U) == NULL);
    assert(tn_application_clock_attach(memory, bytes - 1U, 1024U, 2U) == NULL);
    assert(tn_application_clock_attach((char *)memory + 1U, bytes, 1024U, 2U) == NULL);
    assert(tn_application_clock_initialize(NULL, bytes, 1024U, 2U) == NULL);
    assert(tn_application_clock_required_bytes(0U, 2U) == 0U);
    clock->version = 99U;
    assert(tn_application_clock_attach(memory, bytes, 1024U, 2U) == NULL);
    clock->version = TN_APPLICATION_CLOCK_VERSION;
    clock->magic = 0U;
    assert(tn_application_clock_attach(memory, bytes, 1024U, 2U) == NULL);
    clock->magic = TN_APPLICATION_CLOCK_MAGIC;
    clock->header_bytes -= 1U;
    assert(tn_application_clock_attach(memory, bytes, 1024U, 2U) == NULL);
    clock->header_bytes = sizeof(*clock);
    assert(tn_application_clock_try_producer(clock));
    assert(!tn_application_clock_try_producer(clock));
    tn_application_clock_release_producer(clock);
    assert(tn_application_clock_try_producer(clock));
    tn_application_clock_release_producer(clock);
    assert(tn_application_clock_request_rate(clock, 0U) == 0U);
    const uint64_t first = tn_application_clock_request_rate(clock, 44100U);
    assert((uint32_t)first == 44100U);
    assert(tn_application_clock_request_rate(clock, 44100U) == first);
    assert(tn_application_clock_request_rate(clock, 96000U) != first);
    tn_application_clock_publish_consumer(clock, 7U, 900U, 128U, true);
    assert(atomic_load(&clock->consumer_position) == ((UINT64_C(7) << 48U) | 900U));
    assert(atomic_load(&clock->consumer_ticks) == 128U);
    assert(atomic_load(&clock->consumer_running) == 1U);
    assert((atomic_load(&clock->consumer_sequence) & 1U) == 0U);
    assert(munmap(memory, bytes) == 0);
}

static void test_clocked_registry_rejects_unacknowledged_or_old_rate_audio(void) {
    TNEngineSourceRegistry *registry = tn_engine_source_registry_create();
    TNEngineSourceRequest request = { .source_id = "clocked-fixture", .capacity_frames = 1024U,
        .channels = 2U, .transport_version = 2U };
    int descriptor = -1;
    size_t bytes = 0U;
    uint64_t granted_lease = 0U;
    uint64_t granted_epoch = 0U;
    assert(tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &granted_lease, &granted_epoch));
    void *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    assert(memory != MAP_FAILED);
    assert(close(descriptor) == 0); descriptor = -1;
    TNSharedAudioBuffer *buffer = tn_shared_audio_buffer_attach(memory, bytes);
    TNApplicationClock *clock = tn_application_clock_attach(memory, bytes, 1024U, 2U);
    assert(buffer != NULL && clock != NULL);
    assert(granted_lease != 0U && granted_lease == atomic_load(&clock->producer_lease));
    assert(granted_epoch == tn_shared_audio_buffer_stats(buffer).epoch);
    TNApplicationSourceTransport producer = { .descriptor = -1, .buffer = buffer, .clock = clock,
        .owned_epoch = tn_shared_audio_buffer_stats(buffer).epoch, .owned_lease = atomic_load(&clock->producer_lease) };
    float input[768U * 2U];
    float output[128U * 2U];
    for (size_t i = 0U; i < 768U * 2U; ++i) input[i] = 0.25f;
    const uint64_t token = tn_engine_source_id_token(request.source_id);
    assert(tn_shared_audio_buffer_write(buffer, granted_epoch, 0U, input, 768U));
    assert(tn_engine_source_registry_read_clocked(registry, token, output, 128U, 2U, 44100U) == 0U);
    const uint64_t format = atomic_load(&clock->format_word);
    assert((uint32_t)format == 44100U);
    assert(output[0] == 0.0f && atomic_load(&clock->consumer_running) == 0U);
    assert(tn_application_source_transport_reset(&producer, format, 48000U));
    assert(tn_application_clock_try_producer(clock));
    assert(!tn_application_source_transport_write(&producer, input, 128U));
    assert(!tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &granted_lease, &granted_epoch));
    tn_application_clock_release_producer(clock);
    assert(tn_application_source_transport_write(&producer, input, 768U));
    tn_application_source_transport_publish_native(&producer, 1200U, true);
    assert(tn_engine_source_registry_read_clocked(registry, token, output, 128U, 2U, 44100U) == 128U);
    assert(output[0] == 0.25f);
    assert((atomic_load(&clock->consumer_position) & TN_APPLICATION_CLOCK_FRAME_MASK) == 384U);
    assert(atomic_load(&clock->consumer_running) == 1U);
    TNApplicationSourceClock native_clock = {0};
    assert(tn_engine_source_registry_clock(registry, token, &native_clock));
    assert(native_clock.native_rate == 48000U && native_clock.graph_rate == 44100U);
    assert(native_clock.native_frames == 1200U && native_clock.synchronized == 1U);
    TNApplicationSourceClockSnapshot snapshot = {0};
    assert(tn_application_source_transport_clock(&producer, &snapshot));
    assert(snapshot.produced_frames == 768U && snapshot.consumed_frames == 384U);
    assert(tn_engine_source_registry_set_clock_master(registry, token, true));
    assert(tn_application_source_transport_clock(&producer, &snapshot) && snapshot.clock_master == 1U);
    assert(tn_engine_source_registry_set_clock_master(registry, token, false));
    assert(tn_application_source_transport_clock(&producer, &snapshot) && snapshot.clock_master == 0U);
    atomic_fetch_add(&clock->consumer_sequence, 1U);
    assert(!tn_application_source_transport_clock(&producer, &snapshot));
    atomic_fetch_add(&clock->consumer_sequence, 1U);
    assert(tn_engine_source_registry_read_clocked(registry, token, output, 128U, 2U, 96000U) == 0U);
    assert(atomic_load(&clock->format_word) != format);
    assert(!tn_application_source_transport_write(&producer, input, 128U));
    assert(!tn_engine_source_registry_clock(registry, token, &native_clock));
    /* Reconnect invalidates the old producer acknowledgement and stream epoch. */
    const uint64_t old_epoch = tn_shared_audio_buffer_stats(buffer).epoch;
    assert(tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &granted_lease, &granted_epoch));
    assert(tn_shared_audio_buffer_stats(buffer).epoch != old_epoch);
    assert(close(descriptor) == 0); descriptor = -1;
    assert(atomic_load(&clock->acknowledged_format) == 0U);
    assert(!tn_application_source_transport_clock(&producer, &snapshot));
    assert(!tn_application_source_transport_reset(&producer, atomic_load(&clock->format_word), 48000U));
    assert(!tn_application_source_transport_write(&producer, input, 128U));
    request.transport_version = 0U;
    /* Copying a current epoch cannot let a retired lease impersonate its owner. */
    producer.owned_epoch = tn_shared_audio_buffer_stats(buffer).epoch;
    assert(!tn_application_source_transport_reset(&producer, atomic_load(&clock->format_word), 48000U));
    assert(!tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &granted_lease, &granted_epoch));
    assert(munmap(memory, bytes) == 0);
    tn_engine_source_registry_destroy(registry);
}

static void test_invalid_source_version_never_creates_a_slot(void) {
    TNEngineSourceRegistry *registry = tn_engine_source_registry_create();
    TNEngineSourceRequest request = { .source_id = "invalid-version", .capacity_frames = 1024U, .channels = 1U };
    int descriptor = -1;
    size_t bytes = 0U;
    uint64_t lease = 0U, epoch = 0U;
    assert(!tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch));
    request.transport_version = 99U;
    assert(!tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch));
    assert(descriptor == -1 && bytes == 0U && lease == 0U && epoch == 0U);
    const uint64_t token = tn_engine_source_id_token(request.source_id);
    TNApplicationSourceClock snapshot;
    assert(!tn_engine_source_registry_clock(registry, token, &snapshot));
    assert(!tn_engine_source_registry_set_clock_master(registry, token, true));
    request.transport_version = TN_APPLICATION_CLOCK_VERSION;
    assert(tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch));
    assert(descriptor >= 0 && bytes != 0U && lease != 0U && epoch != 0U);
    assert(close(descriptor) == 0);
    tn_engine_source_registry_destroy(registry);
}

static void test_reset_exhaustion_does_not_acknowledge_format(void) {
    const size_t bytes = tn_application_clock_required_bytes(16U, 2U);
    void *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE,
                        MAP_ANON | MAP_PRIVATE, -1, 0);
    assert(memory != MAP_FAILED);
    TNSharedAudioBuffer *buffer = tn_shared_audio_buffer_initialize(memory, bytes, 16U, 2U);
    TNApplicationClock *clock = tn_application_clock_initialize(memory, bytes, 16U, 2U);
    assert(buffer != NULL && clock != NULL);
    /* Use the public API, not a private header poke: no epoch is reused. */
    uint64_t owned_epoch = tn_shared_audio_buffer_stats(buffer).epoch;
    for (uint64_t expected = owned_epoch + 1U; expected <= UINT16_MAX; ++expected) {
        owned_epoch = tn_shared_audio_buffer_try_begin_epoch(buffer);
        assert(owned_epoch == expected);
    }
    const uint64_t format = tn_application_clock_request_rate(clock, 48000U);
    atomic_store(&clock->producer_lease, 7U);
    atomic_store(&clock->acknowledged_format, format);
    atomic_store(&clock->native_running, 1U);
    atomic_store(&clock->native_sample_rate, 44100U);
    TNApplicationSourceTransport producer = {
        .descriptor = -1, .buffer = buffer, .clock = clock,
        .owned_epoch = owned_epoch, .owned_lease = 7U, .next_frame = 123U,
    };
    assert(!tn_application_source_transport_reset(&producer, format, 48000U));
    assert(producer.owned_epoch == owned_epoch && producer.next_frame == 123U);
    assert(atomic_load(&clock->acknowledged_format) == 0U);
    assert(atomic_load(&clock->native_running) == 0U);
    assert(atomic_load(&clock->native_sample_rate) == 44100U);
    assert(tn_shared_audio_buffer_requires_replacement(buffer));
    assert(tn_application_clock_try_producer(clock));
    tn_application_clock_release_producer(clock);
    const float samples[2] = {0.25f, -0.25f};
    assert(!tn_application_source_transport_write(&producer, samples, 1U));
    assert(munmap(memory, bytes) == 0);
}

static void test_exhausted_attachment_replaces_the_actual_mapping(void) {
    TNEngineSourceRegistry *registry = tn_engine_source_registry_create();
    TNEngineSourceRequest request = { .source_id = "exhausted-attachment", .capacity_frames = 1024U,
        .channels = 1U, .transport_version = TN_APPLICATION_CLOCK_VERSION };
    int descriptor = -1;
    size_t bytes = 0U;
    uint64_t lease = 0U, epoch = 0U;
    assert(tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch));
    void *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    assert(memory != MAP_FAILED);
    TNSharedAudioBuffer *buffer = tn_shared_audio_buffer_attach(memory, bytes);
    TNApplicationClock *clock = tn_application_clock_attach(memory, bytes, 1024U, 1U);
    assert(buffer != NULL && clock != NULL);
    for (uint64_t next = epoch + 1U; next <= UINT16_MAX; ++next) {
        assert(tn_shared_audio_buffer_try_begin_epoch(buffer) == next);
    }
    const uint64_t previous_lease = lease;
    assert(close(descriptor) == 0);
    descriptor = -1; bytes = 0U; lease = 0U; epoch = 0U;
    assert(tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch));
    assert(descriptor >= 0 && bytes != 0U && lease > previous_lease && epoch != 0U);
    void *replacement = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    assert(replacement != MAP_FAILED);
    assert(close(descriptor) == 0);
    TNSharedAudioBuffer *new_buffer = tn_shared_audio_buffer_attach(replacement, bytes);
    assert(new_buffer != NULL && new_buffer != buffer);
    const float pcm[1] = { 0.25f };
    assert(tn_shared_audio_buffer_write(new_buffer, epoch, 0U, pcm, 1U));
    assert(atomic_load(&clock->producer_lease) == 0U);
    assert(tn_shared_audio_buffer_requires_replacement(buffer));
    assert(tn_application_clock_try_producer(clock));
    tn_application_clock_release_producer(clock);
    assert(munmap(memory, tn_application_clock_required_bytes(1024U, 1U)) == 0);
    assert(munmap(replacement, bytes) == 0);
    tn_engine_source_registry_destroy(registry);
}

int main(void) {
    test_exhausted_attachment_replaces_the_actual_mapping();
    test_reset_exhaustion_does_not_acknowledge_format();
    test_clock_trailer_validation();
    test_clocked_registry_rejects_unacknowledged_or_old_rate_audio();
    test_invalid_source_version_never_creates_a_slot();
    puts("TNApplicationClockTests passed");
    return 0;
}
