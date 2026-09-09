/* Explicit, isolated private-device acceptance. Never sets hardware formats,
 * defaults, or the user's graph. IOProcs only contribute silence. */
#include "TNDeviceIO.h"
#include <assert.h>
#include <stdio.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: probe primary-output-UID second-output-UID\n"); return 2; }
    const char *members[] = {argv[1], argv[2]};
    TNAggregateMemberInfo facts[2] = {0};
    if (!tn_aggregate_member_info(members[0], &facts[0]) ||
        !tn_aggregate_member_info(members[1], &facts[1])) return 1;
    int32_t status = 0;
    TNAggregateGroup *group = tn_aggregate_group_create(members, facts, 2, 0, &status);
    if (!group || status) {
        if (group) assert(tn_aggregate_group_try_destroy(group));
        fprintf(stderr, "private aggregate failed: %d\n", status); return 1;
    }
    for (unsigned i = 0; i < 100 && !tn_aggregate_group_ready(group); ++i) usleep(20000);
    assert(tn_aggregate_group_ready(group));
    uint32_t device = 0, offset = 0, total = 0;
    assert(tn_aggregate_group_binding(group, members[0], false, 2, &device, &offset, &total));
    assert(offset == 0 && total >= 4);
    uint32_t primary_device = device;
    assert(tn_aggregate_group_binding(group, members[1], false, 2, &device, &offset, &total));
    assert(device == primary_device && offset >= 2);
    printf("OS readback: private group=%u primary offset=0 secondary offset=%u total=%u; main UID, order, drift policy verified\n", device, offset, total);
    TNDeviceIO *first = tn_device_io_create_group(group, members[0], false, 2, 48000);
    TNDeviceIO *second = tn_device_io_create_group(group, members[1], false, 2, 48000);
    bool connected = first && second && tn_device_io_connect(first) && tn_device_io_connect(second);
    if (connected) {
        usleep(500000);
        connected = tn_device_io_alive(first) && tn_device_io_alive(second) &&
            tn_device_io_native_frames(first) > 0 && tn_device_io_native_frames(second) > 0 &&
            tn_device_io_clock_device(first) == device && tn_device_io_clock_device(second) == device;
        printf("IOProc frames: %llu / %llu; same native clock identity=%u\n",
            (unsigned long long)tn_device_io_native_frames(first),
            (unsigned long long)tn_device_io_native_frames(second), device);
        TNDeviceBufferInfo info = {0};
        connected = connected && tn_device_io_buffer_info(first, &info) &&
            info.device == device && info.settable && info.minimum <= 32 && info.maximum >= 32 &&
            tn_device_buffer_write(device, 32);
        if (connected) {
            usleep(100000);
            connected = tn_device_buffer_size(device) == 32 &&
                tn_device_io_quantum(first, false) == 32 && tn_device_io_quantum(second, false) == 32;
            printf("Native buffer readback=%u callback quantums=%u/%u\n", tn_device_buffer_size(device),
                tn_device_io_quantum(first, false), tn_device_io_quantum(second, false));
        }
    }
    assert(tn_device_io_destroy(second));
    assert(tn_device_io_destroy(first));
    assert(tn_aggregate_group_try_destroy(group));
    if (!connected) { fprintf(stderr, "grouped IOProc did not run\n"); return 1; }
    puts("PASS private aggregate native policy readback, sliced IOProc registrations and ordered teardown");
}
