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
편집기의 자동 저장과 제거 도구도 같은 네이티브 경계를 사용하며, 저장은 기존 unlocked 항목·collection에
한정한다. 잠긴 paid 항목은 덮어쓰거나 제거 성공으로 처리하지 않는다.
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

## 진단 속성의 전송 결함

`1c458123`의 [진단 실행 40214476447](https://github.com/mixrelay/mixrelay/actions/runs/40214476447)은
macOS 15 ARM에서 PCM 측정 전에 실패해 취소했다. 이 실행은 네 Mac만 선택한 진단이며
전체 설치본 검증 결과에 합산하지 않는다. 원시 증거 업로드도 GitHub ArtifactService의 요청 시간 초과로
실패했다. 저장 용량 초과를 나타내는 로그는 없었다.

사용자 정의 속성 `mrdi`·`mrdo`를 `None` 데이터 형식으로 선언한 것이 결함이다.
Apple SDK에서 이 형식은 속성·qualifier 데이터가 없다는 뜻이며 원시 바이트 전송 형식이 아니다.
두 속성은 `CFPropertyList`로 선언하고, 클라이언트와 서버가 포인터 크기의 `CFDataRef`를 교환한다.
HAL 조회가 만든 참조는 호출자가 해제하고, 설정에서 전달한 참조는 호출 동안 빌린다.

C 계약 시험은 실제 CoreFoundation binary property list 직렬화·역직렬화 뒤 SDK 어댑터로 설정·조회한다.
같은 경계를 통해 프로브의 전송·증거 파일 저장도 실행하며 실제 오디오 장치는 열지 않는다.
잘못된 flag·객체 형식·크기는 거부한다. 선언을 이전 `None` 값으로 되돌리면 이 시험이 실패한다.
프로브는 측정 중 실패해도 완성된 JSON에 단계·OSStatus·파일 오류를 남긴다.
이 수정의 설치본 검증과 기존 PCM 손상의 원인 확인은 아직 남아 있다.

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

## cc5e81b9 설치본에서 드러난 서로 다른 실패

전체 실행 [40220258479](https://github.com/mixrelay/mixrelay/actions/runs/40220258479)은 13개 중
7개 통과, 6개 실패다. macOS 14 ARM과 Ubuntu x64·Debian 양 아키텍처·Kali 양 아키텍처·Arch x64가
통과했다. macOS 15 양 아키텍처·26 양 아키텍처·27 ARM은 PCM에서, Ubuntu ARM은 업그레이드의
GRAPH-01에서 실패했다. 이 실행은 `502de680` 이후 소스의 설치본 검증이 아니다.

macOS 15 ARM의 업그레이드 원시 PCM에서 장치 `[167020, 167396)`의 376 프레임이 0이다.
마지막 WriteMix 행 2418은 `[166892, 167020)`, ReadInput 행 2420은 `[166884, 167396)`이고
공급은 136 프레임이다. 다음 WriteMix 행 2421은 167396에서 시작한다. 보존된 WriteMix 중
누락 범위를 게시한 행이 없으며, 프로브는 대응 출력을 input 콜백보다 약 21.46ms 먼저 생성했다.
같은 실행의 Apple 로그에는 작은 128프레임 클라이언트의 overload cycle 생략이 있다.
이 관찰은 최종 WriteMix에만 의존하면 이미 생성된 다른 클라이언트 출력을 놓칠 수 있음을 보여 준다.

SDK의 MixOutput 계약은 구현한 드라이버가 이후 출력 단계와 합산을 맡도록 한다.
수정한 HAL은 클라이언트의 Float32 출력을 이 단계에서 보관하고 입력에서 합산한다.
376프레임 누락 순서, 겹친 두 클라이언트, 한 클라이언트의 재시작·제거, 포맷·매핑 교체를
실제 PCM 큐로 검증한다. 콜백에서는 할당·해제·대기·재시도를 하지 않는다.
버퍼 크기·지연 오프셋·허용 오차는 바꾸지 않았다. 아직 모든 비영(非零) 손상의 원인이
확정된 것은 아니며 실제 설치본에서의 수정 검증이 필요하다.

진단 v2의 행은 `[sequence, render, frame, frames, accepted, available, zeroFrames, hash, client]`다.
고정 보관 용량은 4096행이다. 관측한 3886회 호출을 보존하기 위한 진단 용량이며 오디오 버퍼 크기와
무관하다. 오프라인 분석은 v1·v2를 읽고 다중 클라이언트의 개별 출력 해시를 합산 입력 해시와
직접 비교하지 않는다. `node scripts/e2e/analyse-hal-io.mjs EVIDENCE_DIRECTORY --output NEW_FILE`은
원시 PCM·콜백 시각·진단을 대조하며 합격 판정을 변경하지 않는다.

Ubuntu ARM은 부팅, 신규 설치의 모든 시나리오, 업그레이드의 오디오·녹음·제거를 통과했다.
GRAPH-01의 실패 화면에는 비활성 Recorder 버튼과 `node_not_found`, 자동 적용 중단을 나타내는
`control_unavailable`이 함께 있다. AT-SPI 시간 초과도 있으나 최종 실패를 그 지연만으로 분류할 수 없다.
해당 요청의 서버 거절 원인은 현재 보존 로그만으로 확정하지 못했다.

Loopback 후속 절차 분석에서도 모든 클라이언트의 완료를 기다리는 barrier는 찾지 못했다.
ARK의 출력 큐는 process 단위이고 client ID로 클럭을 고른다. daemon은 각 큐를 한 번 읽고 합산하며
늦은 PCM을 이전 입력에 복구하지 않는다. 큐 frontier는 PCM add보다 먼저 게시된다.
이 구조에서 임의의 대기·768프레임 지연을 MixRelay에 복사하지 않았다.
