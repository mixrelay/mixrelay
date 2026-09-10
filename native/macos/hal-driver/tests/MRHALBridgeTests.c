/* The Rust owner tests control-resource lifetime and reconnect behavior.
 * This fixture exercises the retained SDK COM adapter without starting the HAL. */
#include <assert.h>
#include <stdio.h>
#include "../driver/MixRelayHALBridge.c"

int main(void) {
    assert(MixRelayHAL_Create(NULL, NULL) == NULL);
    assert(MixRelayHAL_Create(NULL, kAudioServerPlugInTypeUUID) == gDriver);
    assert(add_ref(NULL) == 0 && release_ref(NULL) == 0);
    assert(initialize(gDriver, NULL) == kAudioHardwareIllegalOperationError);
    assert(gCore == NULL);
    void *output = NULL;
    assert(query(gDriver, CFUUIDGetUUIDBytes(IUnknownUUID), &output) == S_OK);
    assert(output == gDriver);
    assert(release_ref(gDriver) == 1);
    assert(query(gDriver, CFUUIDGetUUIDBytes(kAudioServerPlugInDriverInterfaceUUID), &output) == S_OK);
    assert(output == gDriver);
    assert(release_ref(gDriver) == 1);
    CFUUIDRef unknown = CFUUIDCreate(NULL);
    assert(unknown != NULL);
    assert(query(gDriver, CFUUIDGetUUIDBytes(unknown), &output) == E_NOINTERFACE);
    assert(output == NULL);
    CFRelease(unknown);
    assert(release_ref(gDriver) == 0);
    assert(release_ref(gDriver) == 0);
    assert(add_ref(gDriver) == 1);
    puts("PASS current HAL COM factory, interface query and singleton reference lifetime");
    return 0;
}
