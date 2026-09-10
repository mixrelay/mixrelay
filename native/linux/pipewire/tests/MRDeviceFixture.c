/* Owned lab-only source/sink. Generates PCM directly in PipeWire callbacks and
 * measures sink PCM independently of MixRelay's meters or transport rings.
 * Never autoconnects or changes the session's default devices. */
#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <math.h>

typedef struct {
    struct pw_stream *stream;
    uint32_t channels;
    int source;
    uint64_t position, frames, active, invalid, gap, maximum_gap;
    double peaks[64];
} Fixture;
static volatile sig_atomic_t stopping;
static volatile sig_atomic_t resetting;
static volatile sig_atomic_t toggle_source, toggle_sink;
static void stop(int signal) { (void)signal; stopping = 1; }
static void reset(int signal) { (void)signal; resetting = 1; }
static void toggle(int signal) { if (signal == SIGUSR2) toggle_source = 1; else toggle_sink = 1; }

static void process(void *data) {
    Fixture *f = data;
    struct pw_buffer *b = pw_stream_dequeue_buffer(f->stream);
    if (!b) return;
    struct spa_buffer *buffer = b->buffer;
    if (!buffer || buffer->n_datas != 1) goto done;
    struct spa_data *d = &buffer->datas[0];
    uint32_t stride = f->channels * sizeof(float);
    if (!d->data || !d->chunk) goto done;
    if (f->source) {
        uint32_t frames = d->maxsize / stride;
        if (b->requested && b->requested < frames) frames = b->requested;
        float *samples = d->data;
        for (uint32_t n = 0; n < frames; n++, f->position++)
            for (uint32_t c = 0; c < f->channels; c++)
                samples[n * f->channels + c] = (c + 1) / 1024.0 * sin(2.0 * M_PI * (f->position % 48) / 48.0);
        d->chunk->offset = 0; d->chunk->stride = stride; d->chunk->size = frames * stride;
    } else {
        if (d->chunk->offset > d->maxsize || d->chunk->size > d->maxsize - d->chunk->offset || d->chunk->size % stride) {
            f->invalid++; goto done;
        }
        float *samples = (float *)((uint8_t *)d->data + d->chunk->offset);
        uint32_t frames = d->chunk->size / stride;
        f->frames += frames;
        for (uint32_t n = 0; n < frames; n++) {
            int active = 0;
            for (uint32_t c = 0; c < f->channels; c++) {
                double value = fabs(samples[n * f->channels + c]);
                if (!isfinite(value)) { f->invalid++; continue; }
                if (value > f->peaks[c]) f->peaks[c] = value;
                active |= value > 0.0001;
            }
            if (active) {
                if (f->active && f->gap > f->maximum_gap) f->maximum_gap = f->gap;
                f->active++; f->gap = 0;
            } else f->gap++;
        }
    }
done:
    pw_stream_queue_buffer(f->stream, b);
}
static const struct pw_stream_events events = { PW_VERSION_STREAM_EVENTS, .process = process };

static int open_fixture(Fixture *f, struct pw_main_loop *loop, const char *name) {
    struct pw_properties *props = pw_properties_new(PW_KEY_NODE_NAME, name,
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, f->source ? "Playback" : "Capture",
        PW_KEY_MEDIA_CLASS, f->source ? "Audio/Source" : "Audio/Sink",
        PW_KEY_NODE_VIRTUAL, "true", "node.pause-on-idle", "false", "node.always-process", "true", NULL);
    pw_properties_setf(props, "audio.channels", "%u", f->channels);
    f->stream = pw_stream_new_simple(pw_main_loop_get_loop(loop), name, props, &events, f);
    if (!f->stream) return -1;
    uint8_t storage[2048];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    struct spa_audio_info_raw format = { .format = SPA_AUDIO_FORMAT_F32, .rate = 48000, .channels = f->channels };
    if (f->channels == 2) { format.position[0] = SPA_AUDIO_CHANNEL_FL; format.position[1] = SPA_AUDIO_CHANNEL_FR; }
    else for (uint32_t c = 0; c < f->channels; c++) format.position[c] = SPA_AUDIO_CHANNEL_AUX0 + c;
    const struct spa_pod *param = spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &format);
    return pw_stream_connect(f->stream, f->source ? PW_DIRECTION_OUTPUT : PW_DIRECTION_INPUT, PW_ID_ANY,
        PW_STREAM_FLAG_MAP_BUFFERS, &param, 1);
}
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    Fixture source = { .source = 1, .channels = (uint32_t)atoi(argv[1]) };
    Fixture sink = { .channels = (uint32_t)atoi(argv[2]) };
    if (!source.channels || source.channels > 64 || !sink.channels || sink.channels > 64) return 2;
    pw_init(NULL, NULL);
    struct pw_main_loop *loop = pw_main_loop_new(NULL);
    if (!loop) return 1;
    int result = 1;
    if (open_fixture(&source, loop, "io.mixrelay.lab-device-source") < 0 ||
        open_fixture(&sink, loop, "io.mixrelay.lab-device-sink") < 0) goto cleanup;
    signal(SIGTERM, stop); signal(SIGINT, stop); signal(SIGUSR1, reset);
    signal(SIGUSR2, toggle); signal(SIGHUP, toggle);
    /* Main-loop callbacks serialize fixture counters; no data race with reporting. */
    while (!stopping) {
        int iteration = pw_loop_iterate(pw_main_loop_get_loop(loop), 50);
        if (iteration < 0 && iteration != -EINTR) { fprintf(stderr, "main loop: %d\n", iteration); goto cleanup; }
        if (resetting) { sink.frames = sink.active = sink.invalid = sink.gap = sink.maximum_gap = 0; memset(sink.peaks, 0, sizeof(sink.peaks)); resetting = 0; }
        /* Independent removal/arrival with stable names and fresh native serials. */
        if (toggle_source) {
            if (source.stream) { pw_stream_destroy(source.stream); source.stream = NULL; }
            else if (open_fixture(&source, loop, "io.mixrelay.lab-device-source") < 0) goto cleanup;
            toggle_source = 0;
        }
        if (toggle_sink) {
            if (sink.stream) { pw_stream_destroy(sink.stream); sink.stream = NULL; }
            else if (open_fixture(&sink, loop, "io.mixrelay.lab-device-sink") < 0) goto cleanup;
            toggle_sink = 0;
        }
    }
    printf("{\"frames\":%lu,\"active\":%lu,\"invalid\":%lu,\"maximumGap\":%lu,\"peaks\":[", sink.frames, sink.active, sink.invalid, sink.maximum_gap);
    for (uint32_t c = 0; c < sink.channels; c++) printf("%s%.9f", c ? "," : "", sink.peaks[c]);
    puts("]}");
    result = 0;
cleanup:
    if (source.stream) pw_stream_destroy(source.stream);
    if (sink.stream) pw_stream_destroy(sink.stream);
    pw_main_loop_destroy(loop);
    pw_deinit();
    return result;
}
