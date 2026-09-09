/* C adapts dispatch/XPC blocks only. Endpoint state, mappings, peers,
 * authorization, routing and publication are Rust-owned. */
#include <dispatch/dispatch.h>
#include <stdlib.h>
#include <xpc/xpc.h>
#include "TNHALControlProtocol.h"

extern bool tn_engine_service_accept_peer(xpc_connection_t peer);
extern void tn_engine_service_peer_event(xpc_connection_t peer, xpc_object_t event);
extern void tn_engine_service_control_tick(void);

typedef struct TNEngineDispatch {
    xpc_connection_t listener;
    dispatch_source_t timer;
} TNEngineDispatch;

void *tn_engine_dispatch_start(void) {
    TNEngineDispatch *owner = calloc(1, sizeof(*owner));
    if (owner == NULL) return NULL;
    owner->listener = xpc_connection_create_mach_service(
        TN_HAL_CONTROL_SERVICE_NAME, NULL, XPC_CONNECTION_MACH_SERVICE_LISTENER);
    owner->timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0,
        dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0));
    if (owner->listener == NULL || owner->timer == NULL) {
        if (owner->listener != NULL) xpc_release(owner->listener);
        if (owner->timer != NULL) { dispatch_resume(owner->timer); dispatch_source_cancel(owner->timer); dispatch_release(owner->timer); }
        free(owner);
        return NULL;
    }
    xpc_connection_set_event_handler(owner->listener, ^(xpc_object_t peer) {
        if (xpc_get_type(peer) != XPC_TYPE_CONNECTION) return;
        if (!tn_engine_service_accept_peer(peer)) { xpc_connection_cancel(peer); return; }
        xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
            tn_engine_service_peer_event(peer, event);
        });
        xpc_connection_activate(peer);
    });
    dispatch_source_set_timer(owner->timer, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_MSEC),
        10 * NSEC_PER_MSEC, NSEC_PER_MSEC);
    dispatch_source_set_event_handler(owner->timer, ^{ tn_engine_service_control_tick(); });
    xpc_connection_activate(owner->listener);
    dispatch_activate(owner->timer);
    return owner;
}

/* Queued events only call the static Rust dispatcher and never dereference
 * this C owner after cancellation. Rust drains audio before service shutdown. */
void tn_engine_dispatch_destroy(void *context) {
    TNEngineDispatch *owner = context;
    if (owner == NULL) return;
    xpc_connection_cancel(owner->listener);
    dispatch_source_cancel(owner->timer);
    xpc_release(owner->listener);
    dispatch_release(owner->timer);
    free(owner);
}
