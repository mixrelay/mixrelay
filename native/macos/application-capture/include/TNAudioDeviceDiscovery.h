#ifndef TN_AUDIO_DEVICE_DISCOVERY_H
#define TN_AUDIO_DEVICE_DISCOVERY_H

#include "TNProcessTap.h"

typedef void (*TNAudioDeviceVisitor)(
    void *context,
    const char *uid,
    const char *display_name,
    uint32_t input_channels,
    uint32_t output_channels,
    bool is_default_input,
    bool is_default_output
);

TNProcessTapError tn_audio_device_visit(
    TNAudioDeviceVisitor visitor,
    void *context
);

#endif
