#include "TNPipeWire.h"
#include "TNPcmRing.h"
#include <pipewire/pipewire.h>
#include <pipewire/filter.h>
#include <pipewire/data-loop.h>
#include <spa/param/audio/format-utils.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

enum { TN_RATE = 48000, TN_RING_FRAMES = 32768 };
static pthread_once_t initialized = PTHREAD_ONCE_INIT;
static void initialize(void) { pw_init(NULL, NULL); }

struct TNPWStream {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    TNPcmRing ring;
    uint32_t mode, channels, sample_rate;
    _Atomic int state;
    _Atomic bool format_valid;
    _Atomic(struct spa_io_position *) position;
    _Atomic uint64_t clock_snapshot;
    _Atomic uint64_t format_snapshot;
    _Atomic uint64_t quantum_snapshot;
};

static int set_stream_position(struct spa_loop *loop, bool async, uint32_t seq,
                               const void *data, size_t size, void *user_data) {
    (void)loop; (void)async; (void)seq;
    TNPWStream *s = user_data;
    struct spa_io_position *position = NULL;
    if (size == sizeof(position)) memcpy(&position, data, sizeof(position));
    atomic_store_explicit(&s->position, position, memory_order_release);
    atomic_store_explicit(&s->clock_snapshot, 0, memory_order_release);
    atomic_store_explicit(&s->quantum_snapshot, 0, memory_order_release);
    return 0;
}

static void stream_io(void *data, uint32_t id, void *area, uint32_t size) {
    TNPWStream *s = data;
    if (id != SPA_IO_Position) return;
    struct spa_io_position *position = size >= sizeof(*position) ? area : NULL;
    /* Same handoff as PipeWire stream.c impl_set_io: finish earlier callbacks
     * before replacing the borrowed IO pointer. Merely storing an atomic pointer
     * on the main loop would not protect the previous IO area's lifetime. */
#if PW_CHECK_VERSION(1, 1, 0)
    /* Newer PipeWire can assign different data loops to individual streams. */
    pw_loop_invoke(pw_stream_get_data_loop(s->stream), set_stream_position,
                   0, &position, sizeof(position), true, s);
#else
    struct pw_context *context = pw_core_get_context(pw_stream_get_core(s->stream));
    pw_data_loop_invoke(pw_context_get_data_loop(context), set_stream_position,
                        0, &position, sizeof(position), true, s);
#endif
}

static void stream_param(void *data, uint32_t id, const struct spa_pod *param) {
    TNPWStream *s = data;
    if (id != SPA_PARAM_Format) return;
    struct spa_audio_info_raw info = {0};
    bool valid = param && spa_format_audio_raw_parse(param, &info) >= 0 &&
        info.format == SPA_AUDIO_FORMAT_F32 && info.rate == s->sample_rate && info.channels == s->channels;
    uint64_t next = ((uint64_t)info.rate << 32) | info.channels;
    if (!valid || atomic_load_explicit(&s->format_snapshot, memory_order_acquire) != next)
        tn_pcm_ring_restart(&s->ring);
    atomic_store_explicit(&s->format_snapshot, next, memory_order_release);
    atomic_store_explicit(&s->format_valid, valid, memory_order_release);
}

static void stream_state(void *data, enum pw_stream_state old,
                         enum pw_stream_state state, const char *error) {
    (void)error;
    TNPWStream *s = data;
    if (old == PW_STREAM_STATE_STREAMING || state == PW_STREAM_STATE_STREAMING)
        tn_pcm_ring_restart(&s->ring);
    atomic_store_explicit(&s->state, state, memory_order_release);
}

