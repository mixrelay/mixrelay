# 설치본의 네이티브 경계 실패 분석

## 2026-10-27 Intel 콜백 대기와 System Trace

제품 `559721e`·측정 `f6d1f67`의 전체 실행 `40438316558`은 11개 통과,
macOS 15 Intel·26 Intel 실패다. Ubuntu ARM 부팅·설치본은 통과했다.
15 Intel 새 설치의 source output과 source HAL 쓰기에는 같은 612프레임 구멍이 있다.
공급한 전체 쓰기의 해시는 279행 모두 기준 신호와 일치한다. 업그레이드의 기본 왕복은 통과했다.
무음→유음의 소스 출력과 종료 녹음 누락은 새 설치 996·업그레이드 876프레임으로 각각 일치한다.
26 Intel의 기본 왕복·녹음·시스템 캡처·정리는 양 모드 모두 통과했지만, 추가 비교의 무음
콜백 좌표가 새 설치 유음→무음에서 724·업그레이드 무음→유음에서 664프레임 빠졌다.
관찰한 무음 payload는 모두 0이다. 콜백 coverage 실패는 그대로 유지한다.

동일 네 설치 파일과 측정 소스 `74ece43`의 집중 진단 `40443993285`에서 15 Intel의
새 설치 `AUDIO-01`은 missing 1,264·corrupt 632·repeated 0프레임으로 실패했다.
System Trace의 시작 알림은 `notify_register_dispatch`로 등록한 전용 Darwin 알림으로
확인한다. `xctrace`의 표준 출력 문구만으로 준비를 승인하지 않는다.
TOC는 실제 측정 중 5.336276초를 기록했으며 CPU 상태 330,294행과 user PC 샘플이 저장됐다.
스레드 상태 export는 60초 제한으로 끊겼다. 아래 대기 구간은 완성된 행에 있지만 전체 XML은
불완전하므로 export 성공으로 기록하지 않는다.

`MRRoundTripProbe` PID 10845의 IO thread `0x8140`과 원본 timing 431개의 진입·종료를
대조했다. 콜백의 두 명령이 해당 스레드의 CPU 실행 구간 안에 있어야 한다는 조건으로 구한
두 시계의 상수 차이는 `[731062505998, 731062512504]`ns다. 6,506ns 폭의 추론 범위이며
직접 기록한 wall-clock anchor는 아니다. 해당 범위 전체에서 다음 순서는 유지된다.

1. trace 1.947574419초부터 1.969690059초까지 소스 IO thread가 22.115640ms 대기한다.
   CoreAudio IO thread `0x8142`가 깨운 뒤 968ns 후 CPU를 받는다. 이 구간은 실행 가능한
   소스 thread가 낮은 우선순위 때문에 계속 밀린 상태와 구별된다.
2. 뒤따른 출력 콜백 좌표는 `73892`에서 `74524`로 건너뛴다. 같은 소스의 HAL 쓰기도
   정확히 그 632프레임을 게시하지 않았다. 입력 콜백의 별도 구멍은 `[72868, 73500)`이다.
   모든 콜백 본문의 최대 실행 시간은 490.963µs다.
3. HAL 읽기는 `[72868, 74012)`의 1,144프레임 중 1,024프레임을 공급하고 120프레임을
   0으로 채운다. 다음 `[74012, 74524)`는 512프레임 전부 미공급이다. 게시되지 않은 구간과
   실제 0 PCM이 일치하며 이전 PCM을 재생한 증거는 없다. source 쓰기 279행의 해시는 모두 맞다.

CoreAudio가 소스를 깨우기 전 구간에는 인터럽트와 Apple framework 실행이 함께 있다.
같은 PID의 binary images와 trace의 symbol archive를 UUID로 대조했다. CoreAudio의
`HALS_IOContext_Legacy_Impl::IsAnyEngineMixing_Helper`·`PerformIO`·`HALB_Guard::WaitFor`와
`HALS_IOEngine2::AudioIssueDetectorAnalyzeBuffer` → `rtaid::DetectorNode::AnalyzeABL` →
libvDSP 경로가 샘플에 있다. CoreAudio UUID는 `98907A8F-43FC-3D69-AB7E-BE41950154D3`,
검출기 UUID는 `31F6A9E2-A1BE-3E15-8867-2A32043F6E10`이다. archive의 image 상대 주소를
Apple `symbols`로 조회한 결과이며 해당 OS framework를 디컴파일한 것은 아니다.
이것만으로 내부 procedure의 비용, 인터럽트 원인, VM CPU 회수, MixRelay의 간접 부하 중
하나를 원인으로 확정하지 않는다. System Trace 자체도 부하를 더한다.
이 기록은 소스 호출 지연의 위치를 좁히며 추적하지 않은 전체 실행의 원인을 증명하지 않는다.
버퍼에는 생성된 PCM만 보존할 수 있으므로 이 구멍을 저장량 증가로 복원할 수는 없다.

집중 진단의 업그레이드 기본 왕복과 새 설치·업그레이드의 무음·유음 양 순서는 통과했다.
정리도 통과했지만 진단 실행은 실패 상태이며 전체 13개 수락에 합산하지 않는다.
원시 ZIP SHA-256은 `75cb0afc45f8def78a62ad001df043989a0429a9283f858c8468750949e5ee9b`로
확인했고 원본 PCM·XML·로그를 목적이 명시된 Diagnostics에 보존했다. ZIP 사본은 남기지 않는다.

## 2026-10-27 출력 콜백과 Driver 조회 수명

