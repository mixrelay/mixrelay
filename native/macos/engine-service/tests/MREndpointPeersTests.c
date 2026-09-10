#include "MREndpointPeers.h"
#include <assert.h>
#include <stdio.h>

int main(void) {
    MREndpointPeers peers = {0};
    xpc_connection_t values[MR_ENDPOINT_MAX_PEERS + 1];
    for (size_t i = 0; i <= MR_ENDPOINT_MAX_PEERS; i++) {
        values[i] = xpc_connection_create(NULL, NULL);
        assert(values[i] != NULL);
        xpc_connection_set_event_handler(values[i], ^(xpc_object_t event) { (void)event; });
        xpc_connection_activate(values[i]);
    }
    // Same duplicate-subscription workload as the former append-only registry.
    xpc_object_t previous = xpc_array_create(NULL, 0);
    for (size_t i = 0; i < 1000; i++) {
        xpc_array_append_value(previous, values[0]);
        assert(mr_endpoint_peers_add(&peers, values[0]));
    }
    assert(xpc_array_get_count(previous) == 1000);
    assert(peers.count == 1);
    mr_endpoint_peers_remove(&peers, values[0]);
    assert(peers.count == 0);
    xpc_release(previous);
    puts("Duplicate subscription retained slots: append-only 1000 -> deduplicated 1 -> disconnected 0 (not an RSS measurement)");
    for (size_t cycle = 0; cycle < 1000; cycle++) {
        for (size_t i = 0; i < MR_ENDPOINT_MAX_PEERS; i++) {
            assert(mr_endpoint_peers_add(&peers, values[i]));
            assert(mr_endpoint_peers_add(&peers, values[i]));
            assert(peers.count == i + 1);
        }
        assert(!mr_endpoint_peers_add(&peers, values[MR_ENDPOINT_MAX_PEERS]));
        // Middle removal exercises swap compaction; remove twice is harmless.
        mr_endpoint_peers_remove(&peers, values[7]);
        mr_endpoint_peers_remove(&peers, values[7]);
        assert(peers.count == MR_ENDPOINT_MAX_PEERS - 1);
        for (size_t i = 0; i < MR_ENDPOINT_MAX_PEERS; i++) mr_endpoint_peers_remove(&peers, values[i]);
        assert(peers.count == 0);
        for (size_t i = 0; i < MR_ENDPOINT_MAX_PEERS; i++) assert(peers.values[i] == NULL);
    }
    for (size_t i = 0; i <= MR_ENDPOINT_MAX_PEERS; i++) {
        xpc_connection_cancel(values[i]);
        xpc_release(values[i]);
    }
    puts("MREndpointPeersTests passed: 1000 bounded add/deduplicate/remove cycles");
    return 0;
}
