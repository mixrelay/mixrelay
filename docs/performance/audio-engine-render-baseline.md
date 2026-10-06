# 성능 측정 기록

측정 조건과 전후 수치만 보존한다. 과거 구현의 측정은 현재 설치본의 통과 결과가 아니다.
현재 상태는 [제품 문서](../product.md#구현-상태)가 기준이다.

## 엔진·UI — 2026-09-02∼03

| 변경 | 동일 조건 | 이전 → 이후 | 한계 |
| --- | --- | --- | --- |
| 실행 계획의 정확한 길이 저장 | 같은 debug 프로세스·stereo schema-v2 fixture, 구조체·문자열 용량·설정·포트·경로 포함, allocator/외부 Arc 제외 | 2,531 → 1,787 bytes (−29.4%) | 필요한 typed 설정 보존으로 최종 3,219 bytes가 됨. 최종 감소로 주장하지 않음 |
| Core Audio 재시작 epoch | 설치 HAL·엔진, 48kHz·128프레임·stereo, 2,048 warmup 뒤 3초, frame/channel 기반 PCM·512프레임 offset | 실패 실행 30,712/281,600 (10.9%) → 수정 후 280,576/280,576·281,600/281,600·281,600/281,600 (100%) | 잘못된 callback 0. 장기 jitter 측정 아님 |
| UI 변경 객체 재사용 | 1,024노드·1,023케이블 중 중간 케이블 삭제, 전후 React Flow snapshot | 새 node 객체 1,024 → 0, 남은 edge 객체 1,022 → 0 | 제거 edge 1개·node 배열 갱신 0회. 프레임 시간 측정 아님 |

설치 엔진의 direct → −6.0206003dB Gain → direct 변경은 각 3초·281,600샘플이
일치했다(Gain 허용 오차 1e−6). 패키지 UI의 Gain revision 6·direct revision 7에서도
같은 결과, 잘못된 callback·underrun·overrun 0을 확인했다. 이전 고정 그래프에는 비교 가능한
실시간 전환이 없었다. 블록 경계 교환은 비차단 gate·소유권 교환이며 RT 할당·해제·
잠금·대기는 없다. 최악 전환 시간이나 전체 지연 개선은 측정하지 않았다.

### 준비된 저장량

RenderPlan은 제어 스레드에서 준비하며 BlockProcessor 생성 시 arena를 한 번 할당한다.
4프레임·두 source/Mixer/Gain/채널 교환/가상 I/O/sink fixture는 80 f32 = 320 bytes다.
macOS 128프레임 stereo arena 2,048 bytes와 입출력 scratch 2,048 bytes의 합계는
4,096 bytes다(shared ring·allocator 제외). 이전 설치 경로에 비교 가능한 비무음 출력이
없어 전후 지연 비교는 없다. 당시 offset 512프레임은 48kHz에서 10.667ms였다.

같은 양방향 16,384프레임 transport 용량에서 채널별 계산값:

| 채널 | bytes | 고정 64채널 대비 감소 |
| --- | ---: | ---: |
| 1 | 393,432 | 95.45% |
| 2 | 524,504 | 93.94% |
| 8 | 1,310,936 | 84.85% |
| 16 | 2,359,512 | 72.73% |
| 32 | 4,456,664 | 48.48% |
| 64 | 8,650,968 | — |

당시 worker의 128프레임·최대 64채널 scratch 두 개는 65,536 bytes였다.
채널 변경 시 재할당하지 않는다. 계산된 저장량이지 RSS·CPU 측정은 아니다.

## Linux FIFO 과다 읽기 수정 — 2026-09-06

대체된 FIFO 구현의 기록이다. queue 용량보다 큰 quantum을 끝까지 읽던 경로를
남은 byte 용량까지만 읽도록 수정했다. Ubuntu 24.04.4 ARM64 VM·PipeWire 1.0.5·
48kHz·DSP 128프레임·stereo direct·debug·Node 24.19.0에서 이전 3회 후 수정 3회를
직렬 실행했다. 매번 엔진을 새로 시작했으며 같은 Mac에 VM 4개가 실행 중이었다.
4초 440/880Hz·peak 0.125/0.0625 신호, left 절댓값 0.001 초과 비율과 활성 구간
내 최장 무음으로 비교했다. sine의 near-zero 때문에 비율 100%가 기준은 아니다.

| 실행 | 활성 비율: 이전 → 이후 | 최장 gap: 이전 → 이후 |
| --- | --- | --- |
| 1 | 50.4426% → 99.4063% | 1,146 → 120 frames |
| 2 | 50.5782% → 99.3237% | 1,276 → 123 frames |
| 3 | 50.6458% → 99.4896% | 1,273 → 1 frame |
| 중앙값 | 50.5782% → 99.4063% (+48.8281pp) | 26.521 → 2.500ms (−90.57%) |

통과 조건은 활성 비율 >97%·내부 gap <48프레임이었다. **수정 후에도 3회 중 2회 실패**했다.
4,096프레임 burst·1,024프레임 queue·128프레임 DSP·2/16/64채널 및 분할 byte 읽기
회귀는 통과했으나 upstream 전달·native clock·live 다채널을 검증한 것은 아니다.
후속 진단에서도 신호 시작 0.856/0.555초 뒤 124/119프레임 gap을 관측했다.
엔진 재사용으로 PCM이 섞인 실행과 실행 권한 preflight 실패는 이 비교에서 제외했다.
현재 1,024프레임 누락과 동일 원인으로 취급하지 않는다.

## Native owner migration measurements (2026-09-10)

### Callback clock

The unchanged `native/macos/common/tests/MRCallbackClockBench.c` harness ran
on the same arm64 Mac with Apple Clang 21.0.0 (`-O3 -std=c17 -Wall -Wextra
-Werror`), Rust 1.96 release defaults, no custom CPU target/RUSTFLAGS, and no
concurrent build/test jobs. Each executable recorded 2,000,000 contiguous
128-frame timestamps per pass, after one full warmup, for seven sequential
measured passes. Nanoseconds per record:

| Statistic | Previous inline C | Rust native entry |
| --- | ---: | ---: |
| Median | 8.523 | 8.534 |
| Minimum | 7.894 | 8.425 |
| Maximum | 8.546 | 8.636 |

Passes: C `[8.546, 8.523, 7.894, 8.367, 8.526, 8.509, 8.523]`;
Rust `[8.611, 8.636, 8.534, 8.472, 8.555, 8.515, 8.425]`.
The +0.13% median difference is below observed variation. This is not a
speedup or end-to-end latency result.

## Matched app-capture ring measurement

The unchanged `native/macos/application-capture/tests/MRAudioRingBench.c`
harness used the same arm64 Mac, Apple Clang 21.0.0
(`-O3 -std=c17 -Wall -Wextra -Werror`) and Rust 1.96 release profile, no
custom CPU/RUSTFLAGS or concurrent benchmark/build jobs. Capacity
was 1024; each case checked exact PCM over 20,000 write/read pairs per pass,
one full warmup and seven measured passes. Medians are nanoseconds per pair
from two complete sequential C/Rust comparisons:

| Channels / frames / input | C #1 | Rust #1 | C #2 | Rust #2 |
| --- | ---: | ---: | ---: | ---: |
| 2 / 32 / interleaved | 14.525 | 17.312 | 15.329 | 14.271 |
| 2 / 32 / planar | 39.837 | 74.610 | 41.379 | 35.748 |
| 2 / 256 / interleaved | 64.960 | 64.746 | 58.562 | 58.990 |
| 2 / 256 / planar | 320.279 | 264.652 | 293.017 | 241.671 |
| 8 / 32 / interleaved | 38.860 | 39.481 | 35.754 | 34.948 |
| 8 / 32 / planar | 80.544 | 86.306 | 74.904 | 77.677 |
| 8 / 256 / interleaved | 262.973 | 272.050 | 244.002 | 255.488 |
| 8 / 256 / planar | 698.981 | 706.202 | 649.323 | 628.371 |
| 64 / 32 / interleaved | 360.552 | 380.006 | 342.752 | 361.644 |
| 64 / 32 / planar | 781.302 | 789.296 | 729.210 | 773.371 |
| 64 / 256 / interleaved | 2471.471 | 2584.750 | 2961.279 | 2654.610 |
| 64 / 256 / planar | 6277.300 | 6526.192 | 6781.475 | 6549.860 |

Stereo/256 planar fell about 17% in both comparisons, but other cases
regressed or changed direction; the first stereo/32 planar run ranged
37.350–81.533 ns. Fixed eight-channel copy groups addressed an initial
scalar-loop regression without benchmark-only flags. No overall speedup,
no-regression guarantee or end-to-end latency gain is claimed.

## Matched device-queue measurement

The unchanged `native/pcm-ring/tests/MRPcmRingBench.c` harness used the same
arm64 Mac, Apple Clang 21 `-O3 -std=c17` and Rust 1.96 release defaults, no
custom CPU/RUSTFLAGS or concurrent jobs. Capacity was 16,384 with one
preloaded block; each pass checked final PCM after 20,000 write/read pairs.
Creation/destruction was outside timing; one warmup and seven timed passes
preceded each median. Two complete sequential comparisons, ns per pair:

| Channels / frames / prefill | C #1 | Rust #1 | C #2 | Rust #2 |
| --- | ---: | ---: | ---: | ---: |
| 2 / 32 / no | 17.650 | 26.400 | 19.400 | 15.950 |
| 2 / 32 / yes | 17.700 | 16.450 | 18.200 | 16.300 |
| 2 / 256 / no | 59.950 | 63.750 | 67.700 | 66.600 |
| 2 / 256 / yes | 65.150 | 66.600 | 64.600 | 61.800 |
| 8 / 32 / no | 49.450 | 45.650 | 43.000 | 45.850 |
| 8 / 32 / yes | 47.850 | 47.900 | 45.850 | 43.400 |
| 8 / 256 / no | 336.400 | 391.300 | 356.950 | 371.700 |
| 8 / 256 / yes | 384.900 | 399.800 | 363.650 | 387.200 |
| 64 / 32 / no | 382.400 | 402.250 | 372.900 | 371.950 |
| 64 / 32 / yes | 369.150 | 417.650 | 373.750 | 356.000 |
| 64 / 256 / no | 3444.400 | 3188.000 | 2724.200 | 2303.800 |
| 64 / 256 / yes | 2616.650 | 2238.250 | 2261.550 | 2297.600 |

The 8-channel/256-frame medians are slower in both comparisons (about
4–16% without prefill, 4–6% with it). This correctness/ownership change
does not claim an overall speedup or audio latency improvement. Old source is
recoverable from Git history; old executables were ignored local artifacts,
not compatibility implementations.

To reproduce current implementations, build `mixrelay-macos-transport` or
`mixrelay-pcm-ring` with normal release defaults and link the documented native
harness above. Previous implementations are in Git history, not runtime fallbacks.

## Resource exchange observations (2026-09-06)

At the same successful Linux resource-swap boundary, explicit retirement-box
allocations changed from one to zero. This was a static allocation-path comparison,
not a measured speedup. Native timing/profiling was deferred; cross-target Clippy
and ownership inspection were the available alternative evidence. macOS external
sample storage increased from 65,536 to 262,144 bytes for bounded 512-frame/64-channel
duplex support; this is a capacity cost, not a reduction.

The portable native process-cycle fixture measured zero heap allocations/frees over
10,000 prepared duplex cycles using a thread-local allocator. This excludes vendor
DSP and native driver allocation. Historical Ubuntu live continuity trials failed:
active fractions 0.992374/0.991989/0.994152, longest internal silence 368/240/112 frames,
and 160/159/159 input starvations including startup. Evidence was
`ubuntu-runtime-contract-live-20260906.log` in the external lab. These diagnostics
are neither current qualification nor a controlled performance comparison.
