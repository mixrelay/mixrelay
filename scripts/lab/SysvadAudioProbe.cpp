// Native baseline smoke. Only explicit sample MMDevice IDs supplied by the lab
// runner are opened. No default device selection or unrelated application capture.
#include <windows.h>
#include <audioclient.h>
#include <mmdeviceapi.h>
#include <wrl/client.h>
#include <cstdio>
#include <memory>
#include <stdexcept>

using Microsoft::WRL::ComPtr;
static void Check(HRESULT status) { if (FAILED(status)) throw status; }
struct Apartment {
    Apartment() { Check(CoInitializeEx(nullptr, COINIT_MULTITHREADED)); }
    ~Apartment() { CoUninitialize(); }
};
struct TaskFree { void operator()(WAVEFORMATEX* p) const { CoTaskMemFree(p); } };
struct RunningClient {
    IAudioClient* client; bool started{};
    ~RunningClient() { if (started) client->Stop(); }
};

static void Probe(IMMDeviceEnumerator* enumerator, const wchar_t* id, EDataFlow expected) {
    ComPtr<IMMDevice> device;
    Check(enumerator->GetDevice(id, &device));
    ComPtr<IMMEndpoint> endpoint;
    Check(device.As(&endpoint));
    EDataFlow flow;
    Check(endpoint->GetDataFlow(&flow));
    if (flow != expected) throw E_INVALIDARG;
    ComPtr<IAudioClient> client;
    Check(device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, &client));
    WAVEFORMATEX* raw{};
    Check(client->GetMixFormat(&raw));
    std::unique_ptr<WAVEFORMATEX, TaskFree> format(raw);
    Check(client->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1000000, 0, raw, nullptr));
    UINT32 capacity{};
    Check(client->GetBufferSize(&capacity));
    ComPtr<IAudioRenderClient> render;
    ComPtr<IAudioCaptureClient> capture;
    ComPtr<IAudioClock> clock;
    Check(client->GetService(IID_PPV_ARGS(&clock)));
    UINT64 clockStart{}, clockEnd{};
    Check(clock->GetPosition(&clockStart, nullptr));
    if (flow == eRender) {
        Check(client->GetService(IID_PPV_ARGS(&render)));
        BYTE* data{};
        Check(render->GetBuffer(capacity, &data));
        Check(render->ReleaseBuffer(capacity, AUDCLNT_BUFFERFLAGS_SILENT));
    } else Check(client->GetService(IID_PPV_ARGS(&capture)));
    RunningClient running{client.Get()};
    Check(client->Start()); running.started=true;
    const ULONGLONG started=GetTickCount64();
    UINT64 frames{}, nonzeroBytes{};
    UINT32 discontinuities{};
    while (GetTickCount64()-started < 1500) {
        Sleep(5);
        if (render) {
            UINT32 padding{};
            Check(client->GetCurrentPadding(&padding));
            if (padding > capacity) throw E_UNEXPECTED;
            const UINT32 count=capacity-padding;
            if (count) {
                BYTE* data{};
                Check(render->GetBuffer(count,&data));
                Check(render->ReleaseBuffer(count,AUDCLNT_BUFFERFLAGS_SILENT));
                frames+=count;
            }
        } else {
            // A bounded polling smoke, not the production real-time processing loop.
            for (UINT32 packets=0; packets<128; ++packets) {
                UINT32 available{};
                Check(capture->GetNextPacketSize(&available));
                if (!available) break;
                BYTE* data{}; UINT32 count{}; DWORD flags{};
                Check(capture->GetBuffer(&data,&count,&flags,nullptr,nullptr));
                if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT)) {
                    const size_t bytes=size_t(count)*raw->nBlockAlign;
                    for (size_t i=0;i<bytes;++i) nonzeroBytes+=(data[i]!=0);
                }
                discontinuities+=(flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY)!=0;
                frames+=count;
                Check(capture->ReleaseBuffer(count));
            }
        }
    }
    Check(clock->GetPosition(&clockEnd,nullptr));
    Check(client->Stop()); running.started=false;
    std::printf("flow=%s rate=%lu channels=%u bits=%u frames=%llu nonzero_bytes=%llu clock_delta=%llu discontinuities=%u\n",
        flow==eRender ? "render" : "capture", raw->nSamplesPerSec,
        raw->nChannels,raw->wBitsPerSample,frames,nonzeroBytes,clockEnd-clockStart,discontinuities);
    if (clockEnd<=clockStart || frames<raw->nSamplesPerSec/2 || (capture && !nonzeroBytes)) throw E_FAIL;
}

int wmain(int argc,wchar_t** argv) {
    try {
        DWORD session{};
        if (argc!=3 || !ProcessIdToSessionId(GetCurrentProcessId(),&session) || !session) {
            std::fprintf(stderr,"Requires an interactive session and explicit render/capture sample endpoint IDs\n"); return 2;
        }
        Apartment apartment;
        ComPtr<IMMDeviceEnumerator> enumerator;
        Check(CoCreateInstance(__uuidof(MMDeviceEnumerator),nullptr,CLSCTX_ALL,IID_PPV_ARGS(&enumerator)));
        for (int cycle=0;cycle<3;++cycle) {
            Probe(enumerator.Get(),argv[1],eRender);
            Probe(enumerator.Get(),argv[2],eCapture);
        }
        std::puts("PASS: 3 render/capture lifecycle cycles. Capture is SysVAD's test tone, NOT MixRelay routed audio.");
        return 0;
    } catch (HRESULT status) { std::fprintf(stderr,"WASAPI failure 0x%08lX\n",static_cast<unsigned long>(status)); return 1; }
}
