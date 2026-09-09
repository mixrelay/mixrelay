#include "ToneNariRing.h"
#include <cassert>
#include <cstdio>
#include <vector>

int main() {
    using tonenari::Plan;
    assert(!Plan(false, 0, 1, 0, 256, 32).valid);
    assert(!Plan(true, 1000, 999, 0, 256, 32).valid);
    assert(!Plan(true, 0, 257, 0, 256, 32).valid);
    assert(!Plan(false, 1000, 500, 0, 256, 32).valid);
    assert(!Plan(false, 1000, 900, 950, 256, 32).valid);
    assert(!Plan(true, 1000, 1100, 1200, 256, 32).valid);
    assert(!Plan(true, 0, UINT64_MAX, 0, 256, 32).valid);
    assert(!Plan(false, UINT64_MAX, 0, 0, 256, 32).valid);
    assert(!Plan(false, 0, 0, 0, 0, 32).valid);
    assert(!Plan(false, 0, 0, 0, 16385, 32).valid);
    unsigned long cases = 0;
    for (uint32_t channels = 1; channels <= 64; ++channels) {
        for (uint32_t capacity : {256u, 257u, 4096u, 16384u}) {
            for (uint32_t offset : {0u, 1u, capacity / 2, capacity - 1}) {
                for (uint32_t frames : {1u, 17u, capacity, capacity + 7}) {
                    const uint64_t position = uint64_t(capacity) * 3 + offset;
                    auto write = Plan(false, position, position, position, capacity, frames);
                    assert(write.valid && write.count() == (frames < capacity ? frames : capacity));
                    assert(write.offset + write.first <= capacity && write.second <= capacity);
                    // Sentinel frames on both allocations expose any off-by-one
                    // copy, also under ASan/UBSan. Verify actual multi-channel PCM.
                    std::vector<float> storage(size_t(capacity + 2) * channels, -123.0f);
                    auto ring = reinterpret_cast<unsigned char*>(storage.data() + channels);
                    std::vector<float> input(size_t(frames) * channels);
                    for (size_t i = 0; i < input.size(); ++i) input[i] = float(i % 1024) / 1024;
                    tonenari::Copy(false, ring, input.data(), write, frames, channels);
                    auto read = Plan(true, position, position + write.count(), position, capacity, frames);
                    std::vector<float> output(size_t(frames + 2) * channels, -456.0f);
                    tonenari::Copy(true, ring, output.data() + channels, read, frames, channels);
                    for (size_t i = 0; i < size_t(frames) * channels; ++i)
                        assert(output[channels + i] == (i < size_t(write.count()) * channels ? input[i] : 0.0f));
                    for (uint32_t c = 0; c < channels; ++c) {
                        assert(storage[c] == -123.0f && storage[size_t(capacity + 1) * channels + c] == -123.0f);
                        assert(output[c] == -456.0f && output[size_t(frames + 1) * channels + c] == -456.0f);
                    }
                    // Malformed producer counters must zero capture, and never
                    // let a renderer overwrite even a single transport sample.
                    auto invalid = Plan(true, position, UINT64_MAX, 0, capacity, frames);
                    tonenari::Copy(true, ring, output.data() + channels, invalid, frames, channels);
                    for (size_t i = channels; i < size_t(frames + 1) * channels; ++i) assert(output[i] == 0);
                    auto before = storage;
                    tonenari::Copy(false, ring, input.data(), invalid, frames, channels);
                    assert(storage == before);
                    ++cases;
                }
            }
        }
    }
    std::printf("PASS: %lu multi-channel wrap/underflow/corrupt-counter cases (1-64 channels)\n", cases);
}
