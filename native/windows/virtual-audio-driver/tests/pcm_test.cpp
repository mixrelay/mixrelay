#include "ToneNariPcm.h"
#include <cassert>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <random>
#include <vector>
using namespace tonenari;
static uint32_t Bits(float value) { uint32_t bits; std::memcpy(&bits, &value, 4); return bits; }
static float Float(uint32_t bits) { float value; std::memcpy(&value, &bits, 4); return value; }
int main() {
    std::mt19937 random(0x544e);
    size_t cases = 0;
    for (auto format : {PcmFormat{2,16}, {3,20}, {3,24}, {4,24}, {4,32}}) {
        assert(format.valid());
        for (uint32_t i = 0; i < 100000; ++i) {
            uint32_t value = i < 65536 && format.bytes == 2 ? i : random();
            const auto containerBits = format.bytes * 8;
            value &= 0xffffffffu >> (32-containerBits);
            value &= 0xffffffffu << (containerBits-format.validBits);
            const int64_t signedValue = (value & (1u << (containerBits-1))) ?
                int64_t(value) - (int64_t(1) << containerBits) : int64_t(value);
            const float expected = static_cast<float>(std::ldexp(double(signedValue), 1-int(containerBits)));
            assert(ToFloatBits(value,format) == Bits(expected));
            const uint32_t source = random(); const float sample = Float(source);
            const double limit = std::ldexp(1., int(format.validBits)-1);
            const int64_t quantized = std::isnan(sample) ? 0 : sample <= -1 ? -int64_t(limit) :
                sample >= 1 ? int64_t(limit)-1 : static_cast<int64_t>(double(sample)*limit);
            const uint32_t encoded = uint32_t(quantized) << (containerBits-format.validBits);
            assert(FromFloatBits(source,format) == encoded);
            ++cases;
        }
        for (uint32_t channels : {1u,2u,8u,16u,32u,64u}) {
            std::vector<unsigned char> ring(256*channels*4,0), pcm(8*channels*format.bytes,0);
            for (uint32_t i=0;i<8*channels;++i) StoreLe(pcm.data()+i*format.bytes,format.bytes,
                FromFloatBits(Bits(i%2 ? -0.25f : 0.5f),format));
            const auto write = Plan(false,254,254,254,256,8);
            CopyPcm(false,ring.data(),pcm.data(),write,8,channels,format);
            const auto expected = pcm;
            std::fill(pcm.begin(),pcm.end(),static_cast<unsigned char>(0xff));
            CopyPcm(true,ring.data(),pcm.data(),Plan(true,254,262,254,256,8),8,channels,format);
            assert(pcm==expected);
            CopyPcm(true,ring.data(),pcm.data(),{},8,channels,format);
            for (auto byte : pcm) assert(!byte);
        }
    }
    assert((!PcmFormat{1,8}.valid())); assert((!PcmFormat{4,33}.valid()));
    assert((!PcmFormat{2,0}.valid()));
    std::printf("PASS: %zu integer/float conversion cases and wrapped 1-64 channel copies\n",cases);
}
