# 실시간 오디오 실행과 제어 프로토콜 설계

제품 범위와 진행 상태는 [제품 문서](../product.md)가 기준이다. 이 문서는 실시간 오디오 실행 경로와 UI↔실행부↔엔진 제어 프로토콜의 설계 결정을 기록한다. 측정 전에는 지연·성능 개선을 주장하지 않는다.

## 현재 구조와 확인된 사실

- **HAL 공유 링은 HAL이 소유하는 루프백 저장소다.** Driver 서비스가 익명 공유 메모리를 할당하고 HAL Driver(coreaudiod)에 전달한다. HAL은 재생을 캡처 링에 직접 써서 Driver 내부 루프백을 제공한다([hal_io.rs](../../native/macos/common/hal_io.rs)). 그래프 엔진은 일반 CoreAudio 클라이언트로 장치를 열며 HAL 링을 직접 처리하지 않는다. 앱 소스의 공유 메모리 전송은 이 루프백 저장소와 별도다.
- **그래프 렌더링은 대부분 CoreAudio IOProc 안에서 실행된다.** 엔진은 MixRelay 가상 장치도 일반 CoreAudio 클라이언트로 열며([spec.rs](../../apps/service/src/macos_devices/spec.rs)), 사용 가능한 장치 클록이 있으면 그 IOProc에서 동기적으로 렌더링한다([direct.rs](../../apps/service/src/macos_devices/direct.rs)). 이 경로는 이미 실시간 스케줄링을 받는다.
  입력 전용 IOProc은 자신의 출력 스트림을 비활성화한다. 직접 렌더링의 sample time과 host time은 master가 소유한 입력 또는 출력 방향에서 함께 읽는다.
- **실시간 승격과 알림 대기는 일부 경로에 적용됐고, 폴링·제어 작업 분리는 남아 있다.**
  - 장치 클록이 없을 때의 대체 워커는 250µs 잠들며 폴링한다([macos/mod.rs](../../apps/service/src/macos/mod.rs)).
  - 앱 캡처 pump는 tap 콜백 알림으로 깨어나고 변환 구간에 time-constraint 우선순위를 적용한다. 형식 갱신·자원 교체 같은 제어 작업은 여전히 같은 스레드에서 실시간 정책을 잠시 해제한 채 처리한다([clock_pump.rs](../../apps/desktop/src-tauri/src/macos_audio/clock_pump.rs)). 엔진 소비자가 측정 burst를 바탕으로 적응 목표를 소유하고 공유 클록 v3로 pump에 발행한다.
  - 플러그인 DSP 스레드는 macOS에서 time-constraint 우선순위를 적용하고, macOS 14.4 이상에서는 공유 futex로 대기한다. 100µs 폴링은 futex를 사용할 수 없거나 대기가 실패했을 때의 대체 경로다. 플러그인 `try_lock` 경합 시에는 50µs 잠든 뒤 재시도한다([processor-host](../../crates/processor-host/src/lib.rs)).
  - MixRelay가 만든 앱 캡처 pump와 플러그인 DSP 스레드는 실시간 스케줄링(time-constraint) 승격을 시도하며 실패 시 일반 우선순위로 동작한다. 대체 그래프 워커의 실시간 승격과 오디오 workgroup 참여는 아직 적용되지 않았다.
- **변경 요청은 요청/응답, 실행 상태는 통합 push다.** UI → 편집 앱(Tauri invoke) → 실행부(사용자 Unix 소켓, 줄 단위 JSON) → 엔진(`engine.sock`의 HTTP/JSON). 미터·전원/세션·그래프 revision·노드 제어·네이티브 에디터 상태는 아래 통합 스트림을 공유한다. push가 없거나 끊겼을 때만 UI가 해당 상태를 폴링하고 5초마다 push를 다시 시도한다. 장치·앱 발견과 OS 아이콘 목록의 폴링은 별도다.
- **로컬 제어 주소:** 엔진은 `engine.sock`, Driver는 `driver.sock`을 사용한다. 소켓 디렉터리는 절대 경로인 `MIXRELAY_IPC_DIR`이 우선이며, 기본값은 macOS `~/Library/Application Support/MixRelay/ipc`, Linux `$XDG_RUNTIME_DIR/mixrelay`다. HTTP API와 경로는 유지하고 Host는 각각 `mixrelay-engine`·`mixrelay-driver`를 보낸다. Driver는 다른 Host와 모든 Origin 헤더를 거부한다. 설치된 서비스는 재정의를 보지 못하므로 편집 앱·실행부는 재정의를 무시하고 세션 디렉터리를 쓴다(재정의는 격리된 시험·lab 실행용).

