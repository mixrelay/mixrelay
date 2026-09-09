#ifndef TN_ENGINE_SOURCE_REGISTRY_H
#define TN_ENGINE_SOURCE_REGISTRY_H

#include "TNEngineControl.h"
#include "TNSharedAudioBuffer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    TN_ENGINE_MAX_APPLICATION_SOURCES = 64,
};

typedef struct TNEngineSourceRegistry TNEngineSourceRegistry;

typedef struct TNApplicationSourceClock {
    uint64_t epoch;
    uint64_t native_frames;
    uint32_t native_rate;
    uint32_t graph_rate;
    uint32_t running;
    uint32_t synchronized;
} TNApplicationSourceClock;

bool tn_engine_source_registry_clock(
    TNEngineSourceRegistry *registry, uint64_t source_token, TNApplicationSourceClock *snapshot);
/* Single engine/control owner calls when choosing the native clock master. */
bool tn_engine_source_registry_set_clock_master(
    TNEngineSourceRegistry *registry, uint64_t source_token, bool master);

TNEngineSourceRegistry *tn_engine_source_registry_create(void);
void tn_engine_source_registry_destroy(TNEngineSourceRegistry *registry);

/* A successful attachment transfers an owned descriptor to the caller. */
bool tn_engine_source_registry_attach_clocked(
    TNEngineSourceRegistry *registry, const TNEngineSourceRequest *request,
    int *descriptor, size_t *mapped_size, uint64_t *producer_lease, uint64_t *epoch);

uint64_t tn_engine_source_id_token(const char *source_id);
uint32_t tn_engine_source_registry_read_clocked(
    TNEngineSourceRegistry *registry, uint64_t source_token, float *interleaved_samples,
    uint32_t frame_count, uint32_t channels, uint32_t sample_rate);

#ifdef __cplusplus
}
#endif

#endif