static void stream_process(void *data) {
    TNPWStream *s = data;
    /* Only the synchronized data loop dereferences borrowed IO memory. Rust
     * receives an atomic value copy, never a native pointer. */
    struct spa_io_position *position = atomic_load_explicit(&s->position, memory_order_acquire);
    uint64_t snapshot = 0;
    if (position && position->clock.rate.num == 1)
        snapshot = ((uint64_t)position->clock.id << 32) | position->clock.rate.denom;
    atomic_store_explicit(&s->clock_snapshot, snapshot, memory_order_release);
    struct pw_buffer *b = pw_stream_dequeue_buffer(s->stream);
    if (!b) return;
    struct spa_buffer *buffer = b->buffer;
    /* Every dequeued buffer, including unusable buffers, is returned exactly once. */
    if (!buffer || buffer->n_datas != 1) goto done;
    struct spa_data *d = &buffer->datas[0];
    uint32_t stride = s->channels * sizeof(float);
    if (!d->data || !d->chunk || !stride) goto done;
    if (!atomic_load_explicit(&s->format_valid, memory_order_acquire)) {
        if (s->mode == 1 || s->mode == 4) d->chunk->size = 0;
        goto done;
    }
    uint32_t graph_frames = position && position->clock.duration <= UINT32_MAX
        ? (uint32_t)position->clock.duration : 0;
    if (s->mode == 1 || s->mode == 4) {
        uint32_t frames = d->maxsize / stride;
        if (b->requested && b->requested < frames) frames = (uint32_t)b->requested;
        atomic_store_explicit(&s->quantum_snapshot, ((uint64_t)graph_frames << 32) | frames,
                              memory_order_release);
        tn_pcm_ring_read(&s->ring, d->data, frames);
        d->chunk->offset = 0;
        d->chunk->stride = (int32_t)stride;
        d->chunk->size = frames * stride;
    } else {
        uint32_t offset = d->chunk->offset;
        uint32_t size = d->chunk->size;
        if (offset > d->maxsize || size > d->maxsize - offset ||
            size % stride || offset % sizeof(float) ||
            (d->chunk->stride != 0 && d->chunk->stride != (int32_t)stride)) goto done;
        atomic_store_explicit(&s->quantum_snapshot,
                              ((uint64_t)graph_frames << 32) | (size / stride), memory_order_release);
        tn_pcm_ring_write(&s->ring, (float *)((uint8_t *)d->data + offset), size / stride);
    }
done:
    pw_stream_queue_buffer(s->stream, b);
}

static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS, .state_changed = stream_state, .process = stream_process,
    .io_changed = stream_io, .param_changed = stream_param,
};

TNPWStream *tn_pw_stream_open(const char *name, const char *description,
                             const char *target, uint32_t mode, uint32_t channels,
                             uint32_t sample_rate, uint32_t block_frames) {
    if (!name || !description || mode > 4 || (mode >= 2 && (!target || !target[0]))) return NULL;
    if (sample_rate < 8000 || sample_rate > 384000 || !block_frames || block_frames > 8192) return NULL;
    pthread_once(&initialized, initialize);
    TNPWStream *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    atomic_init(&s->state, PW_STREAM_STATE_UNCONNECTED);
    atomic_init(&s->format_valid, false);
    atomic_init(&s->position, NULL);
    atomic_init(&s->clock_snapshot, 0);
    atomic_init(&s->format_snapshot, 0);
    atomic_init(&s->quantum_snapshot, 0);
    s->mode = mode; s->channels = channels; s->sample_rate = sample_rate;
    if (!tn_pcm_ring_init(&s->ring, channels, TN_RING_FRAMES)) goto fail;
    s->loop = pw_thread_loop_new("tonenari-stream", NULL);
    if (!s->loop) goto fail;
    struct pw_properties *p = pw_properties_new(
        PW_KEY_NODE_NAME, name, PW_KEY_NODE_DESCRIPTION, description,
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, (mode == 1 || mode == 4) ? "Playback" : "Capture",
        PW_KEY_MEDIA_ROLE, "DSP", PW_KEY_NODE_VIRTUAL, "true",
        PW_KEY_NODE_GROUP, "tonenari.graph", PW_KEY_NODE_RATE, "1/48000",
        PW_KEY_NODE_LATENCY, "128/48000",
        "node.pause-on-idle", "false", "node.always-process", "true", NULL);
    if (!p) goto fail;
    /* Preferences belong to this application's adapter, never server metadata. */
    pw_properties_setf(p, PW_KEY_NODE_RATE, "1/%u", sample_rate);
    pw_properties_setf(p, PW_KEY_NODE_LATENCY, "%u/%u", block_frames, sample_rate);
    if (mode < 2) pw_properties_set(p, PW_KEY_MEDIA_CLASS, mode == 0 ? "Audio/Sink" : "Audio/Source");
    else {
        pw_properties_set(p, PW_KEY_TARGET_OBJECT, target);
        pw_properties_set(p, "stream.dont-remix", "true");
        pw_properties_set(p, "node.dont-reconnect", "true");
        /* An unavailable explicit target must never leak audio to the default. */
        pw_properties_set(p, "node.dont-fallback", "true");
        pw_properties_set(p, "node.passive", "true");
    }
    s->stream = pw_stream_new_simple(pw_thread_loop_get_loop(s->loop), name, p, &stream_events, s);
    if (!s->stream) goto fail;
    uint8_t storage[2048];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    struct spa_audio_info_raw info = { .format = SPA_AUDIO_FORMAT_F32, .rate = sample_rate, .channels = channels };
    if (channels == 1) info.position[0] = SPA_AUDIO_CHANNEL_MONO;
    else if (channels == 2) { info.position[0] = SPA_AUDIO_CHANNEL_FL; info.position[1] = SPA_AUDIO_CHANNEL_FR; }
    else for (uint32_t c = 0; c < channels; c++) info.position[c] = SPA_AUDIO_CHANNEL_AUX0 + c;
    const struct spa_pod *param = spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &info);
    enum pw_stream_flags flags = PW_STREAM_FLAG_MAP_BUFFERS | PW_STREAM_FLAG_RT_PROCESS;
    if (mode >= 2) flags |= PW_STREAM_FLAG_AUTOCONNECT;
    if (pw_stream_connect(s->stream, (mode == 1 || mode == 4) ? PW_DIRECTION_OUTPUT : PW_DIRECTION_INPUT,
                          PW_ID_ANY, flags, &param, 1) < 0 || pw_thread_loop_start(s->loop) < 0) goto fail;
    return s;
