/* Explicit test-only build: reuse the production producer implementation while
 * injecting an in-process registry attachment instead of an installed XPC service. */
#include "TNApplicationSourceTransport.h"
#include "TNEngineSourceRegistry.h"
#include <unistd.h>

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
    TNEngineSourceAttachment attachment = { descriptor, bytes, lease, epoch };
    descriptor = -1;
    TNApplicationSourceTransport *transport = tn_application_source_transport_attach(
        &attachment, 16384U, 2U, NULL);
    if (transport == NULL) goto fail;
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
