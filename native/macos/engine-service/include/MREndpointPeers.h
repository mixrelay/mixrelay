#ifndef MR_ENDPOINT_PEERS_H
#define MR_ENDPOINT_PEERS_H

#include <stdbool.h>
#include <stddef.h>
#include <xpc/xpc.h>

// Audio-server subscriptions only; callers hold the endpoint control lock.
// Bound retained connections and deduplicate resubscriptions. No audio-thread use.
enum { MR_ENDPOINT_MAX_PEERS = 16 };
typedef struct MREndpointPeers {
    xpc_connection_t values[MR_ENDPOINT_MAX_PEERS];
    size_t count;
} MREndpointPeers;

static inline bool mr_endpoint_peers_add(MREndpointPeers *peers, xpc_connection_t peer) {
    for (size_t i = 0; i < peers->count; i++) {
        if (peers->values[i] == peer) return true;
    }
    if (peers->count == MR_ENDPOINT_MAX_PEERS) return false;
    peers->values[peers->count++] = xpc_retain(peer);
    return true;
}

static inline void mr_endpoint_peers_remove(MREndpointPeers *peers, xpc_connection_t peer) {
    for (size_t i = 0; i < peers->count; i++) {
        if (peers->values[i] == peer) {
            xpc_release(peers->values[i]);
            peers->values[i] = peers->values[--peers->count];
            peers->values[peers->count] = NULL;
            return;
        }
    }
}
#endif