## 결정 1. 실시간 실행

- CoreAudio IOProc와 HAL `DoIOOperation`의 스케줄링·workgroup은 바꾸지 않는다. 콜백 안에서 다른 프로세스를 기다리지 않는다.
- MixRelay가 소유하는 PCM 스레드(대체 워커, 앱 캡처 pump, 플러그인 DSP)만 실시간으로 만든다. 감독·준비·XPC·회수·로그·디스크 쓰기 스레드는 실시간으로 올리지 않는다.
- 실시간 정책: 기본 주기 `P = 실제 네이티브 콜백 quantum / 실제 샘플레이트`(그래프 내부 분할 크기가 아님). 제약 `D = 0.8P`, 계산 예산 `C = max(측정 p99.9 × 1.25, D/2)`, 측정 전 임시값 `C = 0.4P`. 설정 후 실제 적용값을 다시 읽어 기록한다.
- 같은 장치 주기를 공유하는 스레드만 장치 workgroup(`kAudioDevicePropertyIOThreadOSWorkgroup`)에 참여한다. 그 밖에는 `AudioWorkIntervalCreate`로 만든 자체 interval workgroup을 쓰고 예상 release 시각 기준으로 interval을 시작·종료한다.
- 실시간 정책이나 workgroup 참여가 실패하면 성능 저하 상태로 표시하고, 낮은 지연 구성을 보장한다고 표시하지 않는다.

## 결정 2. 이벤트 기반 깨우기

- 깨우기는 "권한 있는 상태를 다시 확인하라"는 뜻이며 프레임 credit이 아니다. PCM·클록 발행과 분리한다.
- 같은 프로세스 안(탭 IOProc → 캡처 pump)은 로컬 세마포어를 쓴다.
- 프로세스 사이는 **XPC로 전달한 Mach 세마포어**를 기본으로 한다. 제품 최소 버전 macOS 15.0은 hosted runner의 검증 범위에 따른다. 14.4 이상의 공유 futex(`os_sync_wait_on_address` + `OS_SYNC_*_SHARED`)는 측정 후 협상 가능한 대체 구현으로 둔다. 비공개 `__ulock`은 쓰지 않는다.
- 생산자: 상태 발행 → `sequence` 증가 → `pending`이 0에서 1로 바뀔 때만 signal. 소비자: 토큰을 받으면 `pending`을 먼저 0으로 바꾼 뒤 상태를 확인한다. 대기는 다음 마감 또는 `2P` 중 이른 시각까지로 제한해 깨우기 유실이나 생산자 종료가 진행을 영구히 멈추지 못하게 한다. 연속 실패 시 알림 경로를 교체하고 그동안 `min(P/4, 250µs)` 폴링으로 복구한다.
- 알림 영역은 포인터 없이 magic·version·incarnation과 정렬된 `sequence`/`pending`/`state` 셀로 구성하며, 형식을 바꾸면 전송 버전을 올리고 살아 있는 매핑을 제자리에서 바꾸지 않는다.

## 결정 3. 지연 정책

- 고정 512프레임 읽기 지연과 pump의 고정 목표를 측정 기반 정책으로 바꾼다: 생산자·소비자·변환기 burst + 측정한 전달 지터 + drift 보정 여유.
- 구현 상태: 공유 클록 v3의 소비자 커서와 목표를 함께 발행하고 pump가 이를 따른다. 생산자·변환기 burst와 전달 공백, 연속 그래프 읽기의 누적 소비 burst로 초기 목표를 정하며 512는 생산자 측정 전 bootstrap에만 쓴다. 목표 상한은 capacity/2다.
- 정상 스트림의 starvation episode당 한 번 늘리고, 다음 읽기 커서를 유지한 채 목표만큼 재버퍼링한다. 실행 중 축소는 하지 않으며 시작·epoch 변경·불가피한 overwrite에서만 재정렬한다. 소스별 starvation 횟수·프레임, rebuffer 프레임, overwrite skip·현재 목표는 고정 저장소에 기록하고 pump의 비실시간 구간에서 보고한다. 설치본의 지연·청감 검증은 별도다.
- drift 보정은 독립 클록 경계에서만 한다. 대기 시간을 바꿔 drift를 보정하지 않는다.

