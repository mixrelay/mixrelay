#pragma once

// Allocation-free policy shared by kernel control code and portable sanitizer
// tests. Caller serializes all access. Time is monotonic 100-nanosecond ticks.
namespace tonenari {
enum class RateState : unsigned int { Stable, Pending, RollbackPending, TimedOut, Cancelled };
enum class RateResult { Ok, Invalid, Conflict, Busy, NoListener, Unauthorized };
struct RatePolicy {
    unsigned int preferred = 48000, requested = 0, previous = 48000;
    unsigned long long generation = 0, deadline = 0, owner = 0;
    RateState state = RateState::Stable;

    static bool supported(unsigned int rate) { return rate == 44100 || rate == 48000 || rate == 96000; }
    unsigned int forced() const {
        return state == RateState::Pending || state == RateState::RollbackPending ? requested : 0;
    }
    unsigned int selected() const { return forced() ? requested : preferred; }
    bool accepts(unsigned int rate, unsigned int otherPinRate) const {
        return supported(rate) && (!forced() || rate == requested) && (!otherPinRate || otherPinRate == rate);
    }
    bool mappingAllowed() const { return forced() == 0; }
    RateResult begin(unsigned int rate, unsigned long long expected, unsigned long long now,
        unsigned int timeoutMs, unsigned long long file, bool listeners, bool mapped) {
        if (!supported(rate) || timeoutMs < 100 || timeoutMs > 30000 || !file ||
            now > ~0ull - static_cast<unsigned long long>(timeoutMs) * 10000) return RateResult::Invalid;
        if (expected != generation) return RateResult::Conflict;
        if (generation >= ~0ull - 1) return RateResult::Conflict;
        // The same transaction is idempotent, not another format-change event.
        if (state == RateState::Pending && rate == requested && owner == file) return RateResult::Ok;
        if (state == RateState::Pending || state == RateState::RollbackPending || mapped) return RateResult::Busy;
        if (!listeners) return RateResult::NoListener;
        if (rate == preferred) {
            // An explicit retry of the restored format acknowledges a previous
            // timeout/cancel. No new format generation or event is necessary;
            // callers still must check BOTH actual native rates in the reply.
            state = RateState::Stable;
            return RateResult::Ok;
        }
        previous = preferred; requested = rate; owner = file;
        deadline = now + static_cast<unsigned long long>(timeoutMs) * 10000;
        state = RateState::Pending; ++generation;
        return RateResult::Ok;
    }
    bool cancel(unsigned long long file, bool timeout = false) {
        if (state != RateState::Pending || (!timeout && file != owner)) return false;
        requested = previous; preferred = previous; deadline = 0; owner = 0;
        state = RateState::RollbackPending; ++generation;
        timeoutRollback = timeout;
        return true;
    }
    bool expire(unsigned long long now) {
        return state == RateState::Pending && now >= deadline && cancel(owner, true);
    }
    // The authorized controller confirms only after reading BOTH own MMDevice
    // mix formats, without opening streams. Kernel pins are a separate active
    // safety check: absent pins remain zero and are not fabricated as negotiated.
    RateResult confirm(unsigned int rate, unsigned long long expected, unsigned long long file,
        unsigned int render, unsigned int capture) {
        if (!file || !supported(rate) || rate != selected()) return RateResult::Invalid;
        if (expected != generation) return RateResult::Conflict;
        if (state == RateState::Pending && file != owner) return RateResult::Unauthorized;
        if ((render && render != rate) || (capture && capture != rate)) return RateResult::Busy;
        if (state == RateState::Pending) {
            preferred = requested; requested = 0; deadline = 0; owner = 0; state = RateState::Stable;
        } else if (state == RateState::RollbackPending) {
            // The old owner may have died. Another ACL-authorized controller can
            // confirm the current rollback generation, but cannot change its rate.
            requested = 0; state = timeoutRollback ? RateState::TimedOut : RateState::Cancelled;
        }
        return RateResult::Ok;
    }
    // Open pins alone do not prove the OS mix format. Pending/rollback needs the
    // explicit control-plane acknowledgement, including idle and one-way use.
    void observe(unsigned int render, unsigned int capture) {
        if (!render || render != capture) return;
        if (!forced() && supported(render) && render != preferred) {
            preferred = render; ++generation; state = RateState::Stable;
        }
    }
private:
    bool timeoutRollback = false;
};

// Intersect the exact supported set, not its bounding interval. A forced policy
// rate must match, while ordinary discovery prefers current format then 48k.
inline unsigned int IntersectRate(unsigned int low, unsigned int high,
    unsigned int preferred, unsigned int forced = 0) {
    if (low > high) return 0;
    if (forced) return RatePolicy::supported(forced) && low <= forced && forced <= high ? forced : 0;
    const unsigned int candidates[] = {preferred, 48000, 44100, 96000};
    for (auto rate : candidates) if (RatePolicy::supported(rate) && low <= rate && rate <= high) return rate;
    return 0;
}
}
