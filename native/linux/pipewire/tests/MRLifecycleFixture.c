/* Owned isolated-daemon regression. Include the production implementation only
 * to drive pw_stream_set_active on this fixture's private stream, not user nodes. */
#include "../MRPipeWire.c"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <time.h>

typedef struct {
    MRPWStream *source, *observer;
    _Atomic bool generate;
    _Atomic uint64_t received, stale;
    _Atomic uint64_t idle_callbacks;
} Fixture;

static void tick(void *data, const MRPWCycle *cycle) {
    Fixture *f = data;
    if (cycle->frames > 2048) return;
    float samples[2048 * 2];
    bool generate = atomic_load(&f->generate);
    if (generate) {
        for (uint32_t n = 0; n < cycle->frames * 2; n++) samples[n] = 0.125f;
        mr_pw_stream_write(f->source, samples, cycle->frames);
    }
    uint32_t received = mr_pw_stream_read(f->observer, samples, cycle->frames);
    for (uint32_t n = 0; n < received * 2; n++) {
        if (fabsf(samples[n]) > 0.25f) atomic_fetch_add(&f->stale, 1);
        if (fabsf(samples[n]) > 0.05f) atomic_fetch_add(&f->received, 1);
    }
    if (!generate) atomic_fetch_add(&f->idle_callbacks, 1);
}
static void delay(void) {
    const struct timespec pause = {.tv_nsec = 1000000};
    nanosleep(&pause, NULL);
}
static void active(MRPWStream *s, bool value) {
    pw_thread_loop_lock(s->loop);
    assert(pw_stream_set_active(s->stream, value) == 0);
    /* set_active controls scheduling, not the client stream's link-state.
     * Exercise the actual SPA lifecycle event on this owned node explicitly. */
    struct pw_registry *registry = pw_core_get_registry(pw_stream_get_core(s->stream), PW_VERSION_REGISTRY, 0);
    assert(registry);
    struct pw_node *node = pw_registry_bind(registry, pw_stream_get_node_id(s->stream),
                                           PW_TYPE_INTERFACE_Node, PW_VERSION_NODE, 0);
    assert(node);
    struct spa_command command = SPA_NODE_COMMAND_INIT(value ? SPA_NODE_COMMAND_Start : SPA_NODE_COMMAND_Pause);
    assert(pw_node_send_command(node, &command) >= 0);
    pw_proxy_destroy((struct pw_proxy *)node);
    pw_proxy_destroy((struct pw_proxy *)registry);
    pw_thread_loop_unlock(s->loop);
    int expected = value ? PW_STREAM_STATE_STREAMING : PW_STREAM_STATE_PAUSED;
    for (unsigned n = 0; n < 3000; n++) {
        if (atomic_load(&s->state) == expected) return;
        delay();
    }
    fprintf(stderr, "lifecycle transition active=%d expected_state=%d observed_state=%d negotiated_format=%llu\n",
            value, expected, atomic_load(&s->state), (unsigned long long)mr_pw_stream_format(s));
    assert(!"owned stream did not transition state");
}
int main(void) {
    Fixture f = {0};
    atomic_init(&f.generate, false);
    atomic_init(&f.received, 0);
    atomic_init(&f.stale, 0);
    atomic_init(&f.idle_callbacks, 0);
    f.source = mr_pw_stream_open("io.mixrelay.fixture.lifecycle", "Owned lifecycle fixture",
                                NULL, 1, 2, 48000, 128);
    assert(f.source);
    /* Explicit target streams intentionally never reconnect/fall back. Wait for
     * this source to register before asking policy to resolve its exact name. */
    for (unsigned n = 0; n < 3000 && mr_pw_stream_format(f.source) == 0; n++) delay();
    assert(mr_pw_stream_format(f.source) == ((uint64_t)48000 << 32 | 2));
    char serial[32];
    pw_thread_loop_lock(f.source->loop);
    const char *value = pw_properties_get(pw_stream_get_properties(f.source->stream), PW_KEY_OBJECT_SERIAL);
    assert(value && strlen(value) < sizeof(serial));
    strcpy(serial, value);
    pw_thread_loop_unlock(f.source->loop);
    f.observer = mr_pw_stream_open("io.mixrelay.fixture.lifecycle.observer", "Owned observer",
                                  serial, 3, 2, 48000, 128);
    assert(f.observer);
    MRPWDriver *driver = mr_pw_driver_open(tick, &f);
    assert(driver);
    puts("lifecycle: initial source start"); fflush(stdout);
    active(f.source, true);
    for (unsigned cycle = 0; cycle < 5; cycle++) {
        atomic_store(&f.generate, false);
        uint64_t idle = atomic_load(&f.idle_callbacks);
        for (unsigned n = 0; n < 3000 && atomic_load(&f.idle_callbacks) == idle; n++) delay();
        assert(atomic_load(&f.idle_callbacks) != idle);
        printf("lifecycle: pause cycle=%u\n", cycle); fflush(stdout);
        active(f.source, false);
        /* There is no producer while generate=false; seed the stale queue that
         * the real PAUSED→STREAMING callback must invalidate on both endpoints. */
        float stale[8192 * 2];
        for (unsigned n = 0; n < 8192 * 2; n++) stale[n] = 1.0f;
        assert(mr_pw_stream_write(f.source, stale, 8192) == 8192);
        printf("lifecycle: resume cycle=%u\n", cycle); fflush(stdout);
        active(f.source, true);
        atomic_store(&f.generate, true);
        uint64_t before = atomic_load(&f.received);
        for (unsigned n = 0; n < 400; n++) delay();
        assert(atomic_load(&f.received) > before);
        assert(atomic_load(&f.stale) == 0);
    }
    mr_pw_driver_close(driver);
    mr_pw_stream_close(f.observer);
    mr_pw_stream_close(f.source);
    printf("PASS: five real native pause/resume transitions; fresh_samples=%llu; stale_samples=%llu\n",
           (unsigned long long)atomic_load(&f.received), (unsigned long long)atomic_load(&f.stale));
}
