/* Clang supplies the nested XPC Block ABI; Rust owns dispatch resources. */
#include <xpc/xpc.h>

extern bool tn_engine_service_accept_peer(xpc_connection_t peer);
extern void tn_engine_service_peer_event(xpc_connection_t peer, xpc_object_t event);

void tn_engine_dispatch_install_discard_handler(xpc_connection_t listener) {
    xpc_connection_set_event_handler(listener, ^(xpc_object_t event) { (void)event; });
}

void tn_engine_dispatch_install_handler(xpc_connection_t listener) {
    xpc_connection_set_event_handler(listener, ^(xpc_object_t peer) {
        if (xpc_get_type(peer) != XPC_TYPE_CONNECTION) return;
        if (!tn_engine_service_accept_peer(peer)) { xpc_connection_cancel(peer); return; }
        xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
            tn_engine_service_peer_event(peer, event);
        });
        xpc_connection_activate(peer);
    });
}
