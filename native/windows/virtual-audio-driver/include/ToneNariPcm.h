#pragma once
#include "ToneNariRing.h"

namespace tonenari {
// DPC-safe IEEE-754 bit conversion. No floating-point instructions, allocation,
// unaligned typed loads or dependence on the caller's floating-point state.
// Integer PCM is signed little-endian, valid bits left-aligned in its container.
struct PcmFormat {
    uint32_t bytes, validBits; // validBits == 0 means IEEE float32 transport bits.
    bool valid() const {
        return validBits == 0 ? bytes == 4 :
            (bytes >= 2 && bytes <= 4 && validBits >= 2 && validBits <= bytes * 8);
    }
};
inline uint32_t LoadLe(const unsigned char* data, uint32_t bytes) {
    uint32_t value = 0;
    for (uint32_t i = 0; i < bytes; ++i) value |= uint32_t(data[i]) << (i * 8);
    return value;
}
inline void StoreLe(unsigned char* data, uint32_t bytes, uint32_t value) {
    for (uint32_t i = 0; i < bytes; ++i) data[i] = static_cast<unsigned char>(value >> (i * 8));
}
inline uint32_t ToFloatBits(uint32_t value, PcmFormat format) {
    if (!format.validBits) return value;
    const uint32_t bits = format.validBits;
    value >>= format.bytes * 8 - bits;
    const uint32_t sign = (value >> (bits - 1)) & 1u;
    const uint32_t mask = 0xffffffffu >> (32 - bits);
    uint32_t magnitude = sign ? ((~value + 1) & mask) : value;
    if (!magnitude) return 0;
    uint32_t top = 0;
    for (uint32_t n = magnitude; n >>= 1;) ++top;
    uint32_t significand;
    if (top <= 23) significand = magnitude << (23 - top);
    else {
        const uint32_t shift = top - 23;
        significand = magnitude >> shift;
        const uint32_t remainder = magnitude & ((1u << shift) - 1);
        const uint32_t halfway = 1u << (shift - 1);
        if (remainder > halfway || (remainder == halfway && (significand & 1))) ++significand;
        if (significand == 0x1000000u) { significand >>= 1; ++top; }
    }
    return (sign << 31) | ((top + 128 - bits) << 23) | (significand & 0x7fffffu);
}
inline uint32_t FromFloatBits(uint32_t value, PcmFormat format) {
    if (!format.validBits) return value;
    const uint32_t sign = value >> 31, exponent = (value >> 23) & 255;
    const uint32_t fraction = value & 0x7fffff;
    // NaN is silence; infinities and out-of-range finite samples saturate.
    if (exponent == 255 && fraction) return 0;
    const uint32_t limit = 1u << (format.validBits - 1);
    uint32_t magnitude = 0;
    if (exponent >= 127) magnitude = sign ? limit : limit - 1;
    else if (exponent) {
        const int shift = int(exponent) - 127 + int(format.validBits) - 24;
        const uint32_t significand = fraction | 0x800000;
        magnitude = shift >= 0 ? significand << shift :
            (shift <= -32 ? 0 : significand >> -shift);
    }
    // Quantize toward zero; lower unused container bits are always cleared.
    return (sign ? 0u - magnitude : magnitude) << (format.bytes * 8 - format.validBits);
}
inline void CopyPcm(bool capture, unsigned char* ring, void* pcm, const RingPlan& plan,
    uint32_t frames, uint32_t channels, PcmFormat format) {
    auto data = static_cast<unsigned char*>(pcm);
    for (uint32_t frame = 0; frame < plan.count(); ++frame) {
        const uint32_t slot = frame < plan.first ? plan.offset + frame : frame - plan.first;
        for (uint32_t channel = 0; channel < channels; ++channel) {
            auto r = ring + (size_t(slot) * channels + channel) * 4;
            auto p = data + (size_t(frame) * channels + channel) * format.bytes;
            if (capture) StoreLe(p, format.bytes, FromFloatBits(LoadLe(r, 4), format));
            else StoreLe(r, 4, ToFloatBits(LoadLe(p, format.bytes), format));
        }
    }
    if (capture) memset(data + size_t(plan.count()) * channels * format.bytes, 0,
        size_t(frames - plan.count()) * channels * format.bytes);
}
}