## 결정 4. 제어 프로토콜

- **변경 요청은 요청/응답을 유지한다.** 보내기 전에 만든 작업 ID를 편집 앱·실행부·엔진에 그대로 전달하고, 엔진 incarnation이 다르면 부작용 전에 거부한다. 결과는 `not_admitted / accepted / completed / uncertain`으로 구분하며 자동으로 다시 보내지 않는다. 빠르게 끝난 결과도 실행부가 받을 때까지 보관한다.
- **상태는 push로 전달한다.** 미터·다이내믹스, 플러그인 에디터 상태, 노드 제어, 엔진 전원·세션, 그래프 revision을 변경 시점에 발행한다. 미터는 엔진의 기존 단일 발행자를 정기 발행으로 바꾸고 binding 목록과 packed `f32` 배열로 보낸다.
- 구간: 엔진→실행부는 실행부 전체가 공유하는 단방향 구독 하나(SSE. 상태는 엔진에서 실행부로만 흐르므로 WebSocket이 필요 없다), 실행부↔편집 앱은 편집 앱당 줄 단위 연결 하나(명령과 구독을 요청 ID로 다중화), 편집 앱→웹뷰는 Tauri Channel과 명시적 credit.
- 스트림에는 `engineIncarnation`, `sequence`, 전체 상태의 `generation`, 미터 binding의 그래프 `revision`을 붙이고 재연결 시 전체 snapshot부터 다시 받는다. UI는 포트별 구독 시도 세대로 늦은 응답을 거른다. 실행부 incarnation은 후속 범위다. 느린 구독자는 최신값만 유지하고, 미터는 구간 최대값으로 합친다.
- 엔진 HTTP 수신은 사용자 소유 Unix 소켓으로 옮겨 peer를 인증한다. 폴링은 대체 경로로 남긴다.

### 적용 상태