fail:
    tn_pw_stream_close(s);
    return NULL;
}

void tn_pw_stream_close(TNPWStream *s) {
    if (!s) return;
    if (s->loop) {
        /* Disconnect synchronizes with the data loop. The control-loop lock is
         * never acquired by our RT callbacks; it serializes control callbacks. */
        pw_thread_loop_lock(s->loop);
        if (s->stream) { pw_stream_disconnect(s->stream); pw_stream_destroy(s->stream); }
        pw_thread_loop_unlock(s->loop);
        pw_thread_loop_stop(s->loop);
        pw_thread_loop_destroy(s->loop);
    }
    tn_pcm_ring_clear(&s->ring);
    free(s);
}
int tn_pw_stream_ready(TNPWStream *s) {
    int state = atomic_load_explicit(&s->state, memory_order_acquire);
    return state == PW_STREAM_STATE_PAUSED || state == PW_STREAM_STATE_STREAMING;
}
int tn_pw_stream_connected(TNPWStream *s) {
    return atomic_load_explicit(&s->state, memory_order_acquire) == PW_STREAM_STATE_STREAMING;
}
int tn_pw_stream_input_activity(TNPWStream *s) {
    int state = atomic_load_explicit(&s->state, memory_order_acquire);
    return state == PW_STREAM_STATE_STREAMING ? 2 : state == PW_STREAM_STATE_PAUSED ? 1 : 0;
}
uint64_t tn_pw_stream_clock(TNPWStream *s) {
    if (!atomic_load_explicit(&s->format_valid, memory_order_acquire) ||
        atomic_load_explicit(&s->state, memory_order_acquire) != PW_STREAM_STATE_STREAMING) return 0;
    return atomic_load_explicit(&s->clock_snapshot, memory_order_acquire);
}
uint64_t tn_pw_stream_format(TNPWStream *s) {
    return atomic_load_explicit(&s->format_snapshot, memory_order_acquire);
}
uint64_t tn_pw_stream_quantum(TNPWStream *s) {
    if (!atomic_load_explicit(&s->format_valid, memory_order_acquire) ||
        atomic_load_explicit(&s->state, memory_order_acquire) != PW_STREAM_STATE_STREAMING) return 0;
    return atomic_load_explicit(&s->quantum_snapshot, memory_order_acquire);
}
void tn_pw_stream_discard_input(TNPWStream *s) {
    /* Engine is the sole consumer only for input-side streams. */
    if (s->mode == 0 || s->mode == 2 || s->mode == 3) tn_pcm_ring_discard(&s->ring);
}
TNPWTransferCounters tn_pw_stream_counters(TNPWStream *s) {
    TNPcmRingStats stats = tn_pcm_ring_stats(&s->ring);
    return (TNPWTransferCounters) {
        .overflows = stats.overflows,
        .starvations = stats.starvations,
    };
}
uint32_t tn_pw_stream_read(TNPWStream *s, float *samples, uint32_t frames) {
    return tn_pcm_ring_read(&s->ring, samples, frames);
}
uint32_t tn_pw_stream_write(TNPWStream *s, const float *samples, uint32_t frames) {
    return tn_pcm_ring_write(&s->ring, samples, frames);
}

