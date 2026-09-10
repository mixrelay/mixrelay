// Explicit lab control utility. Default is read-only; requesting a rate requires
// --rate. Acceptance is never reported as success before both native pins agree.
#include "../virtual-audio-driver/include/MixRelayTransport.h"
#include <cstdio>
#include <cwchar>

int wmain(int argc, wchar_t** argv) {
    ULONG rate = 0;
    if (argc != 1) {
        if (argc != 3 || wcscmp(argv[1], L"--rate") ||
            (wcscmp(argv[2], L"44100") && wcscmp(argv[2], L"48000"))) {
            std::fputs("usage: rate-policy-probe [--rate 44100|48000]\n", stderr);
            return 2;
        }
        rate = !wcscmp(argv[2], L"44100") ? 44100u : 48000u;
    }
    const auto device = CreateFileW(L"\\\\.\\MixRelaySysvadBridge", GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING, 0, nullptr);
    if (device == INVALID_HANDLE_VALUE) { std::printf("open error=%lu\n", GetLastError()); return 3; }
    MIXRELAY_RATE_STATUS status{};
    auto command = [&](ULONG operation) {
        MIXRELAY_RATE_REQUEST request{MIXRELAY_RATE_POLICY_VERSION, sizeof(request), operation, rate,
            status.Generation, 5000, 0};
        DWORD received = 0;
        if (!DeviceIoControl(device, IOCTL_MIXRELAY_RATE_POLICY, &request, sizeof(request),
            &status, sizeof(status), &received, nullptr)) {
            std::printf("ioctl operation=%lu error=%lu\n", operation, GetLastError()); return false;
        }
        if (received != sizeof(status) || status.Size != sizeof(status) ||
            status.Version != MIXRELAY_RATE_POLICY_VERSION || status.State > 4) return false;
        std::printf("state=%lu preferred=%lu requested=%lu render=%lu capture=%lu generation=%llu\n",
            status.State, status.PreferredRate, status.RequestedRate, status.RenderRate, status.CaptureRate,
            status.Generation);
        return true;
    };
    bool success = command(MIXRELAY_RATE_QUERY);
    if (success && rate) {
        success = command(MIXRELAY_RATE_BEGIN);
        const auto started = GetTickCount64();
        while (success && (status.State == 1 || status.State == 2) && GetTickCount64() - started < 12000) {
            Sleep(100); success = command(MIXRELAY_RATE_QUERY);
        }
        success = success && status.State == 0 && status.PreferredRate == rate &&
            status.RenderRate == rate && status.CaptureRate == rate;
        if (!success) std::fputs("Native format agreement NOT confirmed; closing owner rolls pending request back.\n", stderr);
    }
    CloseHandle(device);
    return success ? 0 : 4;
}
