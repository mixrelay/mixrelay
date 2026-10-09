#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <assert.h>
#include <pipewire/pipewire.h>

static struct pw_buffer *next_buffer;
static unsigned returned_buffers;
static struct pw_buffer *test_dequeue(struct pw_stream *stream) {
    (void)stream;
    return next_buffer;
}
static int test_queue(struct pw_stream *stream, struct pw_buffer *buffer) {
    (void)stream;
    assert(buffer == next_buffer);
    returned_buffers++;
    return 0;
}
#define pw_stream_dequeue_buffer test_dequeue
#define pw_stream_queue_buffer test_queue
#include "../MRPipeWire.c"
#undef pw_stream_dequeue_buffer
#undef pw_stream_queue_buffer

static void test_output(uint32_t encoding) {
    MRPWStream stream = { .native_output = true, .channels = 2 };
    initialize_transfer_observations(&stream);
    atomic_init(&stream.position, NULL);
    atomic_init(&stream.clock_snapshot, 0);
    atomic_init(&stream.quantum_snapshot, 0);
    atomic_init(&stream.format_snapshot, (uint64_t)encoding << 16);
    atomic_init(&stream.format_valid, true);
    assert(mr_pcm_ring_init(&stream.ring, 2, 8));
    const float samples[8] = {0.25f, -0.25f, 0.5f, -0.5f, 0.75f, -0.75f, 0, 0};
    assert(mr_pcm_ring_write(&stream.ring, samples, 4) == 4);
    union { float aligned[6]; unsigned char bytes[24]; } storage = {0};
    const uint32_t stride = 2 * sample_bytes(encoding);
    struct spa_chunk chunk = {0};
    struct spa_data data = { .data = storage.bytes, .maxsize = 3 * stride, .chunk = &chunk };
    struct spa_buffer spa_buffer = { .n_datas = 1, .datas = &data };
    struct pw_buffer buffer = { .buffer = &spa_buffer, .size = 99, .requested = 2 };
    next_buffer = &buffer;
    unsigned before = returned_buffers;
    stream_process_buffer(&stream);
    assert(buffer.size == 2 && chunk.size == 2 * stride);
    assert(chunk.offset == 0 && chunk.stride == (int32_t)stride);
    assert(returned_buffers == before + 1);

    // Only two frames remain: the ring emits a whole silent three-frame block.
    buffer.requested = 100;
    memset(storage.bytes, 0xff, sizeof(storage.bytes));
    stream_process_buffer(&stream);
    assert(buffer.size == 3 && chunk.size == 3 * stride);
    for (uint32_t i = 0; i < chunk.size; ++i) assert(storage.bytes[i] == 0);
    assert(returned_buffers == before + 2);

    atomic_store(&stream.format_valid, false);
    stream_process_buffer(&stream);
    assert(buffer.size == 0 && chunk.size == 0);
    assert(returned_buffers == before + 3);
    buffer.size = 99;
    spa_buffer.n_datas = 0;
    stream_process_buffer(&stream);
    assert(buffer.size == 0 && returned_buffers == before + 4);
    next_buffer = NULL;
    stream_process_buffer(&stream);
    assert(returned_buffers == before + 4);
    mr_pcm_ring_clear(&stream.ring);
}

int main(void) {
    for (uint32_t encoding = MR_PCM_FORMAT_FLOAT32; encoding <= MR_PCM_FORMAT_SIGNED32; ++encoding)
        test_output(encoding);
    return 0;
}