- **통합 상태 push·packed 미터 (2026-10-14, 항목 10·11 소스 적용).**
  - 엔진 SSE 경로는 `/api/v1/telemetry` 하나다. 공유 publisher가 50ms 창마다 `event: telemetry`를 발행하고 admission close 때 종료한다. 값이 같아도 미터 창을 보내므로 2초 무소식은 실패다. Unix 소켓·`X-MixRelay-Engine` fencing은 그대로다.
  - 이벤트는 `ControlTelemetry { engineIncarnation, sequence, meters, state? }`다. `state`는 `ControlStateSnapshot { generation, status, graph, controls, editors }`의 전체 교체 snapshot이다. 제거된 노드·에디터와 사용할 수 없는 controls(`null`)도 이 교체에 포함한다. generation은 상태 변경에만 증가하고 같은 엔진의 재연결에서도 유지된다. 첫 이벤트에는 항상 전체 상태가 있다.
  - native window close·worker 실패·블록 경계 채택은 HTTP 변경 요청 밖에서 일어나므로 엔진의 비실시간 worker가 관찰한다. 엔진은 batch당 mutation admission을 한 번 `try_lock`하고 바쁜 writer는 기다리지 않는다. 장치/Driver 정보는 기존 1초 관찰 주기와 revision/전원 변화 때 갱신하며, 공유 메모리의 에디터·실행 revision·노드/녹음·전원 상태는 50ms 창에서 읽는다. 서버는 변경된 상태만 스트림에 넣고 현재 전체 snapshot을 별도 보관한다.
  - `meters`는 `{ protocolVersion, revision, bindings, samples }`다. binding은 `{ nodeId, portId, channels, inputChannels }`이며 `inputChannels: null`이면 다이내믹스가 없다. binding 순서대로 output peaks, dynamics input peaks, gain reduction dB, threshold dB를 little-endian `f32`로 이어 붙인다. 매 창 binding 전체를 보내며 `revision`이 binding의 그래프 버전이다.
  - 엔진→실행부와 실행부→편집 앱 frame의 `samples`는 base64다. SSE의 텍스트 framing·기존 HTTP dev bridge·credit 경로를 그대로 쓰기 위한 선택이며 JSON float 배열은 보내지 않는다. 실행부는 엔진 구독 하나를 모든 편집 앱에 공유한다. 느린 mailbox는 미터를 구간 최대값으로 합치고 상태는 마지막 전체 snapshot을 보존한다. 뒤늦은 편집 앱 구독에는 보관한 전체 상태도 즉시 replay한다.
  - 웹뷰는 구독자당 Tauri Channel 하나다(`control_subscribe_telemetry`, `control_telemetry_credit`, `control_telemetry_visibility`, `control_unsubscribe_telemetry`). frame은 `InvokeResponseBody::Raw`: LE `u32` JSON 길이 + JSON 메타데이터 + 4바이트 정렬용 padding + packed `f32` 바이트다. 메타데이터의 `samples`는 빈 문자열이고 UI는 sample 영역을 `Float32Array`로 decoding한다(지원 macOS/Linux host는 little-endian). 종료는 같은 채널의 `{ ended: true }` JSON marker다.
  - 편집 앱은 문서의 visibility 변화를 해당 구독에 전달한다. 숨긴 웹뷰에는 1초에 한 번 최신 전체 상태와 구간 최대 미터를 전송해 2초 무소식 감시를 유지한다. 다시 보이면 대기를 깨우고 최신 창을 즉시 전송한다. 엔진의 50ms 발행·렌더링과 공유 reader는 계속 실행하며, 구독 종료 때 visibility 관찰자도 회수한다.
  - credit은 기존 2개·10초 미반환 종료를 유지한다. 미터 backpressure로 상태가 없어지지 않으며 종류별 상태는 하나의 coherent 전체 snapshot으로 함께 coalesce한다. 과거의 중간 전이는 기록 로그가 아니므로 최신 snapshot으로 대체된다.
  - UI의 포트별 hub가 상태와 미터 구독을 하나로 공유한다. 상태 소비자와 그래프 적용 대기는 push에서 갱신하고, push 실패·stale 때 캐시를 무효화한 뒤 polling fallback과 5초 retry를 쓴다. 엔진 교체 시 이전 상태/미터를 지우고 full snapshot으로 resync하며, revision이 이전 엔진과 같아도 그래프를 다시 읽는다. push 뒤 도착한 fallback 응답과 에디터 명령의 오래된 관찰은 시도 세대로 거른다. 로컬 편집 중 받은 노드 상태는 편집이 끝난 뒤 다시 소비한다. macOS 캡처 종료 확인과 메뉴 관찰은 실행부의 상태 수신 경로에도 유지한다.
  - `MIXRELAY_DEV_ENGINE_BRIDGE=1`의 same-origin `/engine/api/v1/telemetry`는 SSE를 그대로 전달한다. `HttpControlPort`도 같은 frame/schema를 decode하고 동일 hub·fallback을 쓴다.
  - binding 목록은 스트림마다 직전에 보낸 것과 다를 때만 보낸다(엔진 SSE·실행부 연결의 구독·웹뷰 채널 각각). 모든 구간은 순서를 지키고 잃지 않으며, 합치기는 encode 전에 일어나므로 decoder는 마지막으로 받은 binding을 쓴다. binding 없이 시작한 스트림이나 decode할 수 없는 이벤트는 스트림을 끝내고 재구독하게 한다.
  - 실행부↔편집 앱 연결(링크 프로토콜 3): 편집 앱은 실행부 연결 하나에 명령과 구독을 함께 싣는다. 요청은 `{id, command}`, 응답과 push는 같은 `id`를 단다. 실행부는 요청을 동시에(연결당 16개) 처리하고 응답을 하나의 writer로 보낸다. 명령 응답이 텔레메트리보다 먼저 나가고, 구독마다 쓰지 못한 창 하나만 유지해 느린 편집 앱이 응답을 막거나 메모리를 늘리지 않는다. 구독은 `{type: "cancel", subscription}`(응답 없음)이나 연결 종료로 끝난다. 편집 앱은 writer가 받지 않은 요청만 새 연결로 한 번 다시 보내고, 그 밖의 끊김은 재전송하지 않는다. 10초 넘게 비어 있던 연결은 재사용하지 않는다(실행부 유휴 종료 30초).
  - 읽기와 변경 요청은 결과를 기다리는 동안 1초마다 같은 요청 ID의 `status: 102`를 보낸다. 편집 앱의 기존 3초 무소식 감시는 유지하며, 실행부가 응답하는 대기를 연결 단절로 오판하지 않는다. 비동기 읽기는 연결 종료·종료 요청 때 취소하고, 변경은 수락한 작업을 끝까지 실행한다. Linux 장치·앱 검색은 별도 작업 스레드에서 기존 시간 제한이 있는 자식 프로세스를 실행한다. 수락한 검색은 연결 종료 뒤에도 완료를 기다려 자식 프로세스를 회수하며, 대기 중 명령 처리와 생존 응답을 계속한다. 엔진 HTTP 요청과 그래프 적용의 기존 상한은 유지한다. 실행부 연결 실패는 명령 이름과 고정 오류 분류만 개인 로그에 남기며 요청 본문·동적 오류 문자열은 기록하지 않는다.
  - 설치본·네이티브 부하·실제 브라우저 화면 검증은 별도다.