`abf09f6`의 전체 실행 `40375187739`에서 실패한 원본 PCM과 callback timing,
native trace를 같은 장치 sample 좌표로 대조했다. 아래는 새 설치의 유음 구간이다.

| 대상 | 출력 콜백 좌표의 누락 | 같은 source의 HAL 쓰기 누락 | 공급한 HAL 쓰기의 해시 불일치 |
| --- | ---: | ---: | ---: |
| macOS 14 ARM | 1,412프레임 | 1,412프레임 | 0행 |
| macOS 15 Intel | 3,596프레임 | 3,596프레임 | 0행 |
| macOS 26 Intel | 7,760프레임 | 7,760프레임 | 0행 |

`MRRoundTripProbe::mr_play`는 `sampleTime - origin`에서 출력 신호를 생성한다.
콜백 좌표가 건너뛰면 그 구간의 신호를 만들지 않는다. HAL `ReadInput`은 실제 요청받은
범위를 읽고 게시하지 않은 구간을 0으로 채운다. 14 ARM의 최대 native 호출 간격은 약
39.7ms였으며 뒤따른 프로브 콜백 본문은 8.1µs였다. CoreAudio overload도 기록됐다.
읽을 수 없는 source 구간을 가짜 PCM으로 채우거나 기존 PCM으로 되돌려서는 안 된다.
Loopback ARK의 `ECSharedQueue::readSampleTime` `0x14694`도 보존 범위 밖을 0으로 채우고,
`_eraseSampleTime` `0x148d0`은 저장량 안에서만 전진·초기화한다. 이는 보관된 Ghidra
procedure의 정적 관찰이며 동일 호스트에서 Loopback의 성공을 측정한 결과는 아니다.

실패 직후 3초 engine sample에는 짧게 생성된 `reqwest-internal-sync-runtime` thread가
14 ARM 16개·15 Intel 15개·26 Intel 17개 관찰됐다. macOS 26 Intel 실행 파일 UUID는
로컬 최종 Universal 실행 파일의 x64 UUID `95C33BCB-46F0-3330-BD6E-ECD791626032`와 일치한다.
`MacVirtualEndpointBackend::snapshot`은 client를 매번 만들고, `DriverClient::snapshot_blocking`은
매번 blocking client를 만든다. reqwest 0.13.4의 `ClientHandle::new`는 내부 runtime thread를
생성하며 마지막 handle의 `Drop`은 thread를 종료·join한다.

후속 소스는 endpoint 소유자와 format monitor가 같은 client를 사용하고 동기 runtime·연결을
첫 동기 호출부터 공유한다. 실제 Unix 연결 회귀는 같은 11요청에 수정 전 11연결, 수정 후
1연결을 확인했다. 권한·응답·revision·실패 조건은 유지한다. 이 반복 초기화 부하와 PCM
실패의 인과관계는 아직 설치본에서 확정하지 않았으며, 원래 PCM 실패 판정도 유지한다.

후속 진단 실행 `40435326879`에서는 MixRelay 제거 후 같은 호스트의 BlackHole v0.7.1도
2,076프레임 누락·512프레임 반복·512프레임 무음 손상으로 실패했다. 독립 드라이버에서도
불연속이 관찰됐지만 MixRelay의 수락을 대신하지 않는다. 새 설치의 System Trace 준비는
시간 내 완료되지 않아 PCM을 측정하지 못했다. 남은 trace는 0.055334초뿐이므로 원래 PCM
불연속의 scheduler 원인을 그 trace로 확정하지 않는다.

같은 진단의 무음→유음 도구는 두 유효 구간의 payload·callback·native coverage가 모두
통과했지만 요청한 364,800프레임 뒤에도 기록을 계속해 372,736프레임을 저장하고 overflow
2회를 냈다. source 비교 callback도 유한 horizon에 도달하면 저장을 닫도록 수정했다.
마지막 callback 전체는 보존하며 요청 구간 안의 버퍼 부족은 계속 실패한다. 합성 native
callback 시험은 수정 전 종료 지연 사례에서 실패하고 수정 후 통과했다. 이는 설치 파일을
바꾸지 않는 진단 도구 수정이며 `AUDIO-01`의 PCM 손실·반복·손상 기준은 유지한다.

## 2026-10-27 동시 clock journal 시험의 완료 조건

소스 실행 `40438301652`의 macOS 26 Intel은 동시 clock journal 시험에서 전체 경과 시간
10초 assertion으로 실패했다. payload 불일치가 보고된 실패는 아니다. 이 제한은
`381c7715`(2026-10-07)부터 있었으며 당시 스레드가 왜 늦었는지는 로그만으로 확정하지 않는다.

시험은 10,000회 게시와 유한한 동시 읽기를 실행하고 생산자를 join한 뒤 남은 최대 64개
기록을 읽는다. 각 payload의 일관성·누락 합계·최종 cursor·완료 후 빈 기록을 확인한다.
완료 여부를 호스트의 실행 속도로 판정하지 않으며 제품 journal 코드는 바꾸지 않았다.
로컬 ARM release 시험은 239개가 통과했다. 별도 실행하는 성능 측정 1개는 기존 ignore다.
Intel 대상은 컴파일됐지만 이 ARM 호스트에서 실행되지 않았으므로 실제 Intel 확인은 남아 있다.

## 2026-10-27 누적 credit과 보존 용량

