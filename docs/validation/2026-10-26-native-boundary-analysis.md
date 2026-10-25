# 설치본의 네이티브 경계 실패 분석

대상은 `71f49210ba262771d4e37815b2496f07be7129a0`의 설치본이며,
전체 실행은 [40207863718](https://github.com/mixrelay/mixrelay/actions/runs/40207863718)이다.
네 설치 파일을 고정한 candidate digest는
`305676a22b584ad883cae60d22f6291c00dc13f2ef7442f108a72f1de50eb0ee`다.
이 문서는 실패 위치와 소스 수정의 근거를 기록하며 전체 13개 통과를 증명하지 않는다.

## Debian ARM의 그래프 적용과 종료 대기

실제 설치본 스택의 Thread 10은 그래프 적용 중 유료 권한 조회에서 Secret Service의
`Prompt` 응답을 기다린다. 그래프 변경 잠금 뒤에 이 조회가 있어 종료의 작업 회수도 막혔다.
ELF의 `0x3d9fa4`는 credential `get_password` 호출의 반환 PC이고,
`0xebd68`은 프롬프트 대기 루프의 반환 PC다. 두 PC가 기록된 스택과 일치한다.
PIE load bias는 실제 maps 대신 ELF entry와 별도 `select` callsite를 대조해 계산했다.
Ghidra의 분석 주소는 ELF 좌표에 `0x100000`을 더한다.

주요 REA evidence는 `ev_8b64e1ac72cf34757d1f253f2d1b3cbbb04692620c4fcfdacae2dc1d956bf997`
(credential 조회), `ev_d6bf12888f6944a5e477822adc9a41eda31af71e408053d10353bfa1e4c8c3a6`
(프롬프트), `ev_3a98f18072621736282abb2c564683e234efad5a4f0a5f6402fc67d90eff47ea`
(권한 검사)다. Driver HTTP 대기나 reqwest 스택을 이 원인으로 분류하지 않았다.

Linux 읽기 어댑터는 `connect_with_max_prompt_timeout(Dh, 0)`을 사용한다.
`Unlock` 없이 잠긴 항목까지 포함해 유일성을 확인하고, 고유한 unlocked 항목만 `GetSecret`으로
읽는다. 첫 검색에 항목이 없을 때의 legacy default collection 조회도 보존한다.
연결과 개별 D-Bus 호출의 제한 시간은 별도이며, 0은 전체 작업의 0초 deadline을 뜻하지 않는다.
합성 시험은 잠김·중복·부재에서 secret 읽기를 시작하지 않는지와 기존 값·읽기 오류의 전달을 확인한다.
Linux 설치본에서의 수정 검증은 아직 남아 있다. macOS credential 읽기는 이 변경의 대상이 아니다.

## macOS PCM에서 확인한 범위

| 대상 | 신규 설치의 missing / corrupt / repeated 프레임 | 업그레이드 missing 프레임 |
| --- | --- | --- |
| macOS 14 ARM | 0 / 0 / 0 | 0 |
| macOS 15 ARM | 172 / 172 / 0 | 0 |
| macOS 26 ARM | 132 / 84 / 48 | 7152 |
| macOS 26 Intel | 3540 / 956 / 632 | 0 |
| macOS 27 ARM | 188 / 188 / 0 | 0 |

macOS 15 ARM의 원시 녹음에는 `[82672, 82764)`, `[132684, 132764)`의 0 구간이 있다.
대응하는 장치 sample 범위는 `[138128, 138220)`, `[188140, 188220)`이다.
프로브의 output 콜백 완료는 같은 sample 좌표의 input 콜백보다 약 21ms 앞선다.
이 기록만으로 최종 HAL WriteMix 내용과 게시 완료를 관찰했다고 볼 수는 없다.

`mr_hal_io_read_capture`가 반환한 공급 프레임 수를 상위 어댑터가 버리고 성공만 반환하고 있었다.
`IoOutcome`은 이 값을 보존하며 `IoTrace`가 측정 중 Float32 WriteMix·ReadInput의 실제 버퍼와
공급 결과를 기록한다. 게시된 0 PCM과 미공급 프레임을 실제 익명 매핑 경로에서 구별하는
회귀 시험을 둔다. 이 변경은 PCM 손상 원인을 확정하거나 손상을 복구한 수정이 아니다.

`pnpm probe:macos-roundtrip --hal-trace-file PATH`는 측정 전에 기록을 켜고, 정지 뒤 저장·해제한다.
JSON 행은 `[sequence, render, frame, frames, accepted, available, zeroFrames, hash]`이며
해시는 Float32 비트에 적용한 64비트 FNV 값의 16진수 문자열이다. 원본 PCM은 별도 capture 파일에
보존한다. 기록 활성화에 따른 추가 계산 비용과 고정 용량의 한계도 측정 해석에 포함해야 한다.

## Loopback 후속 대조

`ECIOCycleParam::init` (`0x151d0`)은 mixo에 output timestamp, read에 input timestamp를 고른다.
sample·host 유효성이 부족하면 clock으로 host를 복구한다.
Evidence는 `ev_461050d93d6b20dd46fea15537963edfcc7f44fcc3845dabee0e7bf4a6b01708`이다.

ARK의 stable tick은 128 경계 `T`에서 다음 예약을 먼저 설정한다. daemon은 output queue를
`T−192`에서 읽고 mixqueue의 `T+192`에 쓴 뒤, mixqueue `T−192`를 input queue `T+192`로 옮긴다.
SharedQueue의 erase watermark는 PCM mix보다 먼저 게시되며 범위 밖을 일부 0으로 채우는 읽기도
존재한다. 별도 완료 commit barrier가 부분 0을 막는다고 결론 내릴 근거는 없다.
이 차이를 이유로 MixRelay의 버퍼·오프셋·clock algorithm을 임의 변경하지 않았다.

## macOS 26 Intel 소스 검사의 C fixture

해당 실행의 소스 검사는 C fixture의 `transport_identity`에서 USB 판정 assertion으로 실패했다.
control 속성 조회는 20ms identity 예산을 소진하면 Virtual을 반환할 수 있다.
오디오 스레드 범위 진입·종료는 C bridge 호출을 직접 관찰하도록 하고,
단독 Rust FFI의 범위 복원은 TLS 상태로 확인한다. USB 정책은 기존 가짜 시계·신원 시험이 담당한다.
제품의 조회 제한 시간과 설치본 PCM 합격 기준은 바꾸지 않는다.

원시 기록과 REA bundle은 로컬 Diagnostics의 `installed-e2e-20261025/`에 보존한다.
이 분석에서 연 Ghidra 세션·JVM·demangler는 종료했다. 로컬 실제 오디오·서비스·TCC를 조작하지 않았다.
