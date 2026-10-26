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

`646d223` 설치본의 실행 [40229099074](https://github.com/mixrelay/mixrelay/actions/runs/40229099074)에서
macOS 14 ARM의 출력 431회·432회가 각각 모두 거절됐다. macOS 15 ARM에서는 신규 설치의 client
253 출력 1642회가 수락되고 261 출력 409회가 거절됐으며, 업그레이드도 client별 차이가 같다.
이 설치본은 PCM·녹음 검증에 실패했다. 추가한 합성 회귀 시험에서도 한 client가 장치를 시작한 뒤
새로 등록된 다른 client가 출력하면 `ClientMix`의 client별 active 조건에서 실패한다.

ARK의 StartIO·StopIO trampoline과 shim은 client 인자를 장치에 전달하지 않는다.
장치는 시작 참조 수를 관리하고, MixOutput은 AddDeviceClient로 등록된 ID의 context를 조회한다.
BeginIO의 thread hook은 비어 있고 EndIO도 active 상태를 변경하지 않는다. 출력이 끝난 큐는
게시 범위 이후 0을 공급하며, client별 필수 기여 집합은 없다. 이는 기존 ARK export의
0xe410·0xd0b8·0xc1dc·0x5118과 0xb750·0x1349c·0x151d0·0x1a260에서 확인했다.

수정본은 장치 실행 수명과 출력 등록을 분리한다. 실제 Rust 네이티브 런타임의 회귀 시험은
직접 StartIO를 받지 않은 등록 client의 출력, 다른 client가 실행 중일 때의 시작·정지,
제거·재등록, 장치 전체 재시작 후 이전 PCM 폐기를 검증한다. C SDK 시험도 가짜 PCM 콜백을
거치지 않고 실제 Rust 큐에 연결해 같은 순서를 확인한다. 이 시험은 익명 공유 메모리만 쓰며
시스템 장치 등록·실제 오디오 스트림을 사용하지 않는다.

## 등록 수명 수정 이후의 설치 증거

`d65ba391`의 전체 실행 [40232516962](https://github.com/mixrelay/mixrelay/actions/runs/40232516962)에서
macOS 14 ARM·15 ARM은 신규 설치와 업그레이드 PCM을 모두 정확히 반환했다. macOS 15 Intel의
신규 설치는 누락 1,312프레임으로 실패했고 업그레이드는 통과했다. 등록된 출력의 거절은 0회다.
원시 출력 타임스탬프가 건너뛴 656프레임은 HAL에도 게시되지 않았고, 입력 타임스탬프도 한 번
불연속이었다. 같은 시각 Apple 로그의 `HAL_client_IO_duration`은 7,483,127ns이지만 프로브
콜백 본문은 680,833ns였다. 이 차이를 MixRelay 처리 시간으로 단정할 수는 없다.

macOS 26 ARM은 신규 설치를 통과하고 업그레이드 PCM에서 52프레임의 무음으로 실패했다.
그 초기 구간은 v2 진단의 보존 범위 밖이다. v3는 진단 행을 8,192개 보관하고 HAL 처리의
host 시작·종료 counter를 기록한다. 이 counter는 PCM 처리만 둘러싸며 진단 해시 계산은 제외한다.
오디오 큐 용량·quantum·지연 오프셋·합격 허용 오차는 바꾸지 않는다.

macOS 26 Intel의 업그레이드 진단은 헤더가 issued 4,021을 선언했지만 복사 도중 게시된
4,022·4,023번 행도 포함했다. 분석기는 이를 거부했다. 생산 측 snapshot은 선언한 issued 이후의
행을 제외하며, dropped를 먼저 읽어 해당 예약 수가 issued에 포함되도록 한다. 고정된 snapshot
경계 뒤에 콜백을 게시하는 회귀 시험으로 이 조건을 확인한다.

macOS 26 Intel 신규 설치의 첫 불일치 구간 `[216088,216200)`은 프로브 입력에는 있지만
보존된 HAL ReadInput 요청에는 없다. 직전 요청은 216,088에서 끝나고 다음 요청은 216,200에서
시작한다. 출력 좌표·크기가 프로브와 일치하는 유일한 client의 보존된 출력 해시는 기준 신호와
모두 같다. client의 PID 신원은 이 진단에 없으므로 좌표에 따른 추론으로 구분한다.

ARK의 `ECSharedQueue::read` 0x14694와 daemon의 0x100025810은 frontier를 호출당 한 번
읽고 ring wrap 구간마다 복사·합산한다. 읽기·누락 통계의 atomic RMW는 없다. MixRelay는
다중 client 합산에서 프레임마다 공통 read를 호출해 source당 512프레임에 통계 RMW 1,024회를
요청했다. 수정본은 coverage를 함께 읽는 구간 API를 사용한다. frame stamp·epoch 검사와
잘못 읽은 프레임의 무음 처리는 유지한다. 이 비용 차이가 설치 실패의 원인인지는 미확인이다.

## Ubuntu ARM의 제어 관찰

`646d223` 실행의 Ubuntu ARM은 부팅·오디오·녹음·제거를 통과하고 신규 설치 GRAPH-01에서
실패했다. 업그레이드 GRAPH-01은 통과했다. 실패 화면에는 오디오 세션 실행 중과 자동 적용 중단이
함께 표시됐고, 서버 거절 로그에는 `node_not_found`가 반복됐다. 다른 제어 전송 오류는 기록되지 않았다.

`waitForPushedRevision`은 한 번 받은 상태 스트림이 끊기면 즉시 `control_unavailable`로 종료했다.
합성 회귀 시험은 같은 엔진의 새 상태가 원래 제한 시간 안에 도착해도 이 경로가 실패함을 확인한다.
수정본은 스트림을 다시 연결한 뒤 같은 엔진의 새 상태만 받아들인다. 엔진 교체·revision 추월·
원래 제한 시간 만료는 실패이며 변경 명령을 재전송하지 않는다. 기존 제한 시간을 늘리지 않았다.
이 결함과 설치 실패의 최종 인과 확인에는 수정본 설치 검증이 필요하다.

## Debian VM의 이전 버전 credential 대기

`e5a53f98` 실행 [40238734115](https://github.com/mixrelay/mixrelay/actions/runs/40238734115)의
Debian x64는 신규 설치의 모든 시나리오를 통과했지만 업그레이드 준비 중 이전 버전
`f7575aa`의 그래프 조회가 멈췄다. 후보 설치 전 단계다. 해시를 대조한 이전 버전 ELF와 스택에서
runtime 7621의 main thread가 `NativeCredentials::read`의 저장소 잠금을 보유한 채
`get_password`의 Secret Service Prompt 응답을 기다림을 확인했다. editor의 공통 D-Bus
`select` 프레임만으로 같은 credential 대기라고 판단하지 않는다.

VM journal은 22:15:16의 GNOME 키링 client/session assertion, 기존 데몬 종료와 재활성화,
22:15:26의 password prompt 시작을 기록했다. VM 준비의 `printf '\n'`은 빈 암호가 아니다.
[GNOME 입력 처리](https://github.com/GNOME/gnome-keyring/blob/main/daemon/gkd-main.c#L410)는
개행을 암호에 포함하며, 입력 없는 EOF는 암호를 공급하지 않은 것으로 처리한다.
재시작 뒤 LF 암호의 키링 잠금이 이전 버전의 Prompt를 유발했다는 것은 이 절차와 로그에 따른
추론이다. 당시 collection의 실제 잠금 속성은 수집하지 않았다.

VM 전용 `guest-keyring.mjs`는 NUL을 전달해 빈 C 문자열 암호를 공급한다. 기본 collection의
존재·unlocked 상태와 빈 암호의 저장 형식을 확인하고 준비 오류는 OS·경계·단계로 보고한다.
[GNOME unlock 절차](https://github.com/GNOME/gnome-keyring/blob/main/daemon/dbus/gkd-secret-unlock.c#L519)는
빈 암호를 먼저 시도하므로 이 키링은 데몬 재시작 뒤에도 password prompt가 필요 없다.
개인 환경·다른 세션·기존 키링은 변경 전에 거부하며 앱의 credential을 만들지 않는다.
키링 준비 책임은 세션의 디스플레이·오디오 서비스 조립과 분리했다. 오디오 콜백과 설치 합격 조건은
바꾸지 않았고 고정 대기·재시도를 추가하지 않았다. LF를 전달하는 대조 구현은 회귀 시험에 실패하며
수정 구현은 통과한다. 로컬 시험은 주입한 프로세스 경계를 사용하고 실제 credential bus에 접근하지 않는다.

## macOS 26 ARM에서 관찰한 순환 좌표 반복

`e5a53f98`의 같은 실행에서 macOS 26 ARM 신규 설치는 PCM 196프레임 누락·196프레임 반복으로
실패했다. 무음·잘못된 샘플·프로브 타임스탬프 불연속은 0이다. 업그레이드 PCM과 양쪽 제거는
통과했다. v3 진단은 issued 4,203행을 모두 보존했고 dropped는 0이다.

| 프로브의 불일치 입력 좌표 | 실제 반복한 이전 좌표 | 프레임 수 |
| --- | --- | ---: |
| `[84952,85012)` | `[68568,68628)` | 60 |
| `[127892,127932)` | `[111508,111548)` | 40 |
| `[177084,177180)` | `[160700,160796)` | 96 |

세 구간 모두 16,384프레임 이전의 두 채널 원시 PCM과 bit 단위로 같다. 이 간격은 현재
ZTS period와 공유 ring 용량 모두에 해당하므로 숫자만으로 계층을 특정할 수 없다.
세 구간에는 HAL ReadInput 요청이 없으며, 신호 창에 완전히 포함된 capture 1,298행의 해시는
요청 좌표의 기준 신호와 모두 같다. 출력 좌표·크기로 추론한 client 263의 280행도 모두 같고,
다른 client의 신호 창 내 출력 1,117행은 전부 무음이다. client의 PID와 epoch는 기록하지 않았다.

ARK의 기존 Ghidra export에서 `clok`는 0x14f8 → 0x796c → 0x15a04로 설정한 상수를
0x17420 → 0x15bf8에서 반환한다. payload 0x146608은 `raww`다. ZTS는 0x5580 →
0x14f64/0x14fcc에서 현재 host 시각으로 nominal 좌표를 계산한다. MixRelay도 같은 종류의
시각 모델을 쓰지만 `clok`를 노출하지 않아 SDK 기본 IIR가 적용됐다. 기본 IIR는 허용된 동작이며
속성 부재 자체가 SDK 위반은 아니다. 이 차이와 host의 입력 좌표 재정렬은 조사할 가설이며,
IIR가 위 반복의 원인인지는 아직 확인하지 못했다.

nominal 시각을 별도 필터 없이 전달하도록 `raw` 정책을 명시한다. `Clock`이 period·알고리즘·
안정성의 정의를 소유하고, property 어댑터는 제어 snapshot 없이 고정 슬롯으로 조회한다.
제어 잠금을 보유한 상태에서 콜백이 metadata를 읽는 회귀 시험은 이전 구현에서 실패하고 수정 후
통과하며 할당은 0회다. 실제 C SDK 진입점도 값·크기·읽기 전용·stream 객체 거부를 검증한다.
프로브 JSON에는 host가 제공하는 알고리즘·period·안정성을 추가하며 제공하지 않는 값은 null이다.
ZTS 계산·period·PCM 큐 용량·지연·합격 기준은 유지한다. 설치본에서의 수정 확인은 남아 있다.

## raw clock 설치본과 권한 관찰의 측정 경계

`7092df1f` 설치본의 실행 [40244643942](https://github.com/mixrelay/mixrelay/actions/runs/40244643942)에서
macOS 26 양 아키텍처의 신규 설치·업그레이드는 각각 144,000프레임을 정확히 반환했다.
실제 조회한 `clok`는 `raww`, period는 16,384이며 입력·출력 timestamp 불연속은 0회다.
이 결과만으로 이전 실패와 clock 정책의 인과관계를 확정하지 않는다.

macOS 15 Intel 신규 설치는 통과했지만 업그레이드는 누락 6,724프레임으로 실패했다.
프로브의 네 timestamp 점프는 848·1,120·1,172·600프레임으로 합계 3,740이다.
나머지 2,984프레임은 실제 0으로 수신됐다. 반복·잘못된 샘플은 0이며,
issued 4,279개의 HAL 진단 행을 모두 보존했고 출력 거절도 0회다.
이 Intel 기록의 timebase는 1:1이다. HAL 읽기 본문 최대는 167,636ns, 쓰기 본문은 46,480ns였고
프로브 콜백 본문 최대는 6,728,271ns, 콜백 진입 간격 최대는 32,802,207ns다.
같은 구간의 CoreAudio 로그는 프로브 scheduling latency 17,269,679ns와 cycle 생략을 기록한다.
콜백 본문의 wall time에는 선점도 포함되므로 이를 계산 비용이나 외부 호스트 결함으로 단정하지 않는다.

소스에서는 이미 권한이 승인된 업그레이드 측정 중에도 XCTest가 매초 권한 창의 AX 트리를
탐색하고 있었다. 프로브의 권한 처리와 장치 측정, XCTest의 그래프 조작과 권한 관찰 책임을 분리한다.
같은 서명 번들의 권한 전용 명령은 장치를 조회하거나 열지 않는다. 승인 뒤 관찰자가 AX 탐색 사이에서
중단 요청을 확인하고 응답해야 원래 3초 PCM을 한 번 실행한다. 성공·실패 뒤에는 관찰을 재개해
전체 시스템 캡처 권한과 Quit·제거를 처리한다. 토큰·단계·응답 시각을 증거에 남기며,
이전 응답·응답 부재·관찰자 종료는 측정을 시작시키지 않는다.

`MRProbeAuthorization`은 권한 절차, `PermissionObserver`는 실제 창 처리,
`PermissionObservationControl`과 `permission-observation.mjs`는 측정 전후의 확인 절차를 맡는다.
`macos-native-signal.mjs`는 CoreAudio 프로브 실행·판정을, `macos-scheduler-trace.mjs`는 선택적
추적을, `signal-process.mjs`는 자식 회수·완료 관찰을 맡는다. 공통 신호 모듈은 이 경계를 조립한다.
측정 중 AX 탐색 비용을 제거하고 오디오 콜백은 변경하지 않았다. 이 비용 제거가 위 간헐 실패를
해결하는지는 수정한 절차의 실제 설치본 검증이 필요하다.