struct TNPWDriver {
    struct pw_thread_loop *loop;
    struct pw_filter *filter;
    TNPWProcess process;
    void *userdata;
    _Atomic int state;
};
static void driver_state(void *data, enum pw_filter_state old,
                         enum pw_filter_state state, const char *error) {
    (void)old; (void)error;
    TNPWDriver *d = data;
    atomic_store_explicit(&d->state, state, memory_order_release);
}
static void driver_process(void *data, struct spa_io_position *p) {
    TNPWDriver *d = data;
    if (!p || p->clock.rate.num != 1 || p->clock.duration > UINT32_MAX) return;
    TNPWCycle cycle = { .position = p->clock.position, .monotonic_ns = p->clock.nsec,
        .clock_id = p->clock.id, .rate = p->clock.rate.denom, .frames = (uint32_t)p->clock.duration };
    d->process(d->userdata, &cycle);
}
static const struct pw_filter_events driver_events = {
    PW_VERSION_FILTER_EVENTS, .state_changed = driver_state, .process = driver_process,
};
TNPWDriver *tn_pw_driver_open(TNPWProcess process, void *userdata) {
    if (!process || !userdata) return NULL;
    pthread_once(&initialized, initialize);
    TNPWDriver *d = calloc(1, sizeof(*d));
    if (!d) return NULL;
    atomic_init(&d->state, PW_FILTER_STATE_UNCONNECTED);
    d->process = process; d->userdata = userdata;
    d->loop = pw_thread_loop_new("tonenari-clock", NULL);
    if (!d->loop) goto fail;
    d->filter = pw_filter_new_simple(pw_thread_loop_get_loop(d->loop), "ToneNari graph",
        pw_properties_new(PW_KEY_NODE_NAME, "io.tonenari.graph",
            PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Filter", PW_KEY_MEDIA_ROLE, "DSP",
            PW_KEY_NODE_GROUP, "tonenari.graph", PW_KEY_NODE_RATE, "1/48000",
            PW_KEY_NODE_LATENCY, "128/48000",
            "node.always-process", "true", "node.want-driver", "true", NULL), &driver_events, d);
    if (!d->filter || pw_filter_connect(d->filter, PW_FILTER_FLAG_RT_PROCESS, NULL, 0) < 0 ||
        pw_thread_loop_start(d->loop) < 0) goto fail;
    return d;
fail:
    tn_pw_driver_close(d);
    return NULL;
}
int tn_pw_driver_ready(TNPWDriver *d) {
    return atomic_load_explicit(&d->state, memory_order_acquire) == PW_FILTER_STATE_STREAMING;
}
void tn_pw_driver_close(TNPWDriver *d) {
    if (!d) return;
    if (d->loop) {
        pw_thread_loop_lock(d->loop);
        if (d->filter) { pw_filter_disconnect(d->filter); pw_filter_destroy(d->filter); }
        pw_thread_loop_unlock(d->loop);
        pw_thread_loop_stop(d->loop);
        pw_thread_loop_destroy(d->loop);
    }
    free(d);
}
