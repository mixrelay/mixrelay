// Diagnostic controller: explicit lab endpoints only. Exercises the real driver
// transport; it is not the product engine, UI or a continuity/endurance benchmark.
#include <windows.h>
#include <audioclient.h>
#include <mmdeviceapi.h>
#include <ksmedia.h>
#include <wrl/client.h>
#include <cstdio>
#include <cmath>
#include <cwchar>
#include <memory>
#include "ToneNariTransport.h"
#include "TnBridgeDiagnostics.h"

using Microsoft::WRL::ComPtr;
static const char* stage = "startup";
static ULONG sampleRate = 48000;
static void Check(HRESULT status) { if (FAILED(status)) throw status; }
static void RequireAt(bool value, int line) {
    if (!value) { std::fprintf(stderr,"Invariant failed at line %d\n",line); throw E_UNEXPECTED; }
}
#define Require(value) RequireAt((value), __LINE__)
struct Handle {
    HANDLE value{};
    ~Handle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
    Handle() = default;
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
};
struct Apartment {
    Apartment() { Check(CoInitializeEx(nullptr,COINIT_MULTITHREADED)); }
    ~Apartment() { CoUninitialize(); }
};
struct TaskFree { void operator()(WAVEFORMATEX* p) const { CoTaskMemFree(p); } };
static ULONGLONG Counter(volatile LONG64* value) {
    return static_cast<ULONGLONG>(InterlockedCompareExchange64(value,0,0));
}
struct Mapping {
    // Events close first; file CLEANUP then unmaps pages. No pointer escapes this owner.
    Handle file, renderEvent, captureEvent;
    TONENARI_SHARED_AUDIO_HEADER* header{};
    float *render{}, *capture{};
    static constexpr ULONG Capacity = 4096;
    ULONGLONG read{}, write{}, matchedRender{}, settlingRender{};
    Mapping() {
        stage="open/map transport";
        file.value=CreateFileW(L"\\\\.\\ToneNariSysvadBridge",GENERIC_READ|GENERIC_WRITE,
            FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,0,nullptr);
        if (file.value==INVALID_HANDLE_VALUE) throw HRESULT_FROM_WIN32(GetLastError());
        TONENARI_TRANSPORT_REQUEST request{2,sizeof(request),2,1,sampleRate,Capacity};
        TONENARI_TRANSPORT_DESCRIPTOR descriptor{}; DWORD returned{};
        if (!DeviceIoControl(file.value,IOCTL_TONENARI_GET_TRANSPORT,&request,sizeof(request),
            &descriptor,sizeof(descriptor),&returned,nullptr)) throw HRESULT_FROM_WIN32(GetLastError());
        renderEvent.value=reinterpret_cast<HANDLE>(descriptor.RenderReadyEvent);
        captureEvent.value=reinterpret_cast<HANDLE>(descriptor.CaptureReadyEvent);
        Require(returned==sizeof(descriptor) && descriptor.Version==2 && descriptor.Size==sizeof(descriptor));
        Require(descriptor.UserAddress && descriptor.MappingBytes==53248);
        header=reinterpret_cast<TONENARI_SHARED_AUDIO_HEADER*>(descriptor.UserAddress);
        Require(header->Magic==TONENARI_TRANSPORT_MAGIC && header->Version==2 &&
            header->HeaderSize==sizeof(*header) && header->CapacityFrames==Capacity &&
            header->RenderChannels==2 && header->CaptureChannels==1 && header->SampleRate==sampleRate &&
            header->RenderSamplesOffset==4096 && header->CaptureSamplesOffset==36864 &&
            header->MappingBytes==descriptor.MappingBytes);
        // Use validated constants after handshake, never a live user-writable offset.
        render=reinterpret_cast<float*>(reinterpret_cast<BYTE*>(header)+4096);
        capture=reinterpret_cast<float*>(reinterpret_cast<BYTE*>(header)+36864);
    }
    void Process() {
        Require(InterlockedCompareExchange(&header->State,0,0)==TONENARI_TRANSPORT_ONLINE);
        const auto produced=Counter(&header->RenderWriteFrame);
        const auto consumed=Counter(&header->CaptureReadFrame);
        Require(produced>=read && produced-read<=Capacity && consumed<=write && write-consumed<=Capacity);
        const auto free=Capacity-(write-consumed);
        const auto count=(produced-read)<free ? produced-read : free;
        for (ULONGLONG i=0;i<count;++i) {
            const auto slot=(read+i)%Capacity;
            const float left=render[slot*2], right=render[slot*2+1];
            Require(std::isfinite(left) && std::isfinite(right));
            if (std::fabs(left-0.25f)<0.001f && std::fabs(right+0.125f)<0.001f) ++matchedRender;
            else if (read+i<sampleRate/2 && left>=0 && left<=0.251f &&
                right<=0 && right>=-0.126f && std::fabs(left+2*right)<0.001f) {
                // Shared-mode startup can ramp session gain. Bound this settling
                // allowance to 0.5 s and preserve polarity/channel ratio checks.
                // It never contributes to the required steady-state match count.
                ++settlingRender;
            } else if (!(std::fabs(left)<0.001f && std::fabs(right)<0.001f)) {
                std::fprintf(stderr,"Unexpected render frame=%llu left=%.9f right=%.9f\n",read+i,left,right);
                Require(false);
            }
            // Deliberate known mapping: stereo source channel 1 -> mono sink, -6 dB.
            capture[(write+i)%Capacity]=left*0.5f;
        }
        read+=count; write+=count;
        InterlockedExchange64(&header->RenderReadFrame,static_cast<LONG64>(read));
        InterlockedExchange64(&header->CaptureWriteFrame,static_cast<LONG64>(write));
    }
};
struct Client {
    ComPtr<IAudioClient> client;
    std::unique_ptr<WAVEFORMATEX,TaskFree> format;
    ComPtr<IAudioRenderClient> render;
    ComPtr<IAudioCaptureClient> capture;
    UINT32 capacity{}; bool started{};
    Client(IMMDeviceEnumerator* enumerator,const wchar_t* id,EDataFlow flow,WORD channels) {
        stage=flow==eRender ? "render device/format" : "capture device/format";
        ComPtr<IMMDevice> device; Check(enumerator->GetDevice(id,&device));
        ComPtr<IMMEndpoint> endpoint; Check(device.As(&endpoint));
        EDataFlow actual; Check(endpoint->GetDataFlow(&actual)); Require(actual==flow);
        Check(device->Activate(__uuidof(IAudioClient),CLSCTX_ALL,nullptr,&client));
        stage=flow==eRender ? "render GetMixFormat" : "capture GetMixFormat";
        WAVEFORMATEX* raw{}; Check(client->GetMixFormat(&raw)); format.reset(raw);
        std::printf("flow=%s mix_rate=%lu mix_channels=%u bits=%u tag=%u\n",
            flow==eRender ? "render" : "capture",raw->nSamplesPerSec,raw->nChannels,raw->wBitsPerSample,raw->wFormatTag);
        bool floating=raw->wFormatTag==WAVE_FORMAT_IEEE_FLOAT;
        if (raw->wFormatTag==WAVE_FORMAT_EXTENSIBLE && raw->cbSize>=22)
            floating=IsEqualGUID(reinterpret_cast<WAVEFORMATEXTENSIBLE*>(raw)->SubFormat,KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
        Require(floating && raw->wBitsPerSample==32 && raw->nSamplesPerSec==sampleRate &&
            raw->nChannels==channels && raw->nBlockAlign==channels*4);
        stage=flow==eRender ? "render Initialize" : "capture Initialize";
        Check(client->Initialize(AUDCLNT_SHAREMODE_SHARED,0,1000000,0,raw,nullptr));
        Check(client->GetBufferSize(&capacity));
        if (flow==eRender) Check(client->GetService(IID_PPV_ARGS(&render)));
        else Check(client->GetService(IID_PPV_ARGS(&capture)));
    }
    ~Client() { if (started) client->Stop(); }
    void Start() { Check(client->Start()); started=true; }
    void Stop() { Check(client->Stop()); started=false; }
    void Fill() {
        UINT32 padding{}; Check(client->GetCurrentPadding(&padding)); Require(padding<=capacity);
        const UINT32 count=capacity-padding; if (!count) return;
        BYTE* bytes{}; Check(render->GetBuffer(count,&bytes));
        auto samples=reinterpret_cast<float*>(bytes);
        for (UINT32 i=0;i<count;++i) { samples[i*2]=0.25f; samples[i*2+1]=-0.125f; }
        Check(render->ReleaseBuffer(count,0));
    }
    void Read(ULONGLONG& frames,ULONGLONG& matched,ULONG& discontinuities,ULONGLONG& settling) {
        for (ULONG packets=0;packets<128;++packets) {
            UINT32 count{}; Check(capture->GetNextPacketSize(&count)); if (!count) break;
            BYTE* data{}; DWORD flags{}; Check(capture->GetBuffer(&data,&count,&flags,nullptr,nullptr));
            bool valid=count<=capacity;
            if (!(flags&AUDCLNT_BUFFERFLAGS_SILENT)) {
                valid=valid && data!=nullptr;
                if (valid) for (UINT32 i=0;i<count;++i) {
                    const auto value=reinterpret_cast<float*>(data)[i];
                    if (std::isfinite(value) && std::fabs(value-0.125f)<0.001f) ++matched;
                    else if (frames+i<sampleRate/2 && std::isfinite(value) && value>=0 && value<=0.126f) ++settling;
                    else if (!std::isfinite(value) || std::fabs(value)>0.001f) {
                        if (valid) std::fprintf(stderr,"Unexpected capture frame=%llu value=%.9f flags=%lu\n",frames+i,value,flags);
                        valid=false;
                    }
                }
            }
            frames+=count; discontinuities+=(flags&AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY)!=0;
            Check(capture->ReleaseBuffer(count)); Require(valid);
        }
    }
};
struct RateRequest {
    Handle file;
    TONENARI_RATE_STATUS status{};
    RateRequest() {
        stage="rate policy begin";
        file.value=CreateFileW(L"\\\\.\\ToneNariSysvadBridge",GENERIC_READ|GENERIC_WRITE,
            FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,0,nullptr);
        if (file.value==INVALID_HANDLE_VALUE) throw HRESULT_FROM_WIN32(GetLastError());
        Command(TONENARI_RATE_QUERY); Command(TONENARI_RATE_BEGIN);
    }
    void Command(ULONG operation) {
        TONENARI_RATE_REQUEST request{TONENARI_RATE_POLICY_VERSION,sizeof(request),operation,
            sampleRate,status.Generation,10000,0};
        DWORD returned{};
        if (!DeviceIoControl(file.value,IOCTL_TONENARI_RATE_POLICY,&request,sizeof(request),
            &status,sizeof(status),&returned,nullptr)) throw HRESULT_FROM_WIN32(GetLastError());
        Require(returned==sizeof(status) && status.Version==TONENARI_RATE_POLICY_VERSION && status.Size==sizeof(status));
        std::printf("rate_policy state=%lu requested=%lu preferred=%lu render=%lu capture=%lu generation=%llu\n",
            status.State,status.RequestedRate,status.PreferredRate,status.RenderRate,status.CaptureRate,status.Generation);
    }
    void Confirm() {
        Command(TONENARI_RATE_QUERY);
        Require(status.State==TONENARI_RATE_STABLE && status.PreferredRate==sampleRate &&
            status.RenderRate==sampleRate && status.CaptureRate==sampleRate);
    }
};
static void Trace() {
    Handle file;
    file.value=CreateFileW(L"\\\\.\\ToneNariSysvadBridge",GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,
        nullptr,OPEN_EXISTING,0,nullptr);
    TnBridgeTrace trace{}; DWORD returned{};
    if (file.value==INVALID_HANDLE_VALUE || !DeviceIoControl(file.value,IOCTL_TN_BRIDGE_TRACE,
        nullptr,0,&trace,sizeof(trace),&returned,nullptr) || returned!=sizeof(trace) || trace.version!=1) {
        std::printf("native_trace_unavailable error=%lu\n",GetLastError()); return;
    }
    std::printf("native_trace_count=%lu\n",trace.count);
    const ULONG first=trace.count>256 ? trace.count-256 : 0;
    for (ULONG i=first;i<trace.count;++i) {
        const auto& a=trace.attempts[i%256];
        std::printf("native_attempt=%lu capture=%lu channels=%lu rate=%lu bits=%lu valid=%lu status=0x%08lX\n",
            i,a.capture,a.channels,a.rate,a.bits,a.validBits,a.status);
    }
}
int wmain(int argc,wchar_t** argv) {
    try {
        DWORD session{};
        Require(ProcessIdToSessionId(GetCurrentProcessId(),&session) && session);
        if (argc==2 && std::wcscmp(argv[1],L"--owner-exit")==0) {
            Mapping mapping;
            std::puts("Mapped: abrupt controller exit intentionally skips C++ cleanup.");
            std::fflush(stdout);
            // Test-only process death: kernel owner-exit cleanup must release the
            // mapping even though no application destructor closes its file.
            ExitProcess(0);
        }
        bool external=false, requestRate=false, explicitRate=false;
        Require(argc>=3);
        for (int arg=3;arg<argc;++arg) {
            if (std::wcscmp(argv[arg],L"--external-engine")==0) { Require(!external); external=true; }
            else if (std::wcscmp(argv[arg],L"--request-rate")==0) { Require(!requestRate); requestRate=true; }
            else if (std::wcscmp(argv[arg],L"--rate")==0) {
                Require(!explicitRate && ++arg<argc);
                Require(!std::wcscmp(argv[arg],L"44100") || !std::wcscmp(argv[arg],L"48000"));
                sampleRate=!std::wcscmp(argv[arg],L"44100") ? 44100u : 48000u; explicitRate=true;
            } else Require(false);
        }
        // --rate merely validates the existing native setting. A mutation needs
        // this second explicit flag, and cannot race an external engine owner.
        Require(!requestRate || (explicitRate && !external));
        Apartment apartment; ComPtr<IMMDeviceEnumerator> enumerator;
        Check(CoCreateInstance(__uuidof(MMDeviceEnumerator),nullptr,CLSCTX_ALL,IID_PPV_ARGS(&enumerator)));
        std::unique_ptr<RateRequest> policy;
        if (requestRate) policy=std::make_unique<RateRequest>();
        for (int cycle=0;cycle<3;++cycle) {
            // Let WASAPI negotiate its native stream before attaching the
            // controller. A pending controller must not choose GetMixFormat's
            // probing order or force a mismatched native stream format.
            std::unique_ptr<Mapping> owner;
            std::unique_ptr<Client> renderOwner,captureOwner;
            const auto negotiationStarted=GetTickCount64();
            for (;;) {
                try {
                    renderOwner=std::make_unique<Client>(enumerator.Get(),argv[1],eRender,2);
                    captureOwner=std::make_unique<Client>(enumerator.Get(),argv[2],eCapture,1);
                    if (policy) policy->Confirm();
                    break;
                } catch (HRESULT error) {
                    captureOwner.reset(); renderOwner.reset();
                    // Native notification/reopen is asynchronous. Bounded
                    // retries are allowed only for the explicitly requested
                    // transition; ordinary PCM tests retain fail-fast behavior.
                    if (!policy || cycle || GetTickCount64()-negotiationStarted>=8000) throw;
                    std::printf("renegotiation retry HRESULT=0x%08lX\n",static_cast<unsigned long>(error));
                    Sleep(100);
                }
            }
            auto& render=*renderOwner; auto& capture=*captureOwner;
            if (!external) owner=std::make_unique<Mapping>();
            render.Fill(); render.Start(); capture.Start();
            stage="native transfer/comparison";
            ULONGLONG frames{},matched{},settling{}; ULONG discontinuities{};
            const auto start=GetTickCount64();
            while (GetTickCount64()-start<3000) {
                render.Fill(); if (owner) owner->Process(); capture.Read(frames,matched,discontinuities,settling); Sleep(2);
            }
            capture.Stop(); render.Stop();
            if (external) std::printf("cycle=%d external_engine=1 capture_matched=%llu capture_frames=%llu capture_settling=%llu discontinuities=%lu driver_counters=engine_status\n",
                cycle,matched,frames,settling,discontinuities);
            else std::printf("cycle=%d render_matched=%llu capture_matched=%llu capture_frames=%llu render_settling=%llu capture_settling=%llu discontinuities=%lu render_overruns=%llu capture_underruns=%llu\n",
                cycle,owner->matchedRender,matched,frames,owner->settlingRender,settling,discontinuities,
                Counter(&owner->header->RenderOverruns),Counter(&owner->header->CaptureUnderruns));
            Require((external || owner->matchedRender>sampleRate) && matched>sampleRate);
        }
        std::puts(external ? "PASS: 3 external-engine WASAPI capture comparison cycles; inspect engine graph/status separately." :
            "PASS: 3 native WASAPI -> shared ring -> channel 1 * 0.5 -> shared ring -> WASAPI cycles. Test controller, NOT product engine qualification.");
        return 0;
    } catch (HRESULT error) { std::fprintf(stderr,"Bridge failure at %s: 0x%08lX\n",stage,static_cast<unsigned long>(error)); Trace(); return 1; }
}
