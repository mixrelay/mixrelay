# Native 성능 측정 절차

제품 범위와 진행 상태는 [제품 문서](../product.md)가 기준이다. 아래 harness는 native 진입점의 비용을 비교하며 장치 실행이나 전체 오디오 지연을 측정하지 않는다.

## 측정 대상

| Harness | 부하와 확인 항목 |
| --- | --- |
| [MRCallbackClockBench.c](../../native/macos/common/tests/MRCallbackClockBench.c) | 128프레임 간격의 연속 timestamp 2,000,000개. 마지막 snapshot과 discontinuity 0 확인. 단위는 ns/record |
| [MRAudioRingBench.c](../../native/macos/application-capture/tests/MRAudioRingBench.c) | 용량 1,024, 2/8/64채널 × 32/256프레임 × interleaved/planar 입력. 20,000 write/read 쌍 뒤 PCM·빈 queue·drop 0 확인. 생성/해제는 계측 제외. 단위는 ns/pair |
| [MRPcmRingBench.c](../../native/pcm-ring/tests/MRPcmRingBench.c) | 용량 16,384, 한 블록 사전 적재, 2/8/64채널 × 32/256프레임 × 두 읽기 경로. 20,000 write/read 쌍 뒤 PCM 확인. 생성/해제는 계측 제외. 단위는 ns/pair |

## 비교 방법

1. 같은 기기·OS·컴파일러와 최적화 조건을 사용하고 동시 build/test 작업을 멈춘다. 비교하는 소스 revision과 도구 버전을 기록한다.
2. 현재 Rust 구현은 `mixrelay-macos-transport` 또는 `mixrelay-pcm-ring`의 기본 release 설정으로 빌드하고 해당 native harness에 연결한다. C harness의 기준 옵션은 `-O3 -std=c17 -Wall -Wextra -Werror`다. 한쪽에만 custom CPU target이나 RUSTFLAGS를 적용하지 않는다.
3. 각 harness의 한 전체 warmup과 일곱 측정 pass를 사용한다. 최소·중앙값·최대값을 함께 비교하고, 전체 비교를 직렬로 반복해 실행 간 편차를 확인한다.
4. PCM·snapshot assertion을 유지한다. correctness 확인이 실패한 실행은 성능 통과로 취급하지 않는다.
5. 이전 구현과 비교할 때는 Git history의 같은 workload를 사용한다. 과거 실행 파일이나 호환 fallback을 성능 기준으로 유지하지 않는다.

저장량 계산, allocation 경로 검사, native 진입점 시간, 설치된 장치의 연속 PCM·지연 측정은 서로 다른 근거다. 일부 채널·블록 크기의 개선만으로 전체 성능 향상이나 회귀 없음을 주장하지 않는다.
