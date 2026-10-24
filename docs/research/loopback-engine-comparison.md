# Loopback 배포 엔진과 MixRelay의 CoreAudio 경로 비교

초기 조사: 2026-10-16(KST), 기준 커밋 `fae815ff59ff3ad67353e1ceb019d9f1202bba42`.
안정성 경계 후속 대조·구현: 2026-10-17(KST), 기준 커밋 `4a9b064a`, 형식 snapshot 보호 `139b3cef`, 늦은 입력 합류 `df523ee`, 앱 재실행 `cf6d868`, 정렬 경로 범위 `fdd2859`, 최초 PCM 승인·획득 시각 지연 측정 `4252373`, 단일 입력의 시각 읽기·PCM epoch·발행 묶음 `981f8b8`.
이 문서는 연구 근거이며 별도 제품 backlog가 아니다. 제품 범위와 진행 상태는 [product.md](/Volumes/dev/mixrelay/docs/product.md:396), 설치본 검증 결과는 [current.md](/Volumes/dev/mixrelay/docs/validation/current.md)를 따른다.

## 2026-10-17 안정성 경계 대조

Loopback 2.5.0의 `arkaudiod`, ACP, ARK 배포 바이너리에서 추출한 함수 본문을
아래 실행 경계별로 읽고 MixRelay의 실제 호출 경로와 대조했다. 기존 추출물을 재사용했으며
새 분석 서버·추가 에이전트·UI 개발 서버는 실행하지 않았다. 함수명만으로 동작을 추정하지 않고,
호출자와 해제·실패 분기까지 확인한 범위를 아래에 기록한다. 모든 장비·OS 내부 상태의
무고장을 증명한 문서는 아니다. 이전 조사 당시의 구현 공백은 아래 소스 변경으로 갱신한다.