- **로컬 소켓 수신 (2026-10-14).** 엔진과 Driver는 사용자 소유 Unix 소켓만 연다([mixrelay-local-socket](../../crates/local-socket/src/lib.rs)).
  - 소켓 디렉터리는 `0700`, 소켓은 `0600`이다. 정규 경로의 모든 상위 디렉터리가 이 사용자나 root 소유이고 다른 사용자가 쓸 수 없어야 한다(root 소유 sticky 디렉터리 제외).
  - 시작하는 서비스는 소켓 옆 lock으로 직렬화한다. 살아 있는 소켓은 바꾸지 않고, 이 사용자 소유의 오래된 소켓만 한 서비스가 교체한다. 서비스는 자기가 만든 소켓만 지운다.
  - 연결은 요청을 읽기 전에 peer UID로 거른다. 엔진은 자기 사용자만, Driver는 자기 사용자와 root(설치기의 준비 확인)만 받는다. 엔진은 CORS 헤더를 보내지 않는다.
  - 브라우저 개발 화면은 `MIXRELAY_DEV_ENGINE_BRIDGE=1`일 때만 loopback `vite dev`가 같은 출처 `/engine`을 엔진 소켓으로 전달한다. Host·Origin을 확인하지만, 켜 두는 동안 dev 서버에 닿는 다른 계정도 개발자 권한으로 엔진을 조작할 수 있다.
  - 설치기는 서비스 계정 홈의 소켓으로 준비를 확인한다.

## 측정

- 경로별(IOProc 직접 렌더, 연결된 장치, 대체 워커, 앱 캡처, 플러그인) 콜백 지연·실행 시간·깨우기 지연·발행→소비 시간의 p50/p99/p99.9/max와 마감 여유를 `mach_absolute_time`으로 기록한다. 계측은 할당·잠금·로그 없이 미리 할당한 ring에 쓴다.
- underrun, 누락·중복 프레임, discontinuity, 깨우기 timeout·실패를 따로 센다.
- 같은 기기·형식에서 BlackHole(HAL 루프백)과 Loopback(캡처·믹스 구성)을 비교한다. 상관 신호로 종단 지연과 채널 순서를 확인한다.

## 하지 않는 일

감독 스레드의 실시간 승격, 콜백 안 XPC·속성 API 호출, HAL에서 엔진 완료 대기, 깨우기를 프레임 credit으로 취급, 네이티브 타임스탬프 대신 깨운 시각 사용, 512프레임 상수만 낮추기, 측정 없는 BlackHole 동등 지연 주장, 자동 변경 재전송, 노드별 스트림·무제한 큐·중복 미터 drain.

## 단계

1. 실행 경로·타이밍 계측
2. 실시간 스레드 지원과 앱 캡처 pump의 제어 작업 분리
3. 알림 프로토콜(세마포어/futex)과 모델 테스트, 실제 폴링 교체
4. 측정 기반 지연 정책
5. 제어 프로토콜: incarnation·작업 ID → 엔진 구독 → framed 실행부 스트림 → Tauri Channel → Unix 소켓 엔진 수신
6. 128채널(별도 설계)