`a88eada` 집중 실행 `40329569555`의 macOS 26 Intel 새 설치는 뒤 순서의 소스 output에
1,108프레임 구멍이 있으나, 종료 녹음은 유음 144,000프레임 중 896프레임만 식별된다.
이 take는 `stopped`, `error: null`이며 packet queue overrun으로 실패한 이전 `f27da5c`와
구별한다. 앞 순서와 업그레이드 양 순서의 유음은 완전했다. 15 Intel은 새 설치 통과,
업그레이드에서 source output과 종료 녹음의 누락이 각각 4,032·1,600프레임으로 일치했다.
15 ARM은 새 설치·업그레이드 양 순서와 모든 필수 시나리오가 통과했다.

지연된 그래프에 native credit 4,096프레임을 한 번에 주는 합성 회귀가 유음 32,768프레임 중
1,280프레임만 전달하며 실패했다. 실제 공유 VM·ClientPcmMixer·Converter·SourceRegistry·
CaptureAlignment를 통과하는 경로다. 생산자가 남긴 PCM은 보존돼 있는데 획득 어댑터가
이번 128프레임만 반영한 tail 부근에서 읽기 시작하고 누적 credit 전체를 진행하고 있었다.

남은 전체 credit을 전달한 뒤에도 producer reserve를 그대로 더하면 용량 끝에서 실패했다.
16,384프레임 burst의 유음은 수정 전 14,336프레임, 수정 후 32,768프레임이다.
`source_time_for_credit`은 실제 읽기 quantum과 남은 credit을 분리하고 보존 용량을 넘지 않는다.
실제 ApplicationPacer·MacTimeline·BlockProcessor·MacEngineExternalIo를 연결한 별도 회귀도
전달 경계를 검증한다. pending extent 전달을 제거하면 20,480프레임 중 15,616프레임만 남고,
현재 소스는 전부 보존한다. 이 조작은 원래 설치본 손실 수치의 재현과 구분한다.

대기 절차는 다음 native 절대 경계를 계산한다. 이전 250µs 대기는 2,048프레임 동안 171회
깨어났으며 현재 회귀는 native 128프레임의 16주기와 늦은 wake의 위상 보존을 확인한다.
보존된 PCM은 순서대로 따라잡고 놓친 시간만큼 가짜 credit을 만들지 않는다.

입력 링은 최대 지원 192kHz의 4초를 power-of-two로 올린 1,048,576프레임,
녹음 큐는 768,000프레임으로 준비한다. 용량 초과의 기존 실패·불연속 계수와 수락 기준은 유지한다.
4초와 링 끝까지 지연된 공유 입력, 각 지원 rate의 4초 녹음 전달에 대해 유음·무음 순서와
실시간 할당 0을 확인했다. 이는 아직 새 설치본의 성공을 뜻하지 않는다.

## 최초 분석 대상

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

