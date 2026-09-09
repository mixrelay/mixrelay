#ifndef TN_OWN_DEVICE_BUFFER_H
#define TN_OWN_DEVICE_BUFFER_H

#include <stdbool.h>
#include <stdint.h>

/* Diagnostic probe support, not linked into the shared-memory engine runtime.
 * BufferFrameSize is this calling client's IO view of the exact ToneNari UID,
 * NOT the shared virtual device's global callback quantum. With no own IOProc,
 * changing it does not configure other applications' IO or the DSP worker.
 * Never selects a default device or an arbitrary hardware UID. */
uint32_t tn_own_device_buffer_frames(void);
bool tn_own_device_buffer_can_set(uint32_t frames);
bool tn_own_device_buffer_set(uint32_t frames);

#endif
