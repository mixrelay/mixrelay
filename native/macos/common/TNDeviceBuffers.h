#ifndef TN_DEVICE_BUFFERS_H
#define TN_DEVICE_BUFFERS_H
#include <CoreAudio/CoreAudioTypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* Immutable channel selection; Rust validates metadata and owns PCM projection. */
typedef struct { uint32_t channels, offset, total, limit; } TNBufferSelection;
_Static_assert(sizeof(TNBufferSelection) == 16, "selection ABI");
_Static_assert(sizeof(AudioBuffer) == 16 && offsetof(AudioBufferList, mBuffers) == 8, "native buffer ABI");
uint32_t tn_device_buffers_frames(const AudioBufferList *, TNBufferSelection);
bool tn_device_buffers_transfer(const AudioBufferList *, TNBufferSelection, float *, bool input);
#endif