`61d525f2`의 전체 설치 실행
[40251947994](https://github.com/mixrelay/mixrelay/actions/runs/40251947994)에서는
macOS 26 ARM과 27 ARM의 신규 설치가 각각 48·84프레임 누락·반복으로 실패했다.
두 환경의 업그레이드와 녹음은 통과했다. 권한 사전 승인은 실제로 완료됐고,
측정 중 관찰자 정지 응답과 `raww` 메타데이터도 확인했다.
따라서 권한 관찰 분리가 이 반복 결함을 해결했다고 판단할 수 없다.
최종 결과는 13개 대상 중 11개 통과·2개 실패다. Ubuntu ARM도 통과했고,
필수 시나리오 skip은 0개이며 제거는 13개 모두 통과했다.

원본 PCM의 장치 구간은 각각 `[203808,203856)`, `[247892,247976)`이다.
모든 채널의 비트가 정확히 16,384프레임 전 기준 신호와 같다.
각 환경에서 신호 전체에 들어가는 프로브 출력 280행과 HAL 입력 1,396·1,383행은
모두 기준 해시와 일치했다. 다른 출력 클라이언트의 신호 구간 기여는 모두 디지털 무음이다.
두 손상 구간에 겹치는 보존된 HAL 읽기 행은 없다. 진단은 Registry 진입 검사 뒤에서 시작하므로,
잘못된 stream·장치 freeze·비활성 분기에서 먼저 반환한 호출까지 없었다고 증명하지는 않는다.

26 ARM에서 직전 보존 읽기의 끝은 203,808이고 다음 읽기는 203,856부터 256프레임이다.
같은 구간의 엔진 clock-break는 예상 sample 203,808에서 실제 203,984로 176프레임 이동을 기록한다.
로컬 CoreAudio의 확인된 catch-up 식에 요청 끝 204,112와 요청 크기 128을 넣으면
시작점은 `204112−2×128=203856`이며 48프레임의 틈과 일치한다.
27 ARM도 직전 frontier 247,892와 다음 요청 끝 248,232에서 같은 식이 84프레임의 틈을 만든다.
27 ARM VM과 조사한 로컬 시스템은 모두 macOS 27.0.1(26A434)이다.
보존한 27 ARM의 `live-engine.sample.txt`와 `live-coreaudio-server-43609.sample.txt`에서
CoreAudio UUID `3AB8F076-7437-38AA-9F5B-B083CF020ADA`가 로컬 조사 이미지와 같음을 확인했다.
해당 실패 순간의 내부 호출 스택은 없으므로 정적 절차와 일치하는 이 설명을
확정된 실행 인과로 취급하지 않는다.

Loopback의 가상 endpoint는 daemon과 HAL의 공유 큐로 PCM을 교환하고 자체 timer로 처리한다.
물리 장치의 CoreAudio IOProc 경로와 별개이며, daemon의 128프레임 tick은 자기 가상 endpoint의
128프레임 입력 IOProc가 아니다. HAL ZTS는 nominal host 시각을 8,192 경계로 내리는 방식이고
`raww`·stable·기본 safety 0을 사용한다. 이 비교는 MixRelay의 내부 전송 책임을 분리할 근거이며,
버퍼·오프셋을 복사하거나 외부 앱 사이의 캐시 반복까지 해결됐다고 주장할 근거는 아니다.
원시 절차와 해시는 Diagnostics의 `loopback-virtual-endpoint-transport-20261026/`에 보존한다.

오프라인 분석은 신호 생성, 클라이언트별 기준 해시 대조, 원본 입력 PCM 대조를 각각
`probe-signal.mjs`, `probe-native-reference.mjs`, `probe-pcm-reference.mjs`가 맡는다.
무음인 다른 출력이 확인된 경우에도 기준 대조를 수행하며, 알 수 없는 합산에는 적용하지 않는다.
예상 해시는 같은 좌표·길이에서 재사용한다. 프로브 실행과 판정도 분리해,
`macos-probe-verdict.mjs`가 OS 왕복 경계와 네 PCM 실패 수를 전달한다.
이 변경은 진단과 코드 책임의 수정이며 제품 오디오 경로의 수정이 아니다.

## 입력 소스의 무음·유음 대조

위 실행에서 확인한 디지털 무음은 다른 출력 클라이언트의 PCM이다. `from_os` 입력에
보내는 소스 자체를 무음·유음으로 바꾼 대조 시험은 수행하지 않았다. 두 조건을 혼동해
입력 소스 대조까지 끝난 것으로 설명할 수 없다.

`candidate-e2e.yml`의 `macos_source_comparison=true`는 선택한 macOS 대상에 별도 대조를 추가한다.
대상을 생략한 전체 13개 실행에도 이 대조를 포함할 수 있다.
설치본·입력 UID·그래프·클라이언트·버퍼를 유지하고, 한 IOProc 안에서 무음→유음 또는
유음→무음으로 각각 3초씩 재생한다. 두 순서는 실행 전에 고정하며 실패한 측정을 반복하지 않는다.
0.1초 lead와 1.5초 tail은 기존 프로브와 같다. 기존 AUDIO-01·REC-02를 먼저 완료하고,
동일한 `qa-source/from_os → qa-recorder/audio` 그래프에서 별도 녹음을 시작한다.

네이티브 도구는 장치 설정을 조회할 뿐 변경하지 않는다. 파일 PCM과 캡처 저장 공간을 미리
할당하고 콜백에서는 복사와 고정 용량 시각 기록만 한다. 장치 조회, 실시간 전송, 실행 수명,
소스 계획, 오프라인 판정, 녹음 수명을 각각 별도 모듈로 둔다. 새 도구의 권한을 먼저 승인하고
측정마다 AX 관찰 정지·재개 영수증을 따로 보존해 기존 PCM 측정의 증거를 덮어쓰지 않는다.

각 입력 상태에서 출력·입력 timestamp의 전체 구간 coverage, 중복·단절, host 시각 진행과
원본 PCM을 확인한다. 무음의 0 샘플만으로 누락·반복이 없었다고 판단하지 않는다.
무음은 샘플 자체의 식별이 불가능하므로 이 한계를 결과에 명시한다. 유음은 기존 정수 코드와
비트 단위로 대조한다. 원본 PCM·시각·HAL 행·소스 해시·실행 파일 해시를 보존하고,
이 대조의 성공으로 기존 필수 PCM 실패를 통과로 바꾸지 않는다.

로컬에서는 장치를 열지 않는 콜백 self-test와 합성 캡처로 판정기를 검사한다.
실제 두 입력 상태의 설치본 대조는 아래 실행에서 수행했다.

### 고정 설치본의 실제 대조 결과

[40261486297](https://github.com/mixrelay/mixrelay/actions/runs/40261486297)은 제품 `61d525f2`,
suite `d42cffa7`과 기존 PKG SHA-256
`193b94e267fd9008b9842be53f27d271fffae3c8cbf3654873f7d84cf1111b08`을 사용했다.
macOS 26·27 ARM에서 신규 설치·업그레이드마다 두 순서를 한 번씩, 총 8회 수행했다.
각 상태는 144,000프레임이다. 아래 수치는 원본 캡처에서 기대 소스와 다른 프레임 수이며
콜백 timestamp의 누락과 구별한다.

| 환경·설치 | 무음→유음: 무음 / 유음 | 유음→무음: 유음 / 무음 |
| --- | --- | --- |
| 26 ARM 신규 | 0 / 308 | 104 / 0 |
| 26 ARM 업그레이드 | 0 / 0 | 0 / 0 |
| 27 ARM 신규 | 0 / 968 | 236 / 56 |
| 27 ARM 업그레이드 | 0 / 0 | 0 / 0 |

26 ARM 신규 설치의 잘못된 유음 샘플 308·104프레임은 모두 정확히 16,384프레임 전
식별 신호와 일치한다. 소스의 게시 구간은 완전하고 보존된 HAL 읽기의 해시도 기준과 맞지만,
손상 구간과 같은 길이의 HAL 읽기 기록이 비어 있다.

27 ARM 신규 설치의 유음→무음에서는 무음 전환 4,400프레임 뒤에 이전 유음 56프레임이
나타났다. 모든 비트가 16,384프레임 전 유음과 일치한다. 같은 무음 구간의 HAL 읽기 기록에는
총 204프레임의 틈이 있다. 나머지 148프레임은 캡처가 0이어서 샘플만으로 반복을 식별할 수 없다.
소스 게시·시간 coverage·공급량까지 대조해야 하며, 0 PCM이라는 이유로 이 구간을 정상으로
확정하지 않는다. 이 기록의 부재도 Registry 이전 반환이나 host의 캐시 선택을 구분하지 못한다.

27 ARM 신규 설치의 무음→유음에는 별도로 입력·출력 콜백 timestamp가 각각 816프레임
점프했다. 소스가 게시되지 않은 출력 구간의 0 캡처를 HAL 합산 오류로 분류하지 않는다.
오프라인 분석은 소스의 게시 coverage와 네이티브 공급 coverage를 분리하고, 게시가 확인되지
않은 구간은 캡처 기준 해시 대조에서 제외하되 원래 누락은 실패로 보존한다.
두 OS의 업그레이드 대조는 입력·출력·소스 게시·HAL 공급 coverage가 모두 완전했다.

`source-comparison-native.mjs`는 네이티브 게시·공급 구간의 검증을 맡는다.
`analyse-source-comparison.mjs`는 보존한 원본의 해시를 기록하고 추가 판정을 별도 파일로 쓴다.
원본 판정은 덮어쓰지 않는다. 무음의 공급량 0, 소스 게시 누락, 다른 무음 클라이언트가 제공한
0 PCM, 관찰되지 않은 읽기 구간을 구별하는 회귀 시험을 포함한다.

기존 필수 AUDIO-01도 그대로 실행했다. 26 ARM은 신규 설치 112·업그레이드 100프레임,
27 ARM은 신규 설치 136프레임의 누락·반복으로 실패했고 27 ARM 업그레이드는 통과했다.
REC-02와 제거는 두 대상의 신규 설치·업그레이드 모두 통과했다.
이 실행은 두 대상의 진단이며 전체 13개 대상의 합격 실행이 아니다.

## SDK 요청과 Registry 입장 경계

Loopback ARK의 `WillDoIOOperation`(0xc390)은 read·mixo·thrd만 선언하며,
`DoIOOperation`(0xc494)은 read·mixo만 처리한다. 별도의 ProcessInput 훅이나
NonMixable Float32 형식으로 외부 입력 cache를 보완하는 절차는 확인되지 않았다.
두 드라이버의 canonical Float32는 interleaved lpcm/flags 9다.
ARK의 readFromStream(0x1a188)은 큐가 없거나 rate가 다르면 버퍼를 쓰지 않고 성공을 반환한다.
따라서 Loopback이 모든 실패에서 먼저 버퍼를 지운다는 가정은 성립하지 않는다.
원시 31개 절차와 소스 대조는 로컬 진단의
`loopback-hal-read-contract-20261026/findings.md`에 보존했다.

MixRelay의 기존 v3 PCM trace는 Registry 입장 뒤에 시작한다. SDK sample time 거절,
NULL·과대 버퍼, 잘못된 stream ID, 동결된 장치의 IO 거절은 행을 만들지 않는다.
읽기 행의 누락만으로 Apple이 DoIOOperation을 호출하지 않았다고 확정할 수 없다.
이 경계는 실패 설치본 61d525f2와 0ae7aa2 소스에서 같다.

v4 trace는 기존 PCM 행과 처리 host counter를 보존하고, 같은 측정 창의 읽기·쓰기 요청,
sample time·버퍼 범위·stream·freeze·native 거절과 inactive 반환을 별도로 집계한다.
SDK의 원래 Float64 sample time은 Rust 경계로 전달하며, 거절한 값의 비트도 보존한다.
입장 판단은 `io_dispatch`, 집계는 `io_admission`, PCM 행은 `io_trace`가 소유한다.
제어 조회의 snapshot은 여러 counter를 한 transaction으로 읽지 않으며,
거절 counter 자체는 실패 시각을 지정하지 않는다. 잘못된 driver 참조나 이미 없는 장치에는
측정 중인 장치의 counter를 귀속할 수 없다.

로컬 회귀는 fraction·NaN 좌표, NULL·과대 버퍼, stream 불일치와 freeze를 주입해
PCM 행이 없는 거절도 남는지 확인한다. Freeze 시험은 이전 PCM으로 채운 버퍼가
거절 시 그대로 남는 현재 동작까지 확인한다. 이 시험은 해당 거절이 설치본의
56프레임 재생 구간에서 발생했다는 증거가 아니며, PCM 수정 완료를 뜻하지 않는다.

### 입장 경계가 포함된 설치본의 결과

[40272813677](https://github.com/mixrelay/mixrelay/actions/runs/40272813677)은 제품·시험 소스
`7bfc05df`의 새 PKG 한 파일을 사용했다. PKG SHA-256은
`85c6d15c44bc6ed2fa79be1a151a2670a4ea857c5937c3ae38b218af0296a6b1`이다.
두 ARM 대상의 신규 설치·업그레이드에서 두 입력 순서를 각각 한 번 수행했다.
8회 모두 v4 trace를 보존했고 timestamp·extent·stream·frozen·inactive·native counter는 0이었다.
이 결과는 측정된 장치의 IO 입장 거절을 배제한다. GetZeroTimeStamp의 반환과 장치 수명 전환은
이 counter의 관찰 범위가 아니므로 함께 배제했다고 설명하지 않는다.

26 ARM 신규 설치의 무음→유음은 유음 188프레임, 유음→무음은 유음 36프레임이 달랐다.
업그레이드의 유음→무음에서도 40프레임이 달랐다. 각 차이와 같은 구간에 HAL 읽기가 없었고,
신규 설치의 무음 구간에는 값이 0인 채로 읽기 기록이 없는 380프레임도 있었다.
소스 게시에는 틈이 없었다. 이 틈 직후의 새 읽기는 모두 256프레임이었다.
27 ARM의 두 설치 경로는 외부 대조를 통과했다. 해당 실행의 대상 결과는 26 실패·27 통과이며,
이 집중 실행은 전체 13개 대상의 수락 결과가 아니다.

### 엔진 녹음의 식별 신호 연속성

기존 REC-02의 RF64 형식·유한 샘플·음량 검사는 중간의 짧은 손실을 검출하지 못했다.
별도 녹음에도 두 입력 순서의 식별 신호가 들어 있으므로 원본 코드와 비트 단위로 대조한다.
`recording-format`은 finalized RF64를 해석하고, `recording-codewords`는 4프레임 코드로 위치를
찾은 뒤 양 채널 전체를 검증한다. 코드 중간에서 시작한 run의 앞쪽 최대 3프레임도 확인한다.
`source-recording-analysis`는 순서별 144,000 유음 프레임의 누락·중복·순서·삽입 무음을 판정한다.
식별되지 않은 비영 샘플과 nonfinite 샘플도 실패다. 무음 자체의 샘플 식별은 불가능하며,
별도 입력 callback·게시 coverage 검사를 유지한다.

7bfc05df 실행의 원본 녹음을 이 방법으로 다시 읽은 유음 누락 수는 다음과 같다.

| 환경·설치 | 무음→유음의 유음 누락 | 유음→무음의 유음 누락 |
| --- | --- | --- |
| 26 ARM 신규 | 1,104 | 292 |
| 26 ARM 업그레이드 | 0 | 296 |
| 27 ARM 신규 | 0 | 0 |
| 27 ARM 업그레이드 | 972 | 0 |

27 ARM 업그레이드의 외부 대조 통과는 엔진 녹음의 연속성을 보장하지 않았다.
이 분석은 원본 녹음·판정을 바꾸지 않는다. 새 설치본 검증은 stop 완료 후 녹음 SHA-256과
`recording-analysis.json`을 보존하고 연속성 실패를 독립적으로 집계한다.
외부 대조가 실패해도 녹음을 종료·검사하며 두 오류를 모두 남긴다.
회귀 시험은 이전 실행의 524프레임 손실·128프레임 삽입 무음과, 무음 중 56프레임 과거 유음
재생을 포함한다. 제품의 캐시 반복 원인 수정과 전체 13개 수락은 아직 완료하지 않았다.


## a47b164 공유 입력 설치본과 발행 경계의 후속 수정

제품·시험 소스 `a47b164a6a58f69abfc913954d0969f250fb2d58`의
[집중 실행 40292248302](https://github.com/mixrelay/mixrelay/actions/runs/40292248302)은
macOS 26 ARM·27 ARM 모두 실패했다. 전체 13개 수락 실행은 아니다.
PKG는 69,506,808바이트이며 SHA-256은
`21f07bb515c515086dac4e463b6da2a1f7808d2b2a861a02bd1d8c2e2dd5b7b4`다.
공증 `3d7a9a86-8ebf-4d17-b0e2-ab87c8bae5e7`은 Accepted·staple·validate를 통과했고,
같은 소스의 Linux 후보는 빌드 `40290474611`에서 성공했다.
[Native CI](https://github.com/mixrelay/mixrelay/actions/runs/40290468513)의 13개 대상,
VM 준비 7개와 UI 검사 성공은 이 설치본 실패를 대체하지 않는다.

새 설치·업그레이드 각각에서 같은 from_os → recorder 그래프와 두 입력 순서를 유지했다.
결과는 다음과 같다. 녹음 누락은 종료 RF64에서 식별 가능한 유음 codeword의 프레임 수다.

| 환경·설치 | 외부 무음→유음 / 유음→무음 | 녹음 유음 누락: 앞 순서 / 뒤 순서 |
| --- | --- | --- |
| 26 ARM 새 설치 | 통과 / 실패 | 21 / 1,759 |
| 26 ARM 업그레이드 | 통과 / 통과 | 115,517 / 144,000 |
| 27 ARM 새 설치 | 통과 / 통과 | 144,000 / 144,000 |
| 27 ARM 업그레이드 | 통과 / 통과 | 144,000 / 144,000 |

8개 대조의 무음 payload 오류는 0이었다. 26 ARM 새 설치의 뒤 순서에는 input·output callback
sample 좌표가 각각 1,256프레임 점프했고 소스 게시도 그 구간에서 빠졌다. 외부 유음 1,024프레임이
달랐으며, native 공급 기록의 빈 구간은 합계 2,280프레임이다. 게시되지 않은 소스와
엔진의 녹음 손실을 같은 결함으로 합치지 않는다. 이 실패 판정은 유지한다.

26 ARM 새 설치에서는 첫 순서의 외부 source 게시·읽기·payload가 모두 완전한데도 녹음에서
21프레임이 빠졌다. 업그레이드와 27 ARM의 외부 대조가 통과한 경우에도 녹음은 실패했다.
따라서 외부 HAL 대조 성공만으로 새 엔진 입력·녹음을 수락할 수 없다. 공유 source worker가
실제로 실행됐고 식별 가능한 입력 PCM도 녹음에 전달됐으므로 grant 연결 전체를 실패로
분류하지 않는다. 네 녹음의 원본·SHA와 engine 로그는 실행별 target artifact에 보존했다.

모든 설치 경로의 엔진 로그에 `hal_shared_pcm` / `source_pump`의 `Stale` 종료가 있다.
기존 코드의 두 경로가 같은 오류를 낼 수 있었다. `SourceTransport::clock()`은 소비자 sequence
갱신 중에도 `None`을 반환하는데 worker는 이를 만료로 처리했다. 또한 converter 생성은
생산자 reset을 완료한 뒤 소비자 clock을 다시 요구했다. 그 관찰이 보류되면 새 epoch와
이전 converter가 남을 수 있었다. 당시 로그만으로 둘 중 마지막 분기를 구별하지는 못한다.

현재 소스는 관찰 결과를 `Ready`·`Deferred`·`Revoked`로 나누고, reset이 확정한 epoch를
직접 반환한다. 실제 공유 consumer sequence를 갱신 중으로 고정한 회귀에서 이전의
post-reset 관찰 의존성을 재주입하면 `Err(Stale)`로 실패했다. 복원한 구현은 같은 조건에서
새 epoch와 PCM을 유지했다. 초기의 정상 snapshot, 보류, 실제 lease 교체도 구별한다.

추가로 worker는 변환 PCM을 flush하기 전에 native credit을 게시했다. 현재는 source 전송의
같은 producer claim에서 PCM을 먼저 확정하고 그 출력에 대응하는 credit만 게시한다.
21·5·23·79프레임 입력 조각에서 실제 공유 ring의 공급량과 credit을 대조하며, SRC에 남은
입력을 준비된 PCM으로 세지 않는다. 변환 출력의 배출과 보류 쓰기도 유한하게 유지한다.

새 epoch의 초기 native rate·credit 0은 ring epoch release보다 먼저 게시한다. ring 재무장 실패 시
기존 rate·position은 복원하고 running·승인은 해제한다. 기존 C ABI의 실패한 재무장 계약을 유지했다.
포맷 재승인 동안 master 신원도 유지하며, 끝난 worker는 다음 완전한 grant 목록에서 회수·교체한다.
clock 관찰·epoch 승인과 PCM 발행의 소유 구현을 나눴다.

후속 소스의 관련 Rust·네이티브 검사 966개와 스크립트 625개가 통과했다.
기존 benchmark·자식 fixture·설치 장치 soak의 기본 실행 제외는 유지했다.
설치본 필수 시나리오의 skip 0은 최종 13개 실행에서 판정한다.
관련 all-targets clippy·fmt·crate 계층 검사도 별도로 확인한다.
이 후속 코드의 설치본과 같은 네 파일을 사용하는 13개 전체 검증은 아직 남아 있다.

## `8d94344` 설치본과 부분 블록 정렬 회귀

[집중 실행 40307670264](https://github.com/mixrelay/mixrelay/actions/runs/40307670264)는
같은 PKG SHA-256 `e59daddc0e7c093a646758ff71ba81c0bfd0c498891492fe790735b91233f2ef`를
두 ARM 환경의 새 설치·업그레이드에 사용했다. 두 job 모두 실패했다. 13개 전체 실행은 아니다.

| 환경·설치 | 외부 무음→유음 / 유음→무음 | 녹음 유음 누락: 앞 순서 / 뒤 순서 |
| --- | --- | --- |
| 26 ARM 새 설치 | 통과 / 통과 | 0 / 26 |
| 26 ARM 업그레이드 | 통과 / 통과 | 0 / 0 |
| 27 ARM 새 설치 | 통과 / 통과 | 69 / 284 |
| 27 ARM 업그레이드 | 통과 / 통과 | 0 / 6 |

각 외부 대조는 input·output callback 좌표의 누락·중복, payload 오류와 native 오류가 0이었다.
네 경로의 cleanup은 통과했다. 보존된 engine 로그 13개에는 `hal_shared_pcm` / `source_pump`
종료가 없다. 같은 후보의 source CI 13개와 Linux 빌드 성공은 녹음 실패를 대체하지 않는다.

26 ARM 새 설치의 뒤 순서는 소스 codeword `[6494,6504)`, `[7625,7629)`, `[22822,22834)`가
빠졌고, 녹음 중간에 85·118·112·89프레임의 0이 삽입됐다. 27 ARM 새 설치의 뒤 순서는
284프레임이 빠졌으나 해당 경계에 0이 삽입되지는 않았다. 반복·역행·식별 불가 유음·nonfinite는
네 녹음 모두 0이었다. 원본과 분석은 실행의 target artifact 및 로컬 `40307670264`에 보존했다.

실제 공유 VM → mixer → converter → source registry → `CaptureAlignment` 회귀는 native
512프레임 게시, 21·5·23·79프레임 등을 포함한 playhead 관찰과 지연된 worker의 묶음 처리를
연결한다. 수정 전 유음 32,768프레임 중 59프레임이 빠졌다. frontier 후퇴 시 좌표를 초기화하는
분기만 제거하면 이 회귀는 통과했다. 기존 늦은 앱 합류 시험은 그 변경으로 실패했으므로
초기화 자체를 없애지 않는다. 최소 frontier를 제공하는 입력의 신원·epoch를 시각과 함께 전달하고,
다른 입력이 현재 좌표보다 오래된 frontier로 합류할 때 재획득한다. 같은 입력의 스케줄링 reserve
변화에는 연속 좌표를 유지한다. 부분 블록 회귀와 기존 늦은 입력·시작 PCM·독립 클록 정렬이
함께 통과했다. 관련 Rust·네이티브 1,027개와 스크립트 625개가 통과했고 새 회귀의 할당은 0이었다. 설치본의 각 누락이
같은 분기였다는 런타임 추적은 아직 없으며, 새 후보의 강한 녹음 gate로 수락을 다시 확인한다.

별도로 26 ARM 새 설치의 `AUDIO-01`은 144,000프레임 모두 일치했고 누락·중복·손상·slip은
0이었으나 `complete=false`였다. 저장된 228,864프레임은 probe 용량 228,992프레임 직전이다.
기존 JSON은 integrity counter별 값을 남기지 않아 실제 마지막 원인은 확정하지 못한다.
합성 callback에서는 요청 구간이 끝난 뒤 control stop이 지연되면 overflow하는 결함이 재현됐다.
현재 probe는 완전한 최종 callback까지 보존한 뒤 audio owner가 저장을 닫으며 구간 안의
overflow는 계속 실패로 처리한다. 원래 강한 PCM·완료 assertion은 유지하고 integrity counter도
JSON에 기록한다. 버퍼 용량·오디오 quantum·허용 오차는 바꾸지 않았다.

## `f27da5c` 전체 설치본과 주기별 실행

[전체 실행 40315961318](https://github.com/mixrelay/mixrelay/actions/runs/40315961318)의
candidate digest는 `a61afd75f278479af78cac1010bba1f267de2ace59583ab2e0062c590c642431`이다.
macOS PKG SHA-256은 `9f07a01a1449b765b838d10ab2ea692e185c911e07f21e3240db913c861eba5f`다.
13개 중 Linux 7개와 macOS 14·26·27 ARM이 통과했고 15 ARM·15 Intel·26 Intel은 실패했다.
제거는 13개 모두 통과했고 필수 시나리오 skip은 0이다. source CI도 13개 통과했다.
Arch의 첫 소스 검사는 컴파일 전 Docker Hub DNS timeout이었고 재실행에서 통과했다.

| 환경 | 외부 PCM·콜백 | 종료 녹음 |
| --- | --- | --- |
| 14·26·27 ARM | 새 설치·업그레이드의 무음·유음 양 순서 모두 통과 | 각 유음 144,000프레임, 누락·중복·식별 불가 유음·nonfinite 0 |
| 15 ARM | 업그레이드 유음→무음의 무음 좌표 1,824프레임 누락 | 네 유음 구간 모두 누락·중복·삽입 공백 0 |
| 15 Intel | 새 설치의 무음→유음 콜백 612·1,024·1,504프레임 누락, 업그레이드에는 48,308프레임의 콜백 좌표 점프 | 새 설치 유음 누락 3,140프레임은 소스의 미게시 구간과 일치, 업그레이드는 완전함 |
| 26 Intel | 새 설치 AUDIO-01·REC-02와 외부 대조 실패 | 새 설치의 source 녹음이 packet queue overrun으로 실패 종료; 보존 파일의 오프라인 분석상 두 유음 구간은 완전함 |

15 ARM의 문제 콜백 간격은 약 40,813.79µs, 본문은 13.5µs였다. 소스 callback의 최대
본문은 343.2µs, 보존된 HAL 작업 본문 최대는 약 312.3µs다. CoreAudio는 같은 구간에
`Deadline is prior to Start time`과 `Audio IO Overload`를 기록했다. TCC 관찰은 이미 해당
측정 경계에서 중지돼 있었고 stack sample은 사건 이후다. 이 기록만으로 특정 작업이
호스트를 지연시켰다고 단정하지 않는다. 원시 시각과 인과 한계는 로컬 진단의
`40315961318/macos-15-arm-timing-cause.json`에 보존했다.

같은 엔진은 10초에 공유 source로 24,105번 그래프를 실행했다. 선언한 worker period는
48kHz에서 128프레임이므로 10초의 주기는 3,750개다. 연속 playhead의 작은 변화가 즉시
작업을 풀어 불필요한 실행을 만들었다. Loopback ACP `0x1a748`의 완료 주기 계산과 비교해,
host-clock 투영을 `virtual_pcm::cadence`로 분리하고 기존 128프레임 경계로 제한했다.
같은 상수가 실시간 period 계산에도 쓰인다. mixer는 native 좌표의 forward cursor를 유지해
늦게 깨어난 뒤에도 보존된 PCM을 순서대로 배출한다. 기존 부분 게시·늦은 입력 합류·정렬
검사도 유지한다. 각 CoreAudio stall의 원인이 이 실행 빈도였다는 런타임 증명은 아직 없다.

실제 공유 VM 회귀는 sample마다 poll한 뒤 1,024프레임 늦은 wake를 주입한다.
수정 전 1,032회, 수정 후 16회 처리하며 같은 2,048프레임을 비트·순서 일치로 전달한다.
실시간 할당은 0이다. 관련 Rust·네이티브 1,028개, 스크립트 626개와 all-targets clippy·fmt·
크레이트 계층 검사가 통과했다. 버퍼 확대·허용 오차·실시간 예산 변경은 포함하지 않는다.

녹음기는 admission 실패 뒤 수락한 prefix를 배출하고 파일을 마무리한 다음 Failed를
게시한다. 설치 시험도 이 terminal 상태의 파일·해시·분석을 보존한다. 원래 오류는 독립적으로
실패 목록에 남아 유음이 완전해도 통과할 수 없다. 합성 회귀는 완전한 유음과 packet queue
overrun을 함께 주고 이 조건을 확인한다. 후속 후보의 설치본 수락은 아직 미완료다.
