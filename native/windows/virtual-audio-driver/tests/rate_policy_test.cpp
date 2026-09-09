#include "ToneNariRatePolicy.h"
#include <cassert>
#include <iostream>
using tonenari::RatePolicy;
using tonenari::RateResult;
using tonenari::RateState;
int main() {
    assert(tonenari::IntersectRate(44100,44100,48000)==44100);
    assert(tonenari::IntersectRate(96000,96000,48000)==96000);
    assert(tonenari::IntersectRate(45000,47000,48000)==0);
    assert(tonenari::IntersectRate(44100,96000,44100)==44100);
    assert(tonenari::IntersectRate(44100,48000,48000,96000)==0);
    for (const auto render : {0u,44100u}) for (const auto capture : {0u,44100u}) {
        RatePolicy p;
        assert(p.begin(44100,0,100,5000,1,false,false)==RateResult::NoListener);
        assert(p.begin(44100,0,100,5000,1,true,true)==RateResult::Busy);
        assert(p.begin(44100,0,100,5000,1,true,false)==RateResult::Ok);
        assert(p.generation==1 && p.state==RateState::Pending && p.preferred==48000);
        assert(!p.mappingAllowed());
        assert(!p.accepts(48000,48000));
        assert(!p.accepts(44100,48000));
        assert(p.accepts(44100,0) && p.accepts(44100,44100));
        assert(p.begin(44100,1,200,5000,1,true,false)==RateResult::Ok && p.generation==1);
        assert(p.begin(96000,0,200,5000,1,true,false)==RateResult::Conflict);
        p.observe(render,capture);
        assert(p.state==RateState::Pending); // open pins are not OS mix readback
        assert(p.confirm(44100,0,1,render,capture)==RateResult::Conflict);
        assert(p.confirm(44100,1,2,render,capture)==RateResult::Unauthorized);
        assert(p.confirm(48000,1,1,render,capture)==RateResult::Invalid);
        assert(p.confirm(44100,1,1,48000,capture)==RateResult::Busy);
        assert(p.confirm(44100,1,1,render,48000)==RateResult::Busy);
        assert(p.state==RateState::Pending && p.owner==1 && p.generation==1);
        assert(p.confirm(44100,1,1,render,capture)==RateResult::Ok);
        assert(p.state==RateState::Stable && p.preferred==44100 && p.owner==0);
        assert(p.mappingAllowed()); // idle, each one-way, and both-active cases
        assert(p.confirm(44100,1,1,render,capture)==RateResult::Ok && p.generation==1);
        assert(!p.cancel(1) && !p.expire(~0ull));
    }
    for (bool timeout : {false,true}) {
        RatePolicy p;
        assert(p.begin(44100,0,100,5000,1,true,false)==RateResult::Ok);
        assert(!p.cancel(2));
        if (timeout) {
            assert(!p.expire(50000099)); assert(p.expire(50000100));
        } else assert(p.cancel(1)); // owner-close/death invokes this exact path
        assert(p.state==RateState::RollbackPending && p.forced()==48000 && p.generation==2 && !p.owner);
        assert(!p.expire(~0ull)); // one rollback notification
        p.observe(48000,48000); assert(p.state==RateState::RollbackPending);
        assert(p.confirm(44100,1,1,0,0)==RateResult::Invalid);
        assert(p.confirm(48000,1,9,0,0)==RateResult::Conflict);
        assert(p.confirm(48000,2,0,0,0)==RateResult::Invalid);
        assert(p.confirm(48000,2,9,44100,0)==RateResult::Busy);
        assert(p.confirm(48000,2,9,0,44100)==RateResult::Busy);
        assert(p.state==RateState::RollbackPending && !p.mappingAllowed());
        // NEW authorized controller confirms the restored OS formats, without pins.
        assert(p.confirm(48000,2,9,0,0)==RateResult::Ok);
        assert(p.state==(timeout ? RateState::TimedOut : RateState::Cancelled));
        assert(p.preferred==48000 && !p.forced());
        assert(p.begin(48000,2,100000000,5000,9,true,false)==RateResult::Ok);
        assert(p.state==RateState::Stable && p.generation==2);
        assert(p.begin(12345,2,0,5000,1,true,false)==RateResult::Invalid);
        assert(p.begin(44100,2,~0ull,5000,1,true,false)==RateResult::Invalid);
    }
    std::cout << "rate policy: idle/one-way/duplex, fencing, timeout and owner-death rollback passed\n";
}
