/* Only Apple's dispatch-block adapter needs this C fixture. Device policy and
 * PCM tests execute the current Rust core. No old driver implementation is used. */
#include <CoreAudio/AudioServerPlugIn.h>
#include <Block.h>
#include <assert.h>
#include <dispatch/dispatch.h>
#include <stdio.h>
#include <string.h>
#include <xpc/xpc.h>
#include "TNHALControlProtocol.h"

typedef void (^Handler)(xpc_object_t);
static Handler handlers[16];
static unsigned handler_count;
static unsigned connection_count;
static unsigned request_count;
static unsigned cancelled_count;

static xpc_connection_t create_peer(const char *name, dispatch_queue_t queue, uint64_t flags) {
    assert(strcmp(name, TN_HAL_CONTROL_SERVICE_NAME) == 0 && flags == 0);
    ++connection_count;
    /* Anonymous, never resumed: cannot talk to an installed engine. */
    xpc_connection_t peer = xpc_connection_create(NULL, queue);
    if (peer != NULL) xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
        (void)event;
    });
    return peer;
}
static void handler(xpc_connection_t peer, Handler callback) {
    assert(peer != NULL && handler_count < 16);
    handlers[handler_count++] = Block_copy(callback);
}
static void resume_peer(xpc_connection_t peer) { assert(peer != NULL); }
static void cancel_peer(xpc_connection_t peer) {
    assert(peer != NULL);
    ++cancelled_count;
    /* Balance the real object's initial suspension before its last release.
     * Cancel first so fixture cleanup cannot activate any IPC work. */
    xpc_connection_cancel(peer);
    xpc_connection_resume(peer);
}
static void send_request(xpc_connection_t peer, xpc_object_t message) {
    assert(peer != NULL);
    assert(tn_hal_control_parse_inventory_request(message));
    ++request_count;
}

#define xpc_connection_create_mach_service create_peer
#define xpc_connection_set_event_handler handler
#define xpc_connection_resume resume_peer
#define xpc_connection_cancel cancel_peer
#define xpc_connection_send_message send_request
#include "../driver/ToneNariHALBridge.c"
#undef xpc_connection_create_mach_service
#undef xpc_connection_set_event_handler
#undef xpc_connection_resume
#undef xpc_connection_cancel
#undef xpc_connection_send_message

static void host_changed(void *context, uint32_t object, uint32_t change) {
    (void)context; (void)object; (void)change;
}
static bool host_request(void *context, uint32_t object, uint64_t action, uint64_t generation) {
    (void)context; (void)object; (void)action; (void)generation;
    return false;
}
int main(void) {
    const TNHALCoreHostOps callbacks = {NULL, host_changed, host_request};
    gCore = tn_hal_core_create_native(&callbacks);
    assert(gCore != NULL);
    gControlQueue = dispatch_queue_create("io.tonenari.test.control", DISPATCH_QUEUE_SERIAL);
    open_control_connection();
    assert(connection_count == 1 && request_count == 1 && handler_count == 1);
    /* XPC exports immutable error singletons; handlers receive the generic
     * object type. The adapter only compares their identity, never mutates them. */
    xpc_object_t interrupted = (xpc_object_t)XPC_ERROR_CONNECTION_INTERRUPTED;
    xpc_object_t invalid = (xpc_object_t)XPC_ERROR_CONNECTION_INVALID;
    for (unsigned i = 0; i < 1000; ++i) handlers[0](interrupted);
    assert(connection_count == 1 && request_count == 1 && gReconnect);
    for (unsigned i = 0; i < 1000; ++i) handlers[0](invalid);
    assert(gReconnect && connection_count == 1);
    for (unsigned i = 0; i < 10; ++i) control_tick();
    assert(!gReconnect && connection_count == 2 && handler_count == 2);
    assert(cancelled_count == 1 && request_count == 2);
    for (unsigned i = 0; i < 1000; ++i) {
        handlers[0](interrupted);
        handlers[0](invalid);
    }
    assert(!gReconnect && request_count == 2 && connection_count == 2);
    handlers[1](invalid);
    assert(gReconnect && request_count == 2 && connection_count == 2);
    xpc_object_t malformed = xpc_dictionary_create(NULL, NULL, 0);
    handlers[1](malformed);
    xpc_release(malformed);
    assert(request_count == 2 && connection_count == 2);
    for (unsigned i = 0; i < 10; ++i) control_tick();
    assert(!gReconnect && request_count == 3 && connection_count == 3);
    assert(gConnectionGeneration == 3 && cancelled_count == 2);
    for (unsigned i = 0; i < 10; ++i) control_tick();
    assert(request_count == 4 && connection_count == 3);
    /* Exhaustion must never reuse an old closure authority. */
    handlers[2](invalid);
    gConnectionGeneration = UINT64_MAX;
    for (unsigned i = 0; i < 10; ++i) control_tick();
    assert(gReconnect && gConnectionGeneration == UINT64_MAX);
    assert(request_count == 4 && connection_count == 3);
    cancel_peer(gConnection);
    xpc_release(gConnection);
    gConnection = NULL;
    for (unsigned i = 0; i < handler_count; ++i) Block_release(handlers[i]);
    tn_hal_core_destroy(gCore);
    gCore = NULL;
    dispatch_release(gControlQueue);
    puts("PASS current HAL dispatch: 1000 interruptions, invalid errors and stale callbacks; reconnect and periodic inventory");
    return 0;
}
