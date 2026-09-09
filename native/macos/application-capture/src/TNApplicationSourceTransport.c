#include "TNApplicationSourceTransport.h"

#include "TNEngineControl.h"
#include "TNHALControlProtocol.h"
#include "TNSharedAudioBuffer.h"
#include "TNApplicationClock.h"

#include <errno.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <unistd.h>
#include <xpc/xpc.h>

struct TNApplicationSourceTransport {
    int descriptor;
    void *memory;
    size_t mapped_size;
    TNSharedAudioBuffer *buffer;
    uint64_t next_frame;
    uint64_t owned_epoch;
    uint64_t owned_lease;
    TNApplicationClock *clock;
};

static void tn_application_source_set_error(
    TNApplicationSourceTransportError *error,
    TNApplicationSourceTransportErrorCode code,
    int32_t system_error) {
    if (error != NULL) {
        *error = (TNApplicationSourceTransportError){
            .code = code,
            .system_error = system_error,
        };
    }
}

TNApplicationSourceTransport *tn_application_source_transport_connect(
    const char *source_id,
    uint32_t capacity_frames,
    uint32_t channels,
    TNApplicationSourceTransportError *error) {
    tn_application_source_set_error(
        error,
        TN_APPLICATION_SOURCE_TRANSPORT_OK,
        0);
    xpc_object_t request = tn_engine_control_create_clocked_source_request(
        source_id,
        capacity_frames,
        channels);
    if (request == NULL) {
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_INVALID_ARGUMENT,
            0);
        return NULL;
    }
    xpc_connection_t connection = xpc_connection_create_mach_service(
        TN_HAL_CONTROL_SERVICE_NAME,
        NULL,
        0U);
    if (connection == NULL) {
        xpc_release(request);
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_ENGINE_UNAVAILABLE,
            0);
        return NULL;
    }
    xpc_connection_set_event_handler(connection, ^(xpc_object_t event) {
        (void)event;
    });
    xpc_connection_activate(connection);
    xpc_object_t response = xpc_connection_send_message_with_reply_sync(
        connection,
        request);
    xpc_release(request);
    xpc_connection_cancel(connection);
    xpc_release(connection);
    if (response == NULL || xpc_get_type(response) == XPC_TYPE_ERROR) {
        if (response != NULL) {
            xpc_release(response);
        }
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_ENGINE_UNAVAILABLE,
            0);
        return NULL;
    }
    TNEngineSourceAttachment attachment = {
        .descriptor = -1,
        .mapped_size = 0U,
    };
    const bool parsed = tn_engine_control_parse_source_attachment(
        response,
        &attachment);
    const uint64_t producer_lease = attachment.producer_lease;
    const uint64_t source_epoch = attachment.source_epoch;
    xpc_release(response);
    if (!parsed || producer_lease == 0U || source_epoch == 0U) {
        tn_engine_control_source_attachment_close(&attachment);
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_INVALID_ATTACHMENT,
            0);
        return NULL;
    }
    void *const memory = mmap(
        NULL,
        attachment.mapped_size,
        PROT_READ | PROT_WRITE,
        MAP_SHARED,
        attachment.descriptor,
        0);
    if (memory == MAP_FAILED) {
        const int32_t mapping_error = errno;
        tn_engine_control_source_attachment_close(&attachment);
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_MAPPING_FAILED,
            mapping_error);
        return NULL;
    }
    TNSharedAudioBuffer *const buffer = tn_shared_audio_buffer_attach(
        memory,
        attachment.mapped_size);
    TNApplicationClock *const clock = tn_application_clock_attach(
        memory, attachment.mapped_size, capacity_frames, channels);
    if (buffer == NULL ||
        clock == NULL ||
        atomic_load_explicit(&clock->producer_lease, memory_order_acquire) != producer_lease ||
        tn_shared_audio_buffer_stats(buffer).epoch != source_epoch ||
        tn_shared_audio_buffer_channels(buffer) != channels ||
        tn_shared_audio_buffer_capacity(buffer) != capacity_frames ||
        !tn_shared_audio_buffer_is_lock_free(buffer)) {
        (void)munmap(memory, attachment.mapped_size);
        tn_engine_control_source_attachment_close(&attachment);
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_INVALID_ATTACHMENT,
            0);
        return NULL;
    }
    TNApplicationSourceTransport *const transport = calloc(1U, sizeof(*transport));
    if (transport == NULL) {
        (void)munmap(memory, attachment.mapped_size);
        tn_engine_control_source_attachment_close(&attachment);
        tn_application_source_set_error(
            error,
            TN_APPLICATION_SOURCE_TRANSPORT_MAPPING_FAILED,
            ENOMEM);
        return NULL;
    }
    transport->descriptor = attachment.descriptor;
    transport->memory = memory;
    transport->mapped_size = attachment.mapped_size;
    transport->buffer = buffer;
    transport->next_frame = 0U;
    transport->clock = clock;
    transport->owned_epoch = source_epoch;
    transport->owned_lease = producer_lease;
    return transport;
}

