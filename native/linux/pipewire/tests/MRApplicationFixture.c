/* Owned application stream only: no virtual device, default routing, or service
 * change. It never autoconnects to an audio output. Capture tests explicitly link
 * their owned monitor to this node and reap this process after each phase. */
#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>
#include <signal.h>
#include <math.h>
#include <stdlib.h>

static volatile sig_atomic_t stopping;
static struct pw_stream *stream;
static uint64_t position;
static uint32_t sample_rate = 48000;
static void stop(int signal) { (void)signal; stopping = 1; }
static void process(void *data) {
    (void)data;
    struct pw_buffer *b = pw_stream_dequeue_buffer(stream);
    if (!b) return;
    struct spa_data *d = &b->buffer->datas[0];
    if (!d->data || !d->chunk) goto done;
    uint32_t frames = d->maxsize / (2 * sizeof(float));
    if (b->requested && b->requested < frames) frames = b->requested;
    float *samples = d->data;
    for (uint32_t f = 0; f < frames; f++, position++) {
        samples[f * 2] = 0.125f * sinf(2.0f * 3.14159265f * 1000.0f * (position % sample_rate) / sample_rate);
        samples[f * 2 + 1] = samples[f * 2] * 0.5f;
    }
    d->chunk->offset = 0;
    d->chunk->stride = 2 * sizeof(float);
    d->chunk->size = frames * d->chunk->stride;
done:
    pw_stream_queue_buffer(stream, b);
}
static const struct pw_stream_events events = { PW_VERSION_STREAM_EVENTS, .process = process };
int main(void) {
    const char *requested_rate = getenv("MIXRELAY_LAB_SOURCE_RATE");
    if (requested_rate) {
        if (!strcmp(requested_rate, "44100")) sample_rate = 44100;
        else if (strcmp(requested_rate, "48000")) return 2;
    }
    pw_init(NULL, NULL);
    struct pw_main_loop *loop = pw_main_loop_new(NULL);
    if (!loop) return 1;
    struct pw_properties *props = pw_properties_new(
        PW_KEY_NODE_NAME, "io.mixrelay.lab-application",
        PW_KEY_MEDIA_CLASS, "Stream/Output/Audio",
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Playback",
        PW_KEY_APP_ID, "org.mixrelay.fixture.application",
        PW_KEY_APP_NAME, "MixRelay Application Fixture",
        "node.autoconnect", "false", "node.dont-fallback", "true",
        "node.always-process", "true", "node.pause-on-idle", "false", NULL);
    stream = pw_stream_new_simple(pw_main_loop_get_loop(loop), "MixRelay application fixture", props, &events, NULL);
    int result = 1;
    if (!stream) goto cleanup;
    uint8_t storage[1024];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    struct spa_audio_info_raw format = { .format = SPA_AUDIO_FORMAT_F32, .rate = sample_rate, .channels = 2,
        .position = { SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR } };
    const struct spa_pod *param = spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &format);
    if (pw_stream_connect(stream, PW_DIRECTION_OUTPUT, PW_ID_ANY, PW_STREAM_FLAG_MAP_BUFFERS, &param, 1) < 0) goto cleanup;
    signal(SIGTERM, stop); signal(SIGINT, stop);
    while (!stopping) pw_loop_iterate(pw_main_loop_get_loop(loop), 20);
    result = 0;
cleanup:
    if (stream) pw_stream_destroy(stream);
    pw_main_loop_destroy(loop);
    return result;
}
