#pragma once
#if !defined(_NTDDK_) && !defined(_WDMDDK_)
#include <stdint.h>
#include <string.h>
#endif

// Shared by kernel production code and user-mode sanitizer tests. Only trusted
// layout values enter here; peer counters are the untrusted input. No allocation.
namespace mixrelay {
#if defined(_NTDDK_) || defined(_WDMDDK_)
using uint32_t = unsigned int;
using uint64_t = unsigned long long;
#endif
struct RingPlan {
    bool valid;
    uint32_t offset, first, second;
    uint32_t count() const { return first + second; }
};
inline RingPlan Plan(bool capture, uint64_t owned, uint64_t peer,
    uint64_t previousPeer, uint32_t capacity, uint32_t requested) {
    if (!capacity || capacity > 16384 || peer < previousPeer ||
        peer > 0x7fffffffffffffffull || owned > 0x7fffffffffffffffull - requested) return {};
    if (capture ? (peer < owned || peer - owned > capacity)
                : (peer > owned || owned - peer > capacity)) return {};
    const auto available = uint32_t(capture ? peer - owned : capacity - (owned - peer));
    const auto count = requested < available ? requested : available;
    const auto offset = uint32_t(owned % capacity);
    const auto first = count < capacity - offset ? count : capacity - offset;
    return {true, offset, first, count - first};
}
inline void Copy(bool capture, unsigned char* ring, void* pcm, const RingPlan& plan,
    uint32_t frames, uint32_t channels) {
    const size_t frameBytes = size_t(channels) * sizeof(float);
    const size_t first = plan.first * frameBytes, second = plan.second * frameBytes;
    auto data = static_cast<unsigned char*>(pcm);
    if (capture) {
        memcpy(data, ring + plan.offset * frameBytes, first);
        memcpy(data + first, ring, second);
        memset(data + first + second, 0, (frames - plan.count()) * frameBytes);
    } else {
        memcpy(ring + plan.offset * frameBytes, data, first);
        memcpy(ring, data + first, second);
    }
}
}
