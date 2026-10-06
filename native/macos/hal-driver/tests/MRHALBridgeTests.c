/* The Rust owner tests control-resource lifetime and reconnect behavior.
 * This fixture exercises the retained SDK COM adapter without starting the HAL. */
#include <assert.h>
#include <stdio.h>
#include "../driver/MixRelayHALBridge.c"

/* Keep the Rust property's stable FourCC projection tied to the active SDK. */
_Static_assert(kAudioVolumeControlClassID == UINT32_C(0x766c6d65), "volume class ABI");
_Static_assert(kAudioMuteControlClassID == UINT32_C(0x6d757465), "mute class ABI");
_Static_assert(kAudioLevelControlClassID == UINT32_C(0x6c65766c), "level base ABI");
_Static_assert(kAudioBooleanControlClassID == UINT32_C(0x746f676c), "boolean base ABI");
_Static_assert(kAudioLevelControlPropertyScalarValue == UINT32_C(0x6c637376), "scalar ABI");
_Static_assert(kAudioLevelControlPropertyDecibelValue == UINT32_C(0x6c636476), "decibel ABI");
_Static_assert(kAudioLevelControlPropertyDecibelRange == UINT32_C(0x6c636472), "dB range ABI");
_Static_assert(kAudioLevelControlPropertyConvertScalarToDecibels == UINT32_C(0x6c637364), "scalar conversion ABI");
_Static_assert(kAudioLevelControlPropertyConvertDecibelsToScalar == UINT32_C(0x6c636473), "dB conversion ABI");
_Static_assert(kAudioBooleanControlPropertyValue == UINT32_C(0x6263766c), "mute value ABI");
_Static_assert(kAudioControlPropertyScope == UINT32_C(0x63736370), "control scope ABI");
_Static_assert(kAudioControlPropertyElement == UINT32_C(0x63656c6d), "control element ABI");
_Static_assert(kAudioFormatFlagIsFloat == 1U, "Float32 ASBD flag ABI");
_Static_assert(kAudioFormatFlagIsSignedInteger == 4U, "signed integer ASBD flag ABI");
_Static_assert(kAudioFormatFlagIsPacked == 8U, "packed PCM ASBD flag ABI");

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