bool tn_application_source_transport_clock(
    TNApplicationSourceTransport *transport, TNApplicationSourceClockSnapshot *snapshot) {
    if (transport == NULL || snapshot == NULL) return false;
    TNApplicationClock *const clock = transport->clock;
    /* Bounded seqlock read: a racing consumer causes a retry on the next pump tick,
     * never a spin in the audio callback or a partially observed feedback tuple. */
    const uint64_t sequence = atomic_load_explicit(&clock->consumer_sequence, memory_order_acquire);
    const uint64_t format = atomic_load_explicit(&clock->format_word, memory_order_acquire);
    const uint64_t position = atomic_load_explicit(&clock->consumer_position, memory_order_acquire);
    const uint64_t ticks = atomic_load_explicit(&clock->consumer_ticks, memory_order_acquire);
    const uint32_t running = atomic_load_explicit(&clock->consumer_running, memory_order_acquire);
    const uint64_t epoch = tn_shared_audio_buffer_stats(transport->buffer).epoch;
    if ((sequence & 1U) != 0U || sequence != atomic_load_explicit(&clock->consumer_sequence, memory_order_acquire) ||
        format != atomic_load_explicit(&clock->format_word, memory_order_acquire) || epoch != transport->owned_epoch ||
        atomic_load_explicit(&clock->producer_lease, memory_order_acquire) != transport->owned_lease) return false;
    *snapshot = (TNApplicationSourceClockSnapshot){
        .format_word = format, .epoch = epoch, .produced_frames = transport->next_frame,
        .consumed_frames = (position >> 48U) == epoch ? position & TN_APPLICATION_CLOCK_FRAME_MASK : 0U,
        .consumer_ticks = ticks, .consumer_running = (position >> 48U) == epoch ? running : 0U,
        .clock_master = atomic_load_explicit(&clock->clock_master, memory_order_acquire),
    };
    return true;
}

bool tn_application_source_transport_reset(
    TNApplicationSourceTransport *transport, uint64_t format_word, uint32_t native_sample_rate) {
    if (transport == NULL || format_word == 0U || native_sample_rate < 8000U || native_sample_rate > 192000U ||
        !tn_application_clock_try_producer(transport->clock)) return false;
    if (
        tn_shared_audio_buffer_stats(transport->buffer).epoch != transport->owned_epoch ||
        atomic_load_explicit(&transport->clock->producer_lease, memory_order_acquire) != transport->owned_lease ||
        atomic_load_explicit(&transport->clock->format_word, memory_order_acquire) != format_word) {
        tn_application_clock_release_producer(transport->clock);
        return false;
    }
    /* Only the attached producer begins stream discontinuities. The consumer owns
     * its read position and notices the new epoch without either side moving the other's cursor. */
    atomic_store_explicit(&transport->clock->acknowledged_format, 0U, memory_order_release);
    atomic_store_explicit(&transport->clock->native_running, 0U, memory_order_release);
    const uint64_t next_epoch = tn_shared_audio_buffer_try_begin_epoch(transport->buffer);
    if (next_epoch == 0U) {
        /* Failed rearm never grants ownership or acknowledges a new format.
         * Retry on contention; exhausted storage needs a control-plane reattach. */
        tn_application_clock_release_producer(transport->clock);
        return false;
    }
    transport->owned_epoch = next_epoch;
    transport->next_frame = 0U;
    atomic_store_explicit(&transport->clock->native_sample_rate, native_sample_rate, memory_order_release);
    atomic_store_explicit(&transport->clock->acknowledged_format, format_word, memory_order_release);
    tn_application_clock_release_producer(transport->clock);
    return true;
}

void tn_application_source_transport_publish_native(
    TNApplicationSourceTransport *transport, uint64_t native_frames, bool running) {
    if (transport == NULL || !tn_application_clock_try_producer(transport->clock)) return;
    if (tn_shared_audio_buffer_stats(transport->buffer).epoch == transport->owned_epoch &&
        atomic_load_explicit(&transport->clock->producer_lease, memory_order_acquire) == transport->owned_lease) {
        atomic_store_explicit(&transport->clock->native_frames, native_frames, memory_order_release);
        atomic_store_explicit(&transport->clock->native_running, running ? 1U : 0U, memory_order_release);
    }
    tn_application_clock_release_producer(transport->clock);
}

bool tn_application_source_transport_write(
    TNApplicationSourceTransport *transport,
    const float *interleaved_samples,
    uint32_t frame_count) {
    if (transport == NULL || !tn_application_clock_try_producer(transport->clock)) return false;
    if (
        tn_shared_audio_buffer_stats(transport->buffer).epoch != transport->owned_epoch ||
        atomic_load_explicit(&transport->clock->producer_lease, memory_order_acquire) != transport->owned_lease ||
        atomic_load_explicit(&transport->clock->acknowledged_format, memory_order_acquire) == 0U ||
        atomic_load_explicit(&transport->clock->acknowledged_format, memory_order_acquire) !=
            atomic_load_explicit(&transport->clock->format_word, memory_order_acquire)) {
        tn_application_clock_release_producer(transport->clock);
        return false;
    }
    const bool written = tn_shared_audio_buffer_write(
        transport->buffer,
        transport->owned_epoch,
        transport->next_frame,
        interleaved_samples,
        frame_count);
    if (written) {
        transport->next_frame += frame_count;
    }
    tn_application_clock_release_producer(transport->clock);
    return written;
}

void tn_application_source_transport_destroy(
    TNApplicationSourceTransport *transport) {
    if (transport == NULL) {
        return;
    }
    tn_application_source_transport_publish_native(transport, 0U, false);
    if (transport->memory != NULL) {
        (void)munmap(transport->memory, transport->mapped_size);
    }
    if (transport->descriptor >= 0) {
        (void)close(transport->descriptor);
    }
    free(transport);
}