| 경계 | Loopback에서 확인한 코드 | MixRelay 판정·적용 |
| --- | --- | --- |
| 입력 시간·독립 클록 | ACPClockConverter `0x5cc04`와 ACPAudioClock `0x66f7c`가 source sample→host→destination sample을 연결하고 ECGKSourceAudioNode가 시간 주소 큐를 사용 | **누락 보강.** native ring→부분 읽기→sinc SRC→application IPC v5→graph까지 프레임별 획득 시각을 유지. 하드웨어 입력도 같은 좌표를 운반하고 공통 획득 cursor로 읽음. [변환](/Volumes/dev/mixrelay/crates/audio-io/src/clock_bridge/timestamps.rs), [graph 정렬](/Volumes/dev/mixrelay/apps/engine-service/src/macos/mod.rs) |
| 큐의 빈 구간·덮어쓰기 | [ACPAudioQueue read `0x1c64c`](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:21030), erase `0x1d2f8`은 보유 밖 구간을 0으로 채우며 시간 구간을 관리. [ECSharedQueue read `0x14694`](/Volumes/dev/loopback/ghidra/export/ARK/decompiled.c:14305)도 범위 밖을 0으로 채움 | **시각 메타데이터와 PCM의 결합 보강.** private FIFO는 예약 성공 후에만 좌표를 쓰고 PCM release 전에 publish. 가득 찬 큐의 실패 write가 읽지 않은 좌표를 바꾸지 못함. 공유 큐는 epoch/frame tag와 lease를 복사 전후 확인. 오래된·미래 범위 재생 금지. [링 예약](/Volumes/dev/mixrelay/native/pcm-ring/src/ffi.rs), [공유 시각 읽기](/Volumes/dev/mixrelay/native/macos/common/source_registry/timestamps.rs) |
| 준비 완료·부분 시작 실패 | ECIOAudioContextPhysical aggregate 생성의 채널 출현 관찰. 기존 조사에서 제한 시간 뒤 함수가 실패를 반환하지 않는 분기도 확인 | **누락 보강.** `alive`에 더해 input stream ASBD·채널 합·callback buffer configuration·sample rate를 확인해야 등록 허용. 기존 유한 재확인 예산을 유지하며 미완료는 오류로 반환하고 부분 owner 회수. 동기 CoreAudio IPC 자체의 취소 시간 보장과 구분. [readiness](/Volumes/dev/mixrelay/native/macos/process-tap/src/native/readiness.rs) |
| 사용하지 않는 입력 | [ECIOAudioContextDevice `_updateStreamUsage` `0x100039c50`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:41422)는 output IOProc의 모든 input stream을 끔 | **누락 보강.** graph에 하드웨어 입력이 없는 output owner만 input usage를 전체 길이 0으로 설정·readback하고 stream identity 재확인 후 Start. direct duplex graph는 input을 유지하며 입력 추가 시 owner 재준비. 입력 0개인 출력도 허용하고, 이후 input stream 출현 알림에는 등록을 차단해 재설정. 다른 IOProc의 입력 사용은 변경하지 않음. [사용 범위](/Volumes/dev/mixrelay/native/macos/common/device_io/stream_usage.rs), [graph 준비](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/recovery.rs). 공개 [Apple 계약](https://developer.apple.com/documentation/coreaudio/audiohardwareioprocstreamusage) 사용 |
| CoreAudio 재시작·장치 ID 재사용 | [ACPDeviceManager init `0x3398c`](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:39959)는 `dev#`와 `srst`를 감시. `_updateDeviceInfo` `0x353b0`은 reference별 disconnect/reconnect를 구분 | **누락 보강.** 물리 IOProc의 `srst` 즉시 차단, 등록별 알림 재설치, persistent UID 재해석으로 숫자 ID/형식만 같은 타 장치 거부. process tap도 service epoch를 별도로 추적하고 기존 native bundle selector가 같아도 재생성. 재시작 후 system listener 자체도 다시 설치. SDK `AudioHardware.h`와 [서비스 재시작 알림](https://developer.apple.com/documentation/coreaudio/kaudiohardwarepropertyservicerestarted)의 계약을 따름 |
| 삭제된 native 자원·종료 순서 | ECIOAudioContextPhysical `_willStop` `0x1000233e4` → `_destroyDeviceIOProcID` `0x100024744` → aggregate 파괴. [graph invalidate `0x100031698`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:36059)는 player/source를 멈추고 관리 컬렉션에서 제거 | **기존 drain 유지 + 누락 보강.** process tap teardown의 `!obj`·`!dev`만 삭제 확인으로 인정. 다른 오류는 정확한 native receipt를 보존. 삭제 확인 뒤에도 local callback/listener guard가 모두 빠져야 메모리 회수. 물리 lifecycle은 이미 같은 삭제 확인 정책을 사용. [tap 수명](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs), [물리 수명](/Volumes/dev/mixrelay/native/macos/common/device_io/lifecycle.rs) |
| 무음·캡처 고착 | `_updateWatchdogs` `0x100011b54` → 독립 diagnostic → `_resolveWatchdog...` `0x1000126f8` → 원본만 고착이면 `recreateTap` | **기존 구현 재확인.** 3초/8개 callback 관찰, 1초 보조 tap, 원본 회복 시 교체 취소, 실패 후 intent/resume 유지. 실제 무음으로 인한 주기적 재생성 없음. [watchdog](/Volumes/dev/mixrelay/native/macos/process-tap/src/watchdog.rs), [회귀](/Volumes/dev/mixrelay/native/macos/process-tap/src/tests/watchdog.rs) |
| IPC 상대 종료 | [ECSharedQueue invalidation `0x14570`](/Volumes/dev/loopback/ghidra/export/ARK/decompiled.c:14097)의 Mach notifier, ECVirtualDevice client-death의 output queue/clock 제거, [ECAudiodGraphManager `0x10004d500`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:56280)의 해당 client graph 제거 | **기존 구현 재확인.** controller pipe·OS 소유권 lease, XPC authority generation, mapping producer lease, reader pin, consumer claim으로 stale 상대와 늦은 완료를 분리. 회수는 control thread. [엔진 소유권](/Volumes/dev/mixrelay/crates/engine-process/src/child.rs), [HAL reconnect](/Volumes/dev/mixrelay/native/macos/hal-driver/rust/src/control.rs), [source registry](/Volumes/dev/mixrelay/native/macos/common/source_registry.rs) |
| graph 변경·회수 경합 | graph invalidate는 target update suspend/resume으로 native source/player 변경을 묶음. graphRunner disconnect `0x100032424`는 runner를 invalidate하고 activation을 재평가 | **기존 구현 재확인.** graph·recovery publication 두 lane, format reservation, DSP block 경계의 matching plan 교체, control에서 retirement. RT callback은 native owner나 DSP를 해제하지 않음. [handoff](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/handoff.rs), [DSP exchange](/Volumes/dev/mixrelay/crates/audio-engine/src/lib.rs:282) |
| scheduler 지연·클록 단절 | [ACPStableIOClock timer `0x1a748`](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:19263)는 현재 시각의 다음 quantum을 예약. ACPUnstableClock은 sample/host/frame count로 추적 | **기존 보호 유지.** native demand만 graph credit으로 사용, catch-up 상한 초과는 discontinuity, clock owner 교체와 실제 재시작 구별. 시각 정렬 cursor도 스케줄러 wake 시각을 입력 획득 시각으로 쓰지 않음. [demand](/Volumes/dev/mixrelay/apps/engine-service/src/render_demand.rs), [alignment](/Volumes/dev/mixrelay/crates/audio-io/src/capture_alignment.rs) |
| 버퍼·메모리·비정상 PCM | [ECSharedQueue 생성·map `0x14240`/`0x14338`](/Volumes/dev/loopback/ghidra/export/ARK/decompiled.c:13964), ACPAudioQueue의 고정 크기와 modulo 저장을 확인. 바이너리 일부에서 할당 결과 검사가 보이지 않는 것을 전체 제품의 OOM 결함으로 일반화하지 않음 | **우리 보호 유지·개선.** 공유 헤더/정렬/정확한 extent·채널·버전·epoch 검사, 준비 단계의 fallible PCM/coordinate 예약, source/processor 출력의 nonfinite 정규화와 fault 집계, bounded source/retirement admission. 외부 라이브러리와 프로세스 전체 OOM 무고장을 주장하지 않음. [mapping](/Volumes/dev/mixrelay/native/macos/common/application_source_mapping.rs), [PCM 경계](/Volumes/dev/mixrelay/crates/audio-engine/src/render.rs) |
| 외부 플러그인 중단 | [RAAUInstance invalidation `0x10009ea68`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:119105)은 해당 unit의 invalidation을 확인하고 main queue로 처리 | **기존 구현 재확인.** 스캐너/processor child 격리, bounded shared packet, 늦은 출력의 원래 frame tag 유지, 지연이 맞는 dry fallback, 종료 child의 kill/reap. native 플러그인 mutex·state/editor 호출은 엔진 callback에 넣지 않음. [processor 부모](/Volumes/dev/mixrelay/crates/processor-host/src/lib.rs:1492), [공유 PCM](/Volumes/dev/mixrelay/crates/processor-host/src/shared_transport/audio.rs), [수명](/Volumes/dev/mixrelay/crates/processor-host/src/dsp_lifecycle.rs) |

개선은 Loopback 내부 코드를 그대로 옮기는 방식으로 하지 않았다. 원래 큐의 덮어쓰기·미래 읽기·epoch 혼합을
검사하며, 재시도는 유한하고 RT 경로는 기다리지 않는다. `AudioDeviceDuck` 같은 private 호출이나
전역 hardware-sleep 설정은 도입하지 않았다. 추가한 장치·회수 보호는 공개 API와 명시적인 소유권으로 구현했다.

검증: 독립 물리 입력 44.1/48kHz, +300/−200ppm, 64/512-frame callback, 별도 출력 pacing,
20초 합성 신호에서 초기 0.25초 이후 197개 이벤트가 모두 1 graph frame 이내로 정렬됐다. RT graph 처리 할당은 0이다.
정렬을 의도적으로 끄면 같은 회귀가 183-frame 불일치로 실패한다. SRC 자체는 37-frame 부분 읽기와
impulse 중심 좌표 oracle로 따로 검사한다. 좌표 overwrite, epoch 경합, stale read·future read,
서비스 재시작·알림 재등록의 부분 실패, ID 재사용, 삭제와 transient 오류, active callback/listener drain은 별도 회귀에서 확인한다. 획득 시각이 없는 입력 하나가 정상 입력까지 무음으로 묶지 않는 것도 실제 native/SRC/graph 경로로 검사했다. 이전의 모든-input 준비 gate를 재주입하면 정상 경로까지 `[0, 0]`으로 남아 같은 시험이 실패한다. 두 결함 주입 후 원래 소스를 복원했다. 48→44.1kHz refresh 뒤 callback이 초기 nominal rate로 좌표를 계산하던 오류도 실패 회귀로 확인하고, 현재 atomic rate를 사용하도록 수정했다.
형식 통계도 generation을 먼저 acquire하고, admission과 마지막 generation 검사로 혼합 관찰을 거부한다.
구형 레이트와 새 generation이 섞여 새 PCM을 이전 SRC에 전달하던 경합은 결정적 재현 시험으로 확인했다.
별도 `format_ready`는 native `running`과 구분하며, 전환 중에는 PCM·clock credit을 보류하고 pump는 재시도한다.
데스크톱은 native stats의 Rust ABI 타입을 직접 사용한다.
설치본 후속 시험에서는 앱→녹음 입력은 정상인데 앱→가상 출력에서 무음이 남았다.
늦게 준비된 입력의 PCM 경계가 과거로 이동해도 공통 cursor가 앞선 시각을 계속 요구하는 회귀를
실제 native callback·공유 source registry·compiled graph로 재현했다. 경계가 역행하면 공통 좌표를
재획득하고 각 입력의 forward-only 커서로 반복 재생을 막는다. 기존 정상 입력과 늦은 앱 입력이
함께 전달되는 것·RT 할당 0을 검사했다. 재획득 조건을 제거하면 `[약 0.1, 0]`이 되어 같은
회귀가 실패한다. scheduling clock도 주입 가능한 경계로 옮겨 실제 경과 시간에 기대지 않는다.
후속 설치본에서는 Installer의 직접 자식으로 재실행된 runtime의 TCC 책임 앱이 Installer로
귀속되어 실제 캡처가 무음인 문제도 분리했다. 같은 서명 바이너리를 Launch Services로
다시 열면 책임 앱이 MixRelay가 되고 앱→가상 출력·녹음 입력 모두 정상 PCM을 전달했다.
설치기의 재실행도 공개 [NSWorkspace 앱 실행 API](https://developer.apple.com/documentation/appkit/nsworkspace/openapplication(at:configuration:completionhandler:))로 변경했다.
이 결과는 native callback 수만으로 권한·PCM 성공을 판정할 수 없음을 보여 준다.
후속 물리 입력 경유 지연 보고는 두 번 시간 초과했다. 별도로 연결 없는 source와 listener가 없는
capture snapshot이 다른 경로의 시각 정렬에 참여하는 결함을 재현해, 실제 외부 sink/recorder에
이르는 경로만 정렬에 참여하도록 보강했다. 독립 FIFO 소비와 미터는 유지한다.
13:28 KST 후속 패키지 설치 직후에는 별도 수동 재실행 없이 앱→가상 출력·녹음 입력이 모두
정상 PCM을 전달했고, 지연 보고 timeout도 해소됐다. 그러나 5초 동안 임펄스의 실제 지연이
약 2,496→2,268프레임으로 변해 왕복 시험은 수락하지 않았다.
후속 소스는 최초 그래프 실행 전에 쌓인 PCM/필터 이력을 callback 소유자가 회수한 뒤
승인하도록 하고, 피드백 목표의 converter slab을 scratch 크기 256 대신 실제 128프레임으로
계산한다. 준비 PCM 유출과, 동일 클록에서 60개 이벤트의 지연이 약 93프레임 이동하는 것을
각각 실패 회귀로 확인했다. 둘을 수정하면 이벤트 지연 변동은 2프레임 이내다.
입력 지연도 각 PCM의 획득 좌표에서 graph 전달까지 직접 측정한다. 이는 SRC·보존 큐 시간을
이미 포함하므로 별도 큐 평균과 filter delay를 다시 더하지 않는다. 가변 callback에서 기존 평균
합산이 48프레임 오차 한도를 넘긴 회귀를 수정하고, 부분 읽기의 프레임 가중·시각 부재/미래 시각의
관찰 거부도 검사한다. 설치본 지연 재확인은 검증 현황에서 별도로 갱신한다.
13:57 KST 설치본도 임펄스 20개는 도착했지만 지연이 약 43.5프레임 이동해 수락하지 않았다.
단일 입력 경로가 보존한 획득 시각을 읽지 않고 FIFO를 소비하는 분기를 후속으로 확인했다.
현재 소스는 독립 클록 입력이 하나여도 worker의 시각 읽기를 사용하고, 같은 IOProc의
native 입력만 직접 렌더링한다. callback 적용 전 좌표를 막고 graph-input producer epoch를
갱신하며, 소비자는 그 epoch를 확인한 후 좌표를 선택한다. callback 여러 개가 한 graph
주기 사이에 몰릴 때는 실제 SRC 발행량을 여유분에 포함한다. 앞/뒤 순서와 1·2·4개 묶음의
여섯 경우에서 각 60개 임펄스·2프레임 이내 지연 변동·RT 할당 0을 확인했다.
시각 선택을 위한 의도적 history skip은 유효한 PCM age가 있는 경우에만 지연 측정에 쓰며,
시각 부재/미래 시각과 native/SRC epoch 손실은 계속 관찰을 거부한다.
`981f8b8` 설치 엔진의 hash 일치를 확인한 후, 실제 단일 입력 왕복은 20/20 임펄스 정렬·
지연 변동 0.9프레임·보고 오차 2.09프레임으로 통과했다. 설치 앱의 캡처→가상 출력도
양쪽 채널 약 0.05·fault 0으로 통과했다. 다중 독립 입력과 장시간·장애 복구의 설치본
수락까지 포함한 결과는 아니다. 설치가 필요 없는 동작과 발견한 실패 조건은 유닛 테스트로
먼저 재현·검증하며, 설치본은 실제 권한·서비스·장치 경계의 최종 확인으로 한정한다.
소스 검사와 설치본 수락 결과는 [검증 현황](/Volumes/dev/mixrelay/docs/validation/current.md)에 구분한다.

LG HDR 4K는 사용자 결정에 따라 **CoreAudio 단독 시작도 실패하는 현 장치 상태의 예외**로 둔다.
raw function/block IOProc·AUHAL·독립 ARK clock aggregate에서도 Start가 약 10초 뒤 `stop`, callback 0으로
끝났다. LG SDQHD와 built-in 출력은 별도로 통과했다. Loopback UI meter가 움직인 관찰은 HDR 물리
callback의 성공 증거가 아니며 사용자는 두 LG를 Loopback에서 시험하지 않았다고 답했다.
CoreAudio를 강제 재시작하거나 장치 설정을 반복 변경해 제품 해결로 표시하지 않는다.

## 2026-10-16 초기 결론과 근거

이하의 “현재 구현 공백”은 초기 기준 커밋에 대한 기록이다. 최신 구현·판정은 위 후속 대조와 product.md를 따른다.

**처음 앱 캡처 시험이 IOProc 등록에서 멈춘 것은, 현재 근거상 CoreAudio 등록 API를 잘못 골라서가 아니라 첫 시스템 오디오 권한 요청이 끝나지 않은 상태를 시험자가 놓친 문제다.** 실패한 PID `9978`의 요청과 TCC의 `AUTHREQ_PROMPTING` → `AUTHREQ_RESULT` → `REPLY`가 같은 `msgID=43699.556`로 연결된다. 승인 이후에는 block 등록과 기존 function 등록이 모두 실제 PCM 네 단계를 통과했다. 이 실패를 제품 엔진의 영구 교착이나 `AudioDeviceCreateIOProcID` 자체 결함이라고 설명할 수 없다. [실패 스택](/tmp/mixrelay-native-tap-smoke-sample-20261016.txt:24), [해당 요청의 TCC 로그](/tmp/mixrelay-tap-first-permission-request-20261016.log:15), [block 비교](/tmp/mixrelay-tap-registration-block-comparison-20261016.log:1), [function 역검사](/tmp/mixrelay-tap-registration-function-reversal-20261016.log:1).

**그렇다고 Loopback과 같은 수준의 엔진이 완성된 것은 아니다. 실제로 확인한 구조 차이는 다음 세 가지다.**

1. Loopback은 캡처한 PCM의 소스 sample time을 host time을 거쳐 목적 클록 sample time으로 옮기고, 그 시각을 주소로 하는 큐에 쓴다. MixRelay는 sinc SRC·FIFO 점유량 PI·epoch 복구가 실제 경로에 있지만 PCM 범위별 원래 시각이 ring → SRC → 공유 메모리 → 그래프까지 이어지지 않는다. **독립 입력의 절대 시각 정렬은 실제 구현 공백이다.** [Loopback 변환](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:70260), [소스 큐 연결](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:23523), [우리 pump](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:403), [우리 공유 계약](/Volumes/dev/mixrelay/native/macos/common/application_clock.rs:17).
2. Loopback은 프로세스 × 장치 × 출력 스트림별 tap을 구성한다. MixRelay의 일반 앱 캡처는 스테레오 mixdown이며, 소유한 MixRelay 가상 장치의 원음 차단 scoped 경로는 출력 스트림이 정확히 하나일 때만 받아 stream index `0`으로 연다. **Loopback의 임의 장치·스트림 선택과 현재 우리 owned-endpoint 정책은 범위가 다르다.** 일반 앱 캡처 전체가 단일 출력 스트림 장치만 지원한다는 뜻은 아니다. 이는 이번 단일 스트림 fixture의 권한 대기 원인과 별개다. [Loopback 스트림 열거](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:38273), [우리 단일 스트림 판정](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:503), [우리 description](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:106).
3. Loopback은 명시적인 권한 상태 계층, 장치별 clocked/unclocked 정책, aggregate 채널 출현 관찰, autostart 실패 관찰을 조합한다. MixRelay에도 준비·소유권·복구가 있으나 정책과 관찰 항목이 다르다. **이 차이를 무조건 복사하거나 LG 시작 대기의 원인으로 확정하면 안 된다.** API 전제와 실제 장치 조건을 맞춘 실험이 먼저다. [Loopback 권한](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39093), [클록 정책](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48504), [우리 aggregate 준비](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:433).

## 근거의 범위와 식별

이 조사는 Rogue Amoeba의 공개 소스 저장소를 읽은 것이 아니다. **보관된 상용 배포 바이너리의 디컴파일·패키지 메타데이터를 읽고 우리 저장소와 대조했다.** 새 디컴파일 서버·빌드·Node 서버·Docker·실환경 시험은 실행하지 않았다. 실행 관련 결론은 아래 기존 시험 로그에 한정한다. 이번 연구 자체가 Loopback의 모든 실행 분기를 관찰한 것은 아니다.

| 대상 | 버전/식별 | SHA-256 |
| --- | --- | --- |
| Loopback arm64 실행 파일 | 앱 Info.plist `2.5.0`, `com.rogueamoeba.Loopback` | `e6e0b925bc84601559ea93904a312d53321eecded3f819eeaf5fe181de34813f` |
| arkaudiod arm64 실행 파일 | ARK `13.5.2`, `com.rogueamoeba.arkaudiod` | `24bb0995a857a5155a9b46e3725c087352431f399ac23fbae871854673ae2657` |
| ARK arm64 HAL | `13.5.2`, `com.rogueamoeba.ARK.driver` | `6d3593a316192e07e0970a6e110714170ee1d03c1b53327bcb01119ef3473b0d` |
| ACP arm64 앱 내장 사본 | 보관 Loopback 내장 프레임워크 | `0c7e7a1b6f017377caec0585a163810d54d7de1f9d50bfe477a08a1592866930` |
| ACP_ark arm64 helper 내장 사본 | 보관 ARK helper 내장 프레임워크 | `27cc3fff48d64b51822388630610e240d6b5d1687b382f426438cef7b16caf78` |
| arkaudiod `decompiled.c` | Ghidra 12.1.4, 이미지 base `0x100000000` | `f352e663aa8299d743631c38be2e00647a9eaec79d22efabf5639ff6b4b3ee36` |
| ACP `decompiled.c` | Ghidra 12.1.4, 이미지 base `0` | `97083c471f1d3126f4db4d2cb486e96dc029ad106bc7136f044390b7ec8971a1` |

바이너리 원본은 `/Volumes/dev/loopback/bin/arm64/`, 패키지는 `/Volumes/dev/loopback/bin/app/Loopback.app`과 `/Volumes/dev/loopback/bin/ark/ARK.driver`다. 분석 메타데이터는 [arkaudiod meta](/Volumes/dev/loopback/ghidra/export/arkaudiod/meta.tsv:1), [ACP meta](/Volumes/dev/loopback/ghidra/export/ACP/meta.tsv:1). ACP와 ACP_ark의 전체 파일 hash는 다르지만, 직접 읽은 `__TEXT,__text`는 둘 다 주소 `0x1310`, 546,728바이트, SHA-256 `5def9649a831d44be34e84efbb724c81791595b01082a3abac29a1b833163433`로 일치한다. 이 보고서는 공통 코드의 보관 ACP 디컴파일을 인용한다. 주소는 ASLR 적용 전 보관 이미지 주소다. Ghidra의 ObjC 메서드 인자 추론·미사용 임시값은 원래 소스와 같다고 보장되지 않으므로 메서드 selector, 조건, 호출 대상, 데이터 흐름을 기준으로 읽었다.

판정 표기:

- **사실:** 정적 호출/데이터 흐름 또는 특정 실행 로그에서 직접 확인.
- **강한 추론:** 연결된 정적 근거가 요구사항 또는 원인 후보를 강하게 뒷받침하지만 해당 조건의 실행을 확인하지 못함.
- **미확인:** 필요한 상태·실행·비교군이 없어 판별하지 못함.

## 먼저 읽어야 하는 Apple 계약

API 명세와 배포 바이너리의 선택은 구분한다. 현재 로컬 SDK는 **macOS 27.0**이며 아래 헤더를 직접 읽었다. 공식 웹 문서의 Markdown도 읽었다. 오래된 Core Audio Overview의 aggregate 제한을 최신 process tap 지원 범위로 그대로 적용하지 않는다.

공식 문서:

- [Capturing system audio with Core Audio taps — Create a tap and an aggregate device / Configure the sample code project](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)
- [AudioDeviceIOProc — Parameters](https://developer.apple.com/documentation/coreaudio/audiodeviceioproc)
- [AudioDeviceCreateIOProcID](https://developer.apple.com/documentation/coreaudio/audiodevicecreateioprocid(_:_:_:_:)), [WithBlock](https://developer.apple.com/documentation/coreaudio/audiodevicecreateioprocidwithblock(_:_:_:_:)), [Destroy](https://developer.apple.com/documentation/coreaudio/audiodevicedestroyioprocid(_:_:))
- [AudioDeviceStart](https://developer.apple.com/documentation/coreaudio/audiodevicestart(_:_:)), [AudioDeviceStop](https://developer.apple.com/documentation/coreaudio/audiodevicestop(_:_:))
- [AudioHardwareIOProcStreamUsage](https://developer.apple.com/documentation/coreaudio/audiohardwareioprocstreamusage), [AudioTimeStamp — Accessing the Data](https://developer.apple.com/documentation/coreaudiotypes/audiotimestamp)
- [NSAudioCaptureUsageDescription — Discussion](https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription)

아래 `AudioHardware.h` 링크는 모두 현재 SDK의 실제 파일이다.

| API 계약 | 우리 실제 호출과 판정 |
| --- | --- |
| function 등록은 함수 포인터와 client data를 묶은 **등록 ID**를 반환한다. Start/Stop/Destroy에는 그 장치와 반환 ID를 사용한다. [SDK 1355–1375](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1355) | tap은 `resources.aggregate`와 반환 `proc`를 보관한다. 실패 시 반환된 부분 ID도 먼저 채택한다. [native.rs 236–248](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:236), [control.rs 15–28](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:15). **준수. function 선택 자체가 위반이 아니다.** |
| WithBlock의 queue dispatch도 **동기**다. queue가 NULL이면 block을 직접 호출한다. queue와 복사된 block은 Destroy까지 보유된다. [SDK 1378–1399](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1378) | Loopback `0x100024654`는 NULL queue를 전달한다([본문](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:25082)). **이 API가 비동기이므로 등록 대기를 피한다는 설명은 틀리다.** 바꾸더라도 block capture 수명·순환 보유·Destroy를 따로 설계해야 한다. |
| Start는 등록된 IOProc를 시작한다. NULL ID 시작은 콜백 없이 하드웨어만 가동하는 별도 사용이며 NULL Stop과 균형을 이뤄야 한다. [SDK 1417–1431](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1417) | tap은 nonnull ID와 유효 lifecycle을 확인한 뒤 시작하고, 성공 후 `running`을 게시한다. [control.rs 30–54](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:30). **준수.** `running=true`는 Start 반환의 의미이며 첫 PCM 수신을 증명하지 않는다. |
| Stop은 IO를 멈추고 Destroy는 등록을 해제한다. client data 수명이 등록보다 짧아서는 안 된다. [SDK 1402–1414](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1402), [Stop 1459–1469](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1459) | tap은 stop 시도 → destroy 성공 → active callback 0 → listener 제거/진행 중 listener 0 → aggregate → tap 순서로 회수하며 실패 소유자를 보존한다. [control.rs 57–120](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:57). 물리 장치도 정확한 `(device, ID)`를 보유한다([lifecycle.rs 62–77](/Volumes/dev/mixrelay/native/macos/common/device_io/lifecycle.rs:62)). **소유권 보호는 구현돼 있다.** Stop 한 번의 반환만으로 전체 객체 해제를 처리하지 않는다. |
| `inNow`는 스케줄 지연을 포함한 콜백 시작 시각, `inInputTime`은 입력 버퍼 첫 프레임 취득 시각, `inOutputTime`은 출력 첫 프레임의 예정 시각이다. [SDK 745–837](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:745) | tap의 snapshot은 `inInputTime`을 사용하고 물리 I/O는 방향에 따라 input/output timestamp를 고른다([tap ffi.rs 33–63](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:33), [device callback.rs 75–105](/Volumes/dev/mixrelay/native/macos/common/device_io/callback.rs:75)). **timestamp 선택은 맞다.** 이후 PCM 구간과의 연결을 버리므로 절대 정렬은 미구현이다. |
| `AudioTimeStamp.mFlags`가 유효한 표현을 정한다. `mHostTime`은 mach host time이며 `mSampleTime`은 해당 클록의 sample frame time이다. [CoreAudioBaseTypes.h 825–880](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudioTypes.framework/Versions/A/Headers/CoreAudioBaseTypes.h:825) | tap은 sample+host 비트 모두를 검사한다. 공통 clock은 유한 sample/연속성/host 증가를 확인한다([ffi.rs 61](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:61), [callback_clock.rs 57–81](/Volumes/dev/mixrelay/native/macos/common/callback_clock.rs:57)). **진단 유효성 검사는 있다.** 유효한 최신 snapshot 하나로 과거 FIFO 전체의 시각을 대체할 수 없다. |
| `AudioDeviceGetCurrentTime`과 `AudioDeviceTranslateTime`은 실행 중인 장치를 요구한다. 요청한 표현을 모두 지원한다고 가정하지 않고 반환된 `mFlags`와 상태를 확인해야 한다. [SDK 1472–1509](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1472) | 현재 캡처 경로는 콜백이 준 시각을 기록한다. 추후 목적 장치 시각 변환을 추가하더라도 정지 장치의 변환 성공을 가정하거나 최신 변환값으로 오래된 PCM 전체에 시각을 붙일 수 없다. **장치 시각 변환 API와 PCM 범위의 시각 보존은 함께 설계해야 한다.** |
| disabled stream의 `mData`는 NULL이어도 크기는 nonzero일 수 있다. 입력 메타데이터는 수정하지 않고, 고정 PCM 출력의 `mDataByteSize`도 임의 변경하지 않는다. [SDK 755–779](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:755) | 물리 callback은 출력 초기 silence 후 metadata/format 검증을 한다([callback.rs 45–73](/Volumes/dev/mixrelay/native/macos/common/device_io/callback.rs:45)). tap ingest도 별도 shape 검증 뒤 읽는다([ingest.rs 8](/Volumes/dev/mixrelay/native/macos/process-tap/src/process_tap_input/ingest.rs:8)). **NULL을 유효 PCM으로 읽는 코드로 판단하지 않았다.** 다중 stream 선택 확장 시 disabled stream도 배열 위치에 남는다는 계약을 유지해야 한다. |
| `IOProcStreamUsage`는 해당 방향의 **모든** stream을 포함하고, 대상 IOProc ID를 지정한다. [SDK 849–866 / 971–980](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:849) | `native/macos` 제품 경로에서 `suse` 설정은 찾지 못했다. Loopback 물리 출력은 사용하지 않는 aggregate 입력 방향을 false로 설정한다([`0x100039c50`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:41422), false block [`0x100039d18`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:41356)). **구현 차이이며 필수 API 위반으로 단정하지 않는다.** 불필요한 입력 활성화·권한 요청 여부를 실제 측정해야 한다. |
| IOProc/IOBlock은 `CA_REALTIME_API`다. callback은 real time 안전성을 요구받는다. [CoreAudioBaseTypes.h 36–49](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudioTypes.framework/Versions/A/Headers/CoreAudioBaseTypes.h:36) | 우리 tap callback은 준비된 ring·고정 peak·원자 snapshot을 사용하고 native 등록/재탐색/파괴는 제어 경로에 둔다([ffi.rs 33–78](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:33)). Loopback에는 callback에서 buffer 객체 생성·lock 호출이 관찰되므로, 그 구현 전체를 RT 모범 답안으로 복사하면 안 된다([`0x100035f24`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39588), [`0x100041da0`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48369)). 이번 정적 연구로 모든 하위 OS 호출의 최악 실행 시간을 보증한 것은 아니다. |

### tap·aggregate의 공개 계약

`CATapDescription`의 장치 stream 선택은 **AudioObjectID가 아니라 stream index**다. 현재 SDK는 기존 process object 선택과 별도로 `bundleIDs`, `processRestoreEnabled`를 macOS 26.0 공개 속성으로 선언한다. 우리 `respondsToSelector` 분기는 이를 실제 선택에 사용하며, 존재하지 않을 때 process object를 재탐색한다. [CATapDescription.h 85–111](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/CATapDescription.h:85), [131–167](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/CATapDescription.h:131), [우리 분기](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:97).

aggregate의 `master`는 main subdevice UID, `clock`은 별도 clock device UID다. 둘 다 있으면 clock device가 time base를 지배한다. `private=1`은 생성 프로세스 안에서만 보이며 영속 장치가 아니다. subdevice/subtap의 `drift`는 해당 멤버의 drift compensation이다. **`tapautostart`는 단순히 켜 두면 좋은 옵션이 아니다.** private aggregate에서 첫 tap 오디오가 올 때까지 Start가 기다리는 동작이다. 따라서 모든 tap에 무조건 켜거나, Start 반환 지연을 이 옵션과 무관한 교착으로 판단하면 안 된다. [AudioHardware.h 1581–1640](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1581), [subtap drift 1882–1895](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardware.h:1882).

현재 공식 sample은 tap 생성 → aggregate 구성 → tap 연결을 설명하고, 앱의 `NSAudioCaptureUsageDescription`과 최초 recording의 시스템 권한 prompt를 요구한다. 문서 sample의 availability 표시는 26.0이지만 본문의 실행 요건은 14.2 이상이며, tap C API도 SDK `AudioHardwareTapping.h`에 14.2로 선언되어 있다. **문서 페이지의 sample availability를 API 최초 지원 버전으로 혼동하지 않는다.** [공식 sample](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps), [SDK 선언](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudio.framework/Versions/A/Headers/AudioHardwareTapping.h:33).

## 실제 연결 경로 대조

| 단계 | Loopback/ARK에서 확인한 연결 | MixRelay의 실제 연결과 차이 |
| --- | --- | --- |
| 앱 선택과 실행 신원 | `LBSourceApplication::ACPSourceRepresentation:forMonitoring:` `0x100009258`가 앱 참조를 ACP source로 변환한다([Loopback 5759](/Volumes/dev/loopback/ghidra/export/Loopback/decompiled.c:5759)). audiod는 장치·process·channel map으로 audio target을 만들고 context를 선택한다([`_updateAudioTargets` `0x100012f0c`, 18161](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:18161)). | Desktop manifest 예약 → `prepare_application_audio` → `CaptureProbeSession::prepare` → `mr_process_tap_create`로 실제 연결된다([state 833–850](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio.rs:833), [native 준비 271–292](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio.rs:271)). 엔진이 대신 앱 tap을 생성하는 구조가 아니다. |
| process membership | Loopback은 `ECIOAudioTap::_processObjectIDs` `0x10001896c`와 `_createTap` `0x100018480`에 process object 배열을 넘긴다. 구형 OS에서는 최근 active 전환 직후 최대 0.2초 지연하는 `FB13904138` 분기도 있다([15400–15520](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:15400)). | `Selection::new`가 공개 bundle selector 지원을 검사한다. fallback은 object+PID와 bundle을 재확인하고 target output 장치도 검사한다([native.rs 367–397](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:367), [544–572](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:544)). Loopback의 0.2초를 우리 모든 OS 경로에 넣을 근거는 없다. |
| 장치/stream별 tap | `ECIOAudioContextTap::_updateChannelUsageAndTaps:` `0x1000358ec`는 장치 `streamBuffers`를 열거하고 index별 `ECIOAudioTap`을 만든다([38273–38375](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:38273)). `_createTap`은 장치 UID/index initializer 또는 stereo mixdown initializer를 고른다([`0x100018480`, 15450](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:15450)). | 일반 앱 tap은 stereo mixdown. scoped mute tap은 **소유 MixRelay 가상 장치**의 출력 stream 수가 정확히 1일 때만 허용하고 index 0으로 생성한다. **임의 장치·스트림 캡처와 채널 보존으로 확장하려면 별도 요구·설계가 필요하며, 현행 원음 차단의 안전 제한 자체를 버그라고 판단하지 않는다.** [native.rs 522–537](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:522), [shim 120–142](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:120), [owned endpoint gate](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:132). |
| 권한·helper | ACP의 audiod 분기가 권한 상태/RPC를 연결하고 arkaudiod가 preflight/request를 수행한다([ACP `0x7fef0`, 98326](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:98326), [`0x80058`, 98680](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:98680), [audiod `0x1000371a8/0x100037278`, 39093](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39093)). helper는 번들과 사용설명 키를 가진 사용자 LaunchAgent다. | MixRelay Desktop에 사용설명 키가 있고 native tap을 호출한다. CLI smoke의 TCC 책임 앱은 별도로 `com.openai.codex`였다. **설치 앱 권한과 CLI 책임 앱 권한을 같은 것으로 취급한 시험 해석이 잘못이었다.** [우리 Info.plist](/Volumes/dev/mixrelay/apps/desktop/src-tauri/Info.plist:16), [TCC attribution](/tmp/mixrelay-tap-first-permission-request-20261016.log:15). |
| 클록 소스 | `ECIOAudioContext::initWithTarget:` `0x100042354`가 unwrapped/unclocked를 결정하고 `_resolveAudioClock:` `0x100042908`가 자기 장치 UID 또는 ARK clock RPC를 고른다([48504](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48504), [48967](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48967)). | tap은 private tap-only aggregate. 물리 다중 장치는 output 우선 primary와 drift slave를 사용하고 호환되지 않으면 독립 bridge를 유지한다([tap shim 151](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:151), [group plan 31](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/group_plan.rs:31), [spec 104](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/spec.rs:104)). 같은 nominal Hz를 같은 클록으로 오인하는 구현은 아니다. |
| aggregate readiness | `_createAggregateDevice` `0x100023e50` → `AudioHardwareCreateAggregateDevice` → `0x1000241a0`의 input/output 채널 수 관찰 → ID 채택 → IOProc 등록([25343](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:25343), [24858](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:24858)). | tap은 aggregate 생성 후 alive를 최대 약 0.5초 관찰한다([native.rs 433–445](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:433)). 물리 group은 약 2초 동안 멤버·활성 목록·채널·primary·drift readback까지 확인한다([aggregate.rs 96–126](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/aggregate.rs:96), [native ready](/Volumes/dev/mixrelay/native/macos/common/aggregate_group/native.rs:102)). **readiness가 전혀 없는 상태는 아니다.** |
| 등록→Start→첫 PCM | `_willStart` `0x100023358` → aggregate/IOProc, `_willStartIO` `0x1000234bc` → frame count/Start. tap의 첫 callback이 `_aggregateIOProcCalled`를 게시하고 audio manager에 실제 시작을 알린다([24268](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:24268), [24670](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:24670), [39535](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39535)). | `initialize_selected` → tap format/ring/listener → aggregate/readiness → register → 별도 start → callback ingest 순서([ffi.rs 100–128](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:100), [control.rs 30](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:30)). callback/PCM 수치는 별도로 있다. Start 반환만으로 첫 PCM을 보장하지 않는다. |
| 무음·멈춤 복구 | 3초 및 8개 quiet buffer → 별도 watchdog context → 1초 관찰 → watchdog만 소리를 들을 때 원본 tap 재생성. 원본이 회복하거나 진짜 무음이면 재생성하지 않는다([`0x100035f24`, 39616](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39616), [`0x100011b54`, 11628](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:11628), [`0x1000126f8`, 11236](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:11236)). | **이미 구현돼 있고 실제 refresh에서 호출된다.** `TapWatchdog::poll` 3초/8개 또는 callback 없음 → 같은 선택의 unmuted diagnostic → positive일 때 retrack; 10초 재시도 간격. [watchdog.rs 57–120](/Volumes/dev/mixrelay/native/macos/process-tap/src/watchdog.rs:57), [control.rs 219](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:219), [pump 제어 refresh](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:485). 새로 없는 기능이라고 분류하면 틀리다. |
| 장치 변경·중단 | device/process notifier → targets 재평가, stream signature 변경은 이번 패스에서 제외해 context를 재구성한다([`0x10000e0dc`, 8751](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:8751), [`0x100046d24`, 51257](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:51257)). | 물리 I/O는 rate/layout/alive/stream format listener가 즉시 invalid latch를 세우고 제어 owner가 다시 연다. graph intent는 offline 상태에도 남는다([listeners.rs 119](/Volumes/dev/mixrelay/native/macos/common/device_io/listeners.rs:119), [callback.rs 130](/Volumes/dev/mixrelay/native/macos/common/device_io/callback.rs:130), [recovery.rs 107](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/recovery.rs:107)). **구현됨. 설치본 장비별 수락은 별개.** |
| SRC와 FIFO | 실제 소스/monitor context가 `ACPAudioConverter` → resampler → clock converter → sample-indexed queue를 호출한다([소스 `0x100041da0`, 48369](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48369), [monitor `0x100005c18`, 2530](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:2530)). | 실제 Desktop pump·device callback이 `AsyncClockBridge`를 호출한다([pump 403](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:403), [callback 133](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/worker/callback.rs:133)). 단순 passthrough 껍데기는 아니다. **SRC/PI와 콘텐츠 시각 주소 지정은 다른 기능이다.** |

## 경로별 상세 판정

### 1. 권한은 설치 성공이나 usage string 존재만으로 해결되지 않는다

ARK helper 번들 `Audio Routing Kit (ARK).app`의 식별자는 `com.rogueamoeba.arkaudiod`이고 audio capture/microphone 사용설명 키가 있다. LaunchAgent는 그 번들의 `arkaudiod --agent`를 실행하며 AssociatedBundleIdentifiers도 helper를 가리킨다. [Info.plist 13–54](</Volumes/dev/loopback/bin/ark/ARK.driver/Contents/Resources/Audio Routing Kit (ARK).app/Contents/Info.plist:13>), [LaunchAgent plist](/Volumes/dev/loopback/bin/ark/com.rogueamoeba.arkaudiod.plist:5).

권한 흐름은 단순 문자열 검색 이상의 연결이 있다. `ACPAudioPolicies::authorizationStatusForAudioCapture:` `0x7fef0`는 audiod 활성 시 client RPC를 사용하고, `requestAccessForAudioCapture:error:` `0x80058`도 `ECAudiodClient::requestAccess:handler:error:`를 호출한다. audiod의 `ECAudiodApplication::authorizationStatusForAudioCapture` `0x1000371a8` 및 `requestAccessForAudioCapture:` `0x100037278`에는 `dlsym`으로 구한 TCC preflight/request 호출이 있다. 별도의 in-process ACP 경로 `ACPAudioContext::audioContextWithSessionRunner:captures:nodes:` `0x37fc`도 Unknown일 때 timer context로 시작을 유예하고 권한 완료 후 재평가한다. **이 in-process 분기가 Loopback의 이번 audiod 실행에서 선택됐다고 주장하지는 않는다.** [ACP 상태](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:98326), [ACP 요청](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:98680), [audiod 요청/상태](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39093), [별도 in-process 경로](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:3393).

우리 제품 경로는 Desktop의 `spawn_blocking` 작업에서 native prepare를 실행한다. UI 주 스레드에서 직접 CoreAudio 등록을 실행하는 경로로 읽히지 않는다. 하지만 native create가 끝나야 `Result`가 나오므로 현재 이 경로에는 **준비 중인 OS 요청의 단계·대기 원인을 구조화해 내보내는 계약이 없다.** 이는 관찰성 차이다. 동일한 첫 승인 조건의 설치 앱에서 사용자에게 어떤 상태가 보이는지는 이번 정적 연구만으로 확인하지 못했다. [application_capture.rs 283–300](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/control_proxy/application_capture.rs:283), [동기 native create](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio.rs:289).

2026-10-16 실행 근거는 명확하다.

| 시각(KST) | 관찰 | 의미 |
| --- | --- | --- |
| 23:20:36.055 | 실패 fixture PID 9978 시작. | [sample 헤더](/tmp/mixrelay-native-tap-smoke-sample-20261016.txt:12). |
| 23:20:36.815 | 같은 PID에 대한 `AudioCapture` 요청이 subject `com.openai.codex`로 PROMPTING. | [TCC 15–17](/tmp/mixrelay-tap-first-permission-request-20261016.log:15). 이 fixture의 책임 앱은 설치 MixRelay가 아니다. |
| 23:21:01.860 | fixture 주 스레드가 `AudioDeviceCreateIOProcID` → `_TellServerAboutStreamUsage` → `mach_msg`에서 관찰됨. | [sample 12 / 24–39](/tmp/mixrelay-native-tap-smoke-sample-20261016.txt:24). 이때 TCC reply는 아직 없다. |
| 23:26:29.065 | 동일 msgID에 `authValue=2`, `authReason=2` 및 REPLY. | [TCC 18–19](/tmp/mixrelay-tap-first-permission-request-20261016.log:18). PROMPTING부터 약 352.25초. |
| 승인 이후 B→A 비교 | block 등록 B와 기존 function 등록 A 모두 preserved/scoped-prepared/scoped-muted/scoped-released에서 nonzero PCM·정상 rms. | [B](/tmp/mixrelay-tap-registration-block-comparison-20261016.log:1), [A](/tmp/mixrelay-tap-registration-function-reversal-20261016.log:1). API 교체의 인과 효과를 입증하지 못했고 A의 영구 실패 가설은 반증됐다. |

따라서 **확인된 시험 실패 원인은 미완료 권한 요청을 놓친 시험 진행·판독 오류**다. 등록 API 내부에서 어떻게 권한 경로를 기다리는지까지 CoreAudio 비공개 구현을 역분석한 것은 아니지만, 동일 PID/요청 시각·대기 스택·승인 뒤 A/B 성공이 서로 맞는다. 이것으로 LG 출력의 별도 Start 대기까지 설명할 수는 없다.

공개 API로 필요한 요구를 충족하는 방안은, 서명된 실제 앱의 usage string을 유지하고 문서화된 tap 시작에 따른 OS prompt를 사용자 시작 동작과 연결하며, 준비 중·첫 callback·첫 PCM·실패를 분리해 보여 주는 것이다. native 작업이 돌아오는 동안 기존 실행 그래프를 보존하고, 화면 요청 취소가 네이티브 메모리 해제로 오해되지 않게 해야 한다. **TCC 비공개 함수를 그대로 채택하라는 권고가 아니다.** 읽은 공식 문서·위 SDK 헤더 범위에서 동일 목적의 전용 공개 preflight/request 함수를 확인하지 못했다. 마이크 승인 API나 ScreenCaptureKit 승인 API를 system-audio tap의 동일 권한 증명으로 대체하지 않는다.

### 2. 클록·aggregate 정책은 두 제품이 다르다

Loopback 기본 context는 다음과 같이 분기한다.

- Device/Virtual context의 Bluetooth 출력 또는 VAD는 unwrapped로 직접 열고 unclocked도 설정한다.
- Tap context는 입력 장치/unwrapped를 거부한다. Bluetooth 또는 stereo-mixdown 가상 대상이면 unclocked가 된다.
- 그 외 clocked 경로는 `closestSupportedSampleRate`와 `ECPlugInClient::clockForRate:error:` RPC로 ARK clock device UID를 구한다. unclocked에서는 대상 장치의 UID를 구한다.

근거: `ECIOAudioContext::initWithTarget:` `0x100042354` [48504–48587](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48504), `_resolveAudioClock:` `0x100042908` [48967–49006](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48967), `ECPlugInClient::clockForRate:error:` `0x10000accc` [6617–6735](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:6617).

`ECIOAudioContextPhysical::_createAggregateConfig` `0x100023b94`는 private=1/stacked=0을 쓴다. clocked이면 ARK clock subdevice를 drift=0으로 넣고 clock/master를 같은 UID로 지정한다. subclass가 physical subdevice 또는 tap을 덧붙인다. physical/tap의 drift는 clocked에서 1, unclocked에서 0이다. tap 목록은 index 순으로 정렬한다. [기본 구성 25188–25311](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:25188), [tap 추가 `0x100035594`, 38393–38496](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:38393), [physical 추가 `0x100039a64`, 41541–41593](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:41541). Ghidra가 이름으로만 남긴 NSNumber 상수는 보관 Mach-O의 주소 `0x1001415b0/5c8/640/658/670/688`의 객체 payload(+16)를 읽어 각각 `1/0/0/1/0/1`임을 교차 확인했다.

tap autostart는 장치·process policy에 따라 켠다. 장치 policy `canDeviceAutoStart:` `0x10001d240`는 출력에 true이며, process policy `0x10001d290`는 bundle/name의 policy DB를 확인한다. `initWithTarget:watchdog:` `0x100034254` → `_appendAggregateConfig:` → `_startIO:` `0x100035d14` → `updateWithTarget:`의 callback 관찰이 연결된다. 대상이 active인데 autostart aggregate가 한 번도 callback하지 않은 상태를 감지하면 autostartDisable과 재구성·재시작 경로가 있다. [policy](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:19902), [init](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:37786), [실패 감지 `0x100035130`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:38057), [재구성 `0x1000349d8`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39750). `canDeviceAutoStart`의 isDefault 호출 결과를 조건이라고 추측하지 않았으며 해당 arm64 `0x10001d240–0x10001d284`도 직접 확인했다.

MixRelay의 tap-only aggregate는 `private`와 `taps`를 지정하고 별도 clock/master/drift/tapautostart를 지정하지 않는다. 물리 group은 같은 nominal rate의 호환 멤버만 묶어 output 우선 primary, 나머지 drift=1/quality=0x60으로 구성하고 실제 composition을 읽어 검증한다. **다른 구성이라고 바로 잘못된 구성은 아니다.** 특히 서로 다른 경계를 OS drift와 우리 PI로 중복 제어하지 않도록, 어떤 시계가 어떤 PCM 큐를 지배하는지 먼저 명확히 해야 한다. [tap shim](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:151), [물리 plan](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/group_plan.rs:31), [native description/readback](/Volumes/dev/mixrelay/native/macos/common/aggregate_group/native.rs:63), [독립 bridge](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/bridge.rs:76).

Loopback의 readiness는 aggregate input 또는 output 채널이 하나 이상 보일 때까지 약 3초(1ms × 약 3000회) 관찰하는 **상한 있는 폴링**이다. 이 helper는 timeout을 오류로 반환하지 않고 끝나며 이후 등록이 진행될 수 있다. 따라서 이를 “3초 후 반드시 정상 장치” 또는 “등록·Start를 취소할 수 있는 watchdog”으로 해석하면 틀리다. 우리 tap alive 약 0.5초와 physical group geometry 약 2초도 **그 getter 하나가 OS 안에서 막히지 않는다는 wall-clock 보장까지 주지 않는다.** [Loopback `0x1000241a0`](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:24858), [우리 tap](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:433), [우리 group](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/aggregate.rs:115).

### 3. 입력·출력·monitor의 시각은 콘텐츠와 연결된다

Loopback 소스 경로는 다음 연결까지 읽었다.

```text
ECIOAudioContextTap::_deviceIO:buffers:                         0x100035f24
  native input sample/host + frame count + safety offset
  → ACPUnstableClock.updateAbsoluteTime:sampleTime:frameCount:
  → ACPAudioBuffer { absoluteTime, sampleTime, PCM }
  → ECIOAudioManager.audioContext:processInput:
  → ECAudiodGraphSourceIOContext.processContextIOBuffer:channelMappings:
      → _ensureValidConverter(context clock → graph clock)     0x100042050
      → ACPAudioConverter.convertInputBuffer:...               0x00037e48
          → resampler + 실제 입출력 frame count
          → ACPClockConverter.convertInputSampleTime:...       0x0005cc04
              → ACPAudioClock.convertSampleTime:toClock:      0x00066f7c
                  source sample → source host → dest sample
      → ECGKSourceAudioNode.processSourceIOBuffer:sampleTime:  0x1000220a8
          → ACPAudioQueue.writeSampleTime:buffer:
graph IO clock의 목적 sample time
  → ECGKSourceAudioNode.processSimpleInput:writeOutput:         0x1000227ec
      → ACPAudioQueue.readSampleTime:...                       0x0001c64c
```

연결 근거: [tap callback 39484](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:39484), [source context 48369](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48369), [converter 준비 48705](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:48705), [SRC 43399](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:43399), [clock converter 70260](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:70260), [clock 변환 77832](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:77832), [queue write 23523](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:23523), [graph read 23823](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:23823).

`ACPClockConverter`는 다음 입력 시작이 이전 입력 끝과 같고 새 목적 시각이 예상 출력 끝에서 기본 2프레임 이내이면 기존 출력 연속 위치를 유지한다. 입력 불연속 또는 목적 오차 초과는 tracking을 reset하고 재획득한다. **샘플 수만 비례 변환하는 함수가 아니다.** SRC가 실제 만든 출력 frame count가 다음 출력 예상 위치를 갱신한다. [constructor `0x5ca48`, 기본 drift limit=2](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:70174), [변환 `0x5cc04`](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:70260).

`ACPAudioQueue`는 sample time을 modulo storage offset으로 바꿔 쓰고, 읽는 시각과 보유 구간이 겹치는 부분만 copy/mix하며 보유 밖 구간은 silence 처리한다. 즉 부분 버퍼와 ring wrap에서도 **시간 주소를 유지한 범위 읽기**가 있다. [write `0x1cee0`, 20903](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:20903), [read `0x1c64c`, 21030](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:21030), [offset `0x1d3b4`, 20999](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:20999). 모든 장치 조합에서 phase가 완벽하다고 동적 검증한 것은 아니지만 콘텐츠 시간 주소 지정이 실제 엔진 경로에 있는 것은 확인된다.

monitor도 `ECIOAudioPlayerIOContext::processSourceIO:buffer:clock:` `0x100005b20` → source별 converter `0x100005e18` → `0x100005cfc`가 목적 시각에 sync offset·safety/buffer 여유를 더해 queue에 쓰고, 목적 callback `0x10000571c`가 목적 sample time으로 읽는다. 사용자 sync offset을 줄일 때 큐를 비우는 분기도 있다. [source→converter](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:2492), [offset queue write](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:2597), [목적 read](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:2320). 별도 stable IO clock `0x1a748`는 현재 host를 sample로 바꿔 다음 절대 quantum을 예약한다. 타이머가 늦었을 때 과거 quantum을 모두 몰아서 처리하는 구조가 아니다. [ACP timer](/Volumes/dev/loopback/ghidra/export/ACP/decompiled.c:19263).

우리 실제 경로에는 다음이 있다.

- native callback은 원래 `inInputTime`의 sample/host와 discontinuity snapshot을 기록한다. [ffi.rs 52–63](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:52).
- `AudioRing`은 samples/read/written/dropped를 보유한다. 구간별 timestamp/길이의 sidecar는 없다. [capture_ring.rs 22–29](/Volumes/dev/mixrelay/native/macos/common/capture_ring.rs:22).
- Desktop pump가 ring을 부분 읽기하고 비정상 float 정규화 → `AsyncClockBridge::push_interleaved/pull_interleaved`를 실제 호출한다. 같은 클록 증거가 있을 때만 SRC를 생략하며, 실제 소비 큐의 점유/진행으로 PI를 돌린다. [pump 274–339](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:274), [403–431](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:403), [sinc 준비](/Volumes/dev/mixrelay/crates/audio-io/src/clock_bridge.rs:129), [PI](/Volumes/dev/mixrelay/crates/audio-io/src/clock_bridge/controller.rs:45).
- `FrameFifo`도 PCM/읽기 cursor/프레임 수만 보유한다. `ApplicationClock` v4는 소비 위치·진행·생산 lease·native frame 수·buffer 측정치를 전달하지만 PCM 범위의 원래 host/sample 시각을 전달하지 않는다. [FIFO](/Volumes/dev/mixrelay/crates/audio-io/src/clock_bridge/fifo.rs:2), [공유 계약](/Volumes/dev/mixrelay/native/macos/common/application_clock.rs:17).
- 엔진 worker와 direct callback 양쪽의 `ExternalIo::source_epoch`가 application lease를 확인하고 그래프 `input_timeline`이 읽기 전/후 epoch 경합을 거부한다. **현재 구현된 timeline은 세션 연속성 보호이며 절대 콘텐츠 시각 배치가 아니다.** [worker](/Volumes/dev/mixrelay/apps/engine-service/src/macos/mod.rs:59), [direct](/Volumes/dev/mixrelay/apps/engine-service/src/macos/direct.rs:43), [graph 보호](/Volumes/dev/mixrelay/crates/audio-engine/src/render/input_timeline.rs:11).

따라서 최신 callback timestamp를 예전 ring 전체에 붙이는 수정은 틀리다. 최소한 epoch + source frame interval + 첫 프레임 host/sample 좌표를 PCM과 함께 publish하고, 부분 read는 좌표를 소비한 만큼 전진시키며, SRC는 실제 consumed/produced frame count와 필터 지연을 따라 목적 시간 구간을 만들어야 한다. FIFO residence는 체류 지연 측정에 유용하지만 이 좌표를 복원하지 못한다.

### 4. rate scalar와 residence는 별도로 해석한다

SDK의 `mRateScalar`는 **실제 host ticks/frame ÷ nominal host ticks/frame**이다. 따라서 scalar가 1보다 크면 native 한 프레임의 실제 시간이 nominal보다 길다. 우리 `native_frame_ns = scalar × 1e9 / native_rate`는 이 정의와 맞고, SRC residence의 nominal 시간 면적에도 scalar를 곱하는 현재 방향을 잘못된 역수 처리라고 분류하지 않았다. [SDK 정의](/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreAudioTypes.framework/Versions/A/Headers/CoreAudioBaseTypes.h:858), [실제 계산](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/latency.rs:110). **프레임 시간 환산이 맞는 것과, 그 PCM이 어느 절대 시각에 속하는지 보존하는 것은 다른 요구다.**

### 5. 복구 기능은 이미 있는 부분과 없는 부분을 나눈다

Loopback의 watchdog은 “무음이면 3초마다 재생성”이 아니다. quiet가 지속될 때 별도 tap으로 원본 무음이 진짜인지 확인하고 그 결과를 원본과 대조한다. `_resolveWatchdogForContext:watchdog:didObserveAudio:` `0x1000126f8`는 보조 tap을 정지시킨 뒤, 보조만 소리를 들었다면 `recreateTap`을 호출한다. `recreateTap` `0x1000348a0`는 기존 IO 정지 → per-stream taps 제거/재생성 → aggregate composition/usage 갱신 → 조건부 Start다. [결정](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:11236), [재생성](/Volumes/dev/loopback/ghidra/export/arkaudiod/decompiled.c:37974).

MixRelay도 이 정책이 실제로 연결돼 있다. 제어 refresh가 watchdog을 호출하고, positive면 `RebuildIntent::Replace`를 남긴 뒤 unregister/drain → tap/aggregate 재생성 → register → epoch commit → 재시작한다. 중간 실패 시 intent/resume을 보존해 후속 refresh가 이어서 수행한다. 생산 callback과 종료가 경합할 때는 lifecycle gate가 먼저 PCM admission을 막는다. [control.rs 219–335](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:219), [lifecycle gate](/Volumes/dev/mixrelay/native/macos/process-tap/src/process_tap_lifecycle.rs:65), [watchdog](/Volumes/dev/mixrelay/native/macos/process-tap/src/watchdog.rs:57).

우리 watchdog의 3초/1초는 native 호출이 정상 반환하는 조건의 관찰 예산이다. **register나 Start 자체가 OS 안에서 기다리는 동안 이 watchdog이 같은 owner를 강제 해제하거나 고쳐 주는 것은 아니다.** 앱이 조용한 상태와 권한을 기다리는 상태, process가 끝난 상태를 같은 “무음 복구” 문제로 합치면 안 된다. 반대로 이 이미 구현된 복구를 다시 넣는 작업도 필요하지 않다.

### 6. LG 출력 시작 대기는 아직 별개의 미확인 문제다

앞선 설치본 검증에서 LG의 standalone `AudioDeviceStart`도 대기한 기록이 있고, built-in/Loopback 조합은 양쪽 멤버 순서에서 통과했다. 그 범위는 [검증 내역](/Volumes/dev/mixrelay/docs/validation/current.md)에 있다. 이것은 “두 번째 aggregate 멤버라서 실패”라는 단일 설명을 지지하지 않는다. 이번 조사로 확인한 후보 차이는 clock master 선택, physical 출력의 미사용 입력 stream disable, 준비 완료의 관찰 조건, 장치 변경 시 재개 순서다. **동일 LG·동일 rate·동일 권한·동일 연결 상태에서 Loopback과 우리 실행을 직접 비교한 증거가 아직 없으므로 어느 차이가 원인인지는 미확인이다.**

## 원인 후보의 현재 판정

| 주장/후보 | 판정 | 근거와 제한 |
| --- | --- | --- |
| 첫 native 앱 캡처 실패 동안 system-audio 승인이 미완료였다. | **사실.** 시험 판독의 가장 직접적인 원인. | 동일 PID9978/msgID의 PROMPTING과 REPLY 사이에 IOProc 등록 대기 스택이 있다. 승인 후 기존 function API도 PCM 통과. |
| `AudioDeviceCreateIOProcID`가 원인이며 WithBlock으로 바꾸면 해결된다. | **현재 반증됨.** | 기존 API 역검사가 통과했다. 두 실행은 모두 승인 이후였으므로 block 교체의 원인 효과가 분리되지 않는다. |
| 우리 설치 앱에 usage description이 없어 실패했다. | **사실과 맞지 않음.** | 소스와 설치 앱에 키가 있다. 실패 fixture의 권한 subject는 Codex였다. plist 키의 존재는 실제 사용자 승인 완료와 별개다. |
| Loopback이 모든 공개 API 전제를 지키고 우리만 위반한다. | **근거 없음.** | function/block 모두 공개 API다. 우리 등록 ID·owner 수명·timestamp 방향 선택은 구현돼 있다. Loopback의 callback lock/buffer 패턴도 무비판적으로 복사할 대상이 아니다. |
| 입력 절대 정렬이 부족하다. | **정적 사실 + 결과에 대한 강한 추론.** | 우리 PCM 범위 시각이 ring/SRC/IPC에서 유지되지 않는다. Loopback은 source→host→dest 변환과 시간 주소 큐가 실제 연결된다. 이 격차가 특정 녹음의 가청 오차 크기라는 실행 증명은 별도 필요. |
| tap aggregate alive만 확인하면 모든 geometry가 준비됐다고 볼 수 있다. | **입증 안 됨.** | 우리 tap은 alive, Loopback은 채널 출현을 본다. 공식 API는 “alive이면 입력 stream/format 준비 완료”라는 동치 계약을 제공하지 않는다. 이번 최초 실패는 권한 근거가 있으므로 readiness 원인으로 재분류하지 않는다. |
| LG는 두 번째 aggregate 멤버이므로 멈춘다. | **미확인, 기존 관찰과 충돌.** | standalone Start도 대기한다. 물리/권한/stream 사용과 클록 조건을 분리해야 한다. |
| unused input stream 활성화가 물리 출력의 불필요한 권한/Start 대기를 만든다. | **검증할 가설.** | Loopback은 false usage를 설정하고 우리 native 경로에는 명시 설정이 없다. 해당 장치에서 실제로 어떤 stream이 켜졌는지, TCC와 연결되는지 관찰하지 못했다. |
| quiet watchdog을 추가해야 한다. | **이미 구현됨.** | 우리 poll→diagnostic→confirmed retrack 호출 경로가 있다. 실제 OS failure injection 수락은 구현 존재와 구분한다. |

## 수정이 필요한 경계와 최소 실험

아래는 연구 결론을 구현·검증으로 옮길 때의 경계다. 제품 진행표를 대체하지 않으며 이번 연구에서는 제품 코드를 수정하지 않았다.

| 경계 | 수정/확인할 위치 | 원인을 가르는 최소 실험과 수락 기준 |
| --- | --- | --- |
| 권한 대기의 관찰과 시험 실행 신원 | [smoke script](/Volumes/dev/mixrelay/scripts/smoke-macos-application-capture.sh), [native 준비](/Volumes/dev/mixrelay/native/macos/process-tap/src/ffi.rs:100), [Desktop 예약](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/control_proxy/application_capture.rs:293) | 실제 서명 앱에서 시작한 첫 capture와 CLI fixture를 구분한다. bundle ID·실행 hash·TCC responsible ID·create/register/start enter/return·첫 callback/첫 PCM을 같은 run ID로 기록한다. 승인 요청 중에는 이를 별도로 표시하고 장치 고장이라고 완료 보고하지 않는다. 재실행은 반환/자원 회수 확인 뒤 한다. 개인 계정의 TCC DB 변경/우회나 무단 reset은 하지 않는다. 미결정/거부 비교가 필요하면 별도 시험 계정을 사용한다. |
| 등록 API 비교 | [native register](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:236), [비교 fixture](/Volumes/dev/mixrelay/native/macos/application-capture/tests/MRIOProcBlockRegistration.m) | 동일 실행 파일/신호/장치/권한이 확인된 상태에서 A(function)→B(block)→A를 반복한다. 각 회차 등록·Start·first callback·PCM rms를 확인한다. **현재 실제 실행한 승인 후 B→A가 모두 성공했으므로 API 교체를 fix로 채택하지 않는다.** 추가 A→B→A 반복은 아직 수행하지 않은 제안이다. 등록 단계별 시간은 진단이며 합격을 위해 timeout을 늘리지 않는다. |
| tap aggregate 준비 | [Description::aggregate](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:433), [tap dictionary shim](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:151) | 이미 승인된 동일 조건에서 alive-only와 input stream count/stream configuration/format readback을 대조한다. geometry가 늦게 나오는 fault를 주입해 조기 등록을 거부하는지 본다. 생성은 한 번만 하고 확인을 반복하며 timeout 뒤 부분 native owner를 안전 회수한다. Loopback의 3초를 그대로 복사하지 말고 실제 조건을 확인한다. |
| 물리 output stream usage·LG 시작 | [공통 control](/Volumes/dev/mixrelay/native/macos/common/device_io/control.rs:83), [native ABI](/Volumes/dev/mixrelay/native/macos/common/device_io/native.rs:117), [group 정책](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/group_plan.rs:31) | LG standalone와 aggregate를 나누고, 해당 IOProc의 input/output stream 수와 usage를 모두 읽는다. output 전용에서 input usage를 전체 길이 false로 한 조건과 기존 조건을 비교한다. 장치/rate/default/권한을 고정하고 생성→readback→register→Start→callback을 구분한다. 정상 출력 PCM, 불필요한 입력 사용 없음, 반복 회수 뒤 owner 0을 수락 조건으로 둔다. Start가 반환하지 않으면 미해결로 남기고 원인이 다른 시험 결과에 합쳐 통과시키지 않는다. |
| 독립 입력의 절대 콘텐츠 정렬 | [capture ring](/Volumes/dev/mixrelay/native/macos/common/capture_ring.rs:22), [pump](/Volumes/dev/mixrelay/apps/desktop/src-tauri/src/macos_audio/clock_pump.rs:403), [bridge/FIFO](/Volumes/dev/mixrelay/crates/audio-io/src/clock_bridge.rs:99), [공유 계약](/Volumes/dev/mixrelay/native/macos/common/application_clock.rs:17), [graph 입력](/Volumes/dev/mixrelay/crates/audio-engine/src/render/input_timeline.rs:11) | 고정 메모리의 시각 sidecar와 PCM을 한 epoch/범위로 publish한다. 64/128/512프레임 callback, partial read, queue wrap, overflow, pause/resume, 44.1↔48k, ±ppm을 합성 입력으로 검사한다. 같은 이벤트의 목적 시간은 **스케줄 지연·부분 읽기 분할에 따라 움직이지 않아야 한다.** 같은 클록은 정확한 frame 위치, 독립 클록+SRC는 알려진 필터 지연을 포함한 oracle의 정렬 오차 기준을 명시해 비교한다. invalid timestamp/epoch 변화 때 과거 PCM을 새 좌표로 재라벨하지 않고 downstream history를 한 번만 reset해야 한다. |
| 임의 장치·stream 캡처 요구를 확장할 경우 | [DeviceLease/Selection](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:350), [장치 판정](/Volumes/dev/mixrelay/native/macos/process-tap/src/native.rs:503), [ObjC description](/Volumes/dev/mixrelay/native/macos/application-capture/src/MRProcessTap.m:106) | 두 출력 stream에 서로 다른 톤을 넣어 index별 캡처를 확인한다. 선택하지 않은 stream·다른 process의 원음은 남아야 하며, Off/해제/소유권 상실 시 차단 lease가 풀려야 한다. stream 재배열·장치 교체는 object ID와 index/geometry 재확인 전까지 이전 mute 권한을 재사용하면 안 된다. 이는 현행 소유 MixRelay endpoint의 제한을 무조건 제거하라는 뜻이 아니다. 일반 stereo mixdown 유지·임의 장치 지원 여부는 product.md에 따른다. |
| 이미 구현된 복구의 실제 장애 수락 | [watchdog](/Volumes/dev/mixrelay/native/macos/process-tap/src/watchdog.rs:57), [control retrack](/Volumes/dev/mixrelay/native/macos/process-tap/src/control.rs:289), [물리 recovery](/Volumes/dev/mixrelay/apps/engine-service/src/macos_devices/recovery.rs:107) | 실제 무음에서는 재생성 0, 원본만 멈추고 독립 diagnostic만 PCM을 받는 경우에만 교체, 원본 회복이 겹치면 교체 취소를 확인한다. 각 native 실패 지점 뒤 후속 refresh가 resume을 마치는지, 종료 후 owner/registration이 0인지 본다. API 자체 대기 중에 제어 watchdog이 복구했다고 판정하지 않는다. |

## 확인하지 못한 범위

- 같은 LG 장치에서 Loopback과 MixRelay를 동일 권한·rate·stream usage·clock master로 맞춘 실행 대조.
- 최초 승인/거부/미결정 상태에서 **설치 MixRelay 앱 자체**의 UI 상태와 TCC attribution. 현재 확정 로그는 Codex가 실행한 native fixture다.
- Loopback의 모든 장치별 policy DB 항목, Bluetooth/다중 stream/무음 autostart 실패 분기의 실동작, 모든 입력·monitor의 장시간 phase 오차.
- 최신 timestamp 하나가 아니라 각 PCM 범위의 시각을 보존하는 우리 전체 설계·구현 및 설치본 수락. 현재 SRC/PI/epoch 구현이 이 검증을 대신하지 않는다.
- 화면·스크린리더 비교. 이 연구는 엔진/API 경로 조사이며 UI 시각 검증이나 스크린리더 시험을 수행했다고 보고하지 않는다.

이 범위에서는 추가 전체 디컴파일이 필요하지 않았다. LG 문제는 새 정적 추출보다 통제된 장치·권한 실행 증거가 우선이다. native 호출 내부에서 다시 멈춘다면 해당 **우리 소유 PID의** 단일 stack sample과 같은 run의 좁은 권한/장치 이벤트를 묶는 것이 다음 분석이다.
