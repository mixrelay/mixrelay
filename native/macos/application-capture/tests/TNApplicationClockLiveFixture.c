/* Explicit test-only build: reuse the production producer implementation while
 * injecting an in-process registry attachment instead of an installed XPC service. */
#include "../src/TNApplicationSourceTransport.c"
#include "TNEngineSourceRegistry.h"

TNEngineSourceRegistry *tn_application_clock_fixture_create(TNApplicationSourceTransport **output) {
    if (output == NULL) return NULL;
    *output = NULL;
    TNEngineSourceRegistry *registry = tn_engine_source_registry_create();
    TNEngineSourceRequest request = { .source_id = "owned-live-clock-fixture", .capacity_frames = 16384U,
        .channels = 2U, .transport_version = 2U };
    int descriptor = -1;
    size_t bytes = 0U;
    uint64_t lease = 0U, epoch = 0U;
    if (!tn_engine_source_registry_attach_clocked(registry, &request, &descriptor, &bytes, &lease, &epoch)) goto fail;
    const int owned = descriptor;
    descriptor = -1;
    void *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, owned, 0);
    if (memory == MAP_FAILED) { close(owned); goto fail; }
    TNApplicationSourceTransport *transport = calloc(1U, sizeof(*transport));
    if (transport == NULL) { munmap(memory, bytes); close(owned); goto fail; }
    transport->descriptor = owned; transport->memory = memory; transport->mapped_size = bytes;
    transport->buffer = tn_shared_audio_buffer_attach(memory, bytes);
    transport->clock = tn_application_clock_attach(memory, bytes, 16384U, 2U);
    transport->owned_epoch = epoch; transport->owned_lease = lease;
    if (transport->buffer == NULL || transport->clock == NULL) {
        munmap(memory, bytes); close(owned); free(transport); goto fail;
    }
    *output = transport;
    return registry;
fail:
    if (descriptor >= 0) close(descriptor);
    tn_engine_source_registry_destroy(registry);
    return NULL;
}

uint32_t tn_application_clock_fixture_read(TNEngineSourceRegistry *registry, float *samples, uint32_t rate) {
    return tn_engine_source_registry_read_clocked(registry,
        tn_engine_source_id_token("owned-live-clock-fixture"), samples, 128U, 2U, rate);
}
bool tn_application_clock_fixture_clock(TNEngineSourceRegistry *registry, TNApplicationSourceClock *clock) {
    return tn_engine_source_registry_clock(registry, tn_engine_source_id_token("owned-live-clock-fixture"), clock);
}
