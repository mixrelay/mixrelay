/* MixRelay-owned LV2 native-state fixture; MIT OR Apache-2.0, same as this adapter.
 * No installed plugin is modified. Compile inside a temporary fixture bundle. */
#include <lv2/core/lv2.h>
#include <lv2/state/state.h>
#include <lv2/urid/urid.h>
#include <stdlib.h>
#include <string.h>

typedef struct { unsigned char bytes[5]; LV2_URID key, type; float *gain; } Fixture;
static LV2_Handle instantiate(const LV2_Descriptor *d, double rate, const char *path, const LV2_Feature *const *features) {
    (void)d; (void)rate; (void)path;
    const LV2_URID_Map *map = NULL;
    for (size_t i = 0; features && features[i]; ++i) if (!strcmp(features[i]->URI, LV2_URID__map)) map = features[i]->data;
    if (!map) return NULL;
    Fixture *f = calloc(1, sizeof(*f));
    if (!f) return NULL;
    memcpy(f->bytes, (unsigned char[]){1, 0, 255, 13, 0}, 5);
    f->key = map->map(map->handle, "urn:mixrelay:fixture:opaque");
    f->type = map->map(map->handle, "urn:mixrelay:fixture:binary");
    return f;
}
static void connect_port(LV2_Handle h, uint32_t p, void *v) { if (p == 2) ((Fixture *)h)->gain = v; }
static void activate(LV2_Handle h) { (void)h; }
static void run(LV2_Handle h, uint32_t frames) { (void)h; (void)frames; }
static void deactivate(LV2_Handle h) { (void)h; }
static void cleanup(LV2_Handle h) { free(h); }
static LV2_State_Status save(LV2_Handle h, LV2_State_Store_Function store, LV2_State_Handle handle, uint32_t flags, const LV2_Feature *const *features) {
    (void)flags; (void)features;
    Fixture *f = h;
    if (f->bytes[0] == 253) return LV2_STATE_ERR_UNKNOWN;
    return store(handle, f->key, f->bytes, 5, f->type, LV2_STATE_IS_POD | LV2_STATE_IS_PORTABLE);
}
static LV2_State_Status restore(LV2_Handle h, LV2_State_Retrieve_Function retrieve, LV2_State_Handle handle, uint32_t flags, const LV2_Feature *const *features) {
    (void)flags; (void)features;
    Fixture *f = h; size_t size = 0; uint32_t type = 0, saved_flags = 0;
    const unsigned char *bytes = retrieve(handle, f->key, &size, &type, &saved_flags);
    if (!bytes || size != 5 || type != f->type || bytes[0] == 254) return LV2_STATE_ERR_UNKNOWN;
    memcpy(f->bytes, bytes, 5); return LV2_STATE_SUCCESS;
}
static const LV2_State_Interface state = {save, restore};
static const void *extension_data(const char *uri) { return !strcmp(uri, LV2_STATE__interface) ? &state : NULL; }
static const LV2_Descriptor descriptor = {"urn:mixrelay:state-fixture", instantiate, connect_port, activate, run, deactivate, cleanup, extension_data};
LV2_SYMBOL_EXPORT const LV2_Descriptor *lv2_descriptor(uint32_t index) { return index == 0 ? &descriptor : NULL; }
