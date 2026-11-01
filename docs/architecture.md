# 코드 구조와 책임

Rust 워크스페이스의 크레이트 책임, 의존 방향, 새 코드를 둘 위치를 정한다. 제품 수준의 실행 책임은 [제품 문서](product.md)의 "실행 책임과 수명", UI 구조는 [디자인 시스템](design-system.md)과 `check:boundaries`가 기준이다. Rust 코드 작성 규칙은 [Rust 작업 규칙](rust-guidelines.md)이다.

## 계층과 크레이트

의존은 위 계층에서 아래 계층으로만 향한다. 같은 계층 안의 의존은 표에 적은 것만 허용한다. `pnpm check:crate-layers`가 `cargo metadata`로 일반·build·dev 의존 모두를 크레이트별 허용 목록과 비교하고, 표에 없는 워크스페이스 크레이트도 실패로 처리한다. dev 의존에는 테스트 지원 크레이트가 추가로 허용된다. 워크스페이스 크레이트의 `test-*` feature(테스트용 가짜·관찰 hook)도 dev 의존에서만 켤 수 있다.

| 계층 | 크레이트(경로) | 책임 | 허용 의존 |
| --- | --- | --- | --- |
| 기반 | `mixrelay-audio-io`(crates/audio-io) | 장치·스트림·형식·클럭 모델, FIFO·residence·클럭 브리지, 백엔드 trait | 없음 |
| 기반 | `mixrelay-pcm-ring`(native/pcm-ring) | 단일 생산자·단일 소비자 PCM 링 | 없음 |
| 기반 | `mixrelay-realtime`(crates/realtime) | 실시간 스레드 정책, 호스트 시각 | 없음 |
| 기반 | `mixrelay-time`(crates/time) | 단조 시각·deadline 값, 시각 읽기(`MonotonicClock`)·시간 대기(`Sleeper`) port와 표준 라이브러리 구현 `StdClock`. 판단 코드는 port를 주입받고 제품 경로의 진입점만 `StdClock`을 만든다 | 없음 |
| 기반 | `mixrelay-local-socket`(crates/local-socket) | 사용자 소유 Unix 소켓, peer 확인 | 없음 |
| 기반 | `mixrelay-licensing`(crates/licensing) | 라이선스 확인·저장 | 없음 |
| 기반 | `mixrelay-processor-state`(crates/processor-state) | 플러그인 상태 저장 형식 | 없음 |
| 기반 | `mixrelay-engine-process`(crates/engine-process) | 엔진 프로세스 수명·소유권 | `time` |
| 기반 | `mixrelay-midi-control`(crates/midi-control) | MIDI 바인딩·학습 입력 mailbox·pick-up·볼륨 발행 주기 정책 | 없음 |
| 모델 | `mixrelay-audio-graph` | 그래프 문서, 검증, 실행 계획 컴파일 | 기반 |
| 모델 | `mixrelay-audio-recorder` | 녹음 파일·작업자 | 기반 |
| 모델 | `mixrelay-audio-engine` | 블록 처리, 렌더 계획, 지연 보상·측정 | 기반, `audio-graph`, `audio-recorder` |
| 계약 | `mixrelay-control-protocol` | 편집 앱 ↔ 엔진 제어 메시지 | 기반, 모델 |
| 계약 | `mixrelay-driver-control` | 엔진·앱 ↔ Driver 서비스 제어, 지연 권한 | 기반 |
| 도메인 | `mixrelay-engine`(crates/engine) | 엔진 도메인 타입, 도메인·애플리케이션 서비스, 이 서비스가 쓰는 port trait | 기반, 모델, 계약(서비스 부품과는 서로 의존하지 않음) |
| 서비스 부품 | `mixrelay-control-server` | 제어 HTTP 서버, 작업 영수증 | 기반, 모델, 계약 |
| 서비스 부품 | `mixrelay-processor-host` | 플러그인 호스팅(VST3·CLAP, Apple의 AU·AUv3, Linux의 LV2)과 플러그인 편집기 표시 | 기반, 모델, `control-protocol`, vendor 플러그인 형식 |
| macOS 어댑터 | `mixrelay-macos-transport`(native/macos/common) | CoreAudio IO·FIFO 탭·인벤토리 XPC 인코딩 | 기반 |
| macOS 어댑터 | `mixrelay-hal-core`(native/macos/hal-driver/rust) | AudioServerPlugIn 정책: 장치·속성·전송·지연 판정 | 기반, `macos-transport` |
| macOS 어댑터 | `mixrelay-process-tap`(native/macos/process-tap) | 앱 오디오 캡처(process tap) | 기반, `macos-transport` |
| macOS 어댑터 | `mixrelay-macos-driver`(native/macos/driver-control) | Driver 서비스의 macOS 백엔드 | 기반, 계약, `macos-transport` |
| macOS 어댑터 | `mixrelay-macos-installer`(native/macos/installer) | 설치·업데이트·제거 도우미 | 기반, 계약 |
| macOS 어댑터 | `mixrelay-hal-bundle`(native/macos/hal-driver/bundle) | HAL 번들 빌드 도구 | 없음 |
| Linux 어댑터 | `mixrelay-pipewire`(native/linux/pipewire) | PipeWire 스트림·장치 | 기반 |
| Linux 어댑터 | `mixrelay-linux-driver`(native/linux/driver-control) | Driver 서비스의 Linux 백엔드 | 기반, 계약, `pipewire` |
| 앱 | `mixrelay-service`(apps/service) | 엔진 실행 파일: 도메인 서비스와 어댑터 조립, 실행 수명 | 앱 외 전부 |
| 앱 | `mixrelay-driver-service`(apps/driver-service) | Driver 서비스 실행 파일 | 앱 외 전부 |
| 앱 | `mixrelay-desktop`(apps/desktop/src-tauri) | 편집 앱과 백그라운드 실행부(Tauri) | 앱 외 전부 |
| 테스트 지원 | `mixrelay-test-support`, `mixrelay-macos-test-support`, `tests/fixtures/*`, `mixrelay-pipewire-observer`(native/linux/pipewire/tests/buffer-observer) | 테스트 도구·진단용 preload | dev-dependency로만 사용 |
| vendor | `truce-rack-core`·`truce-rack-clap`·`truce-rack-vst3`(vendor/) | 플러그인 형식 어댑터(외부 코드) | `truce-rack-core`(clap·vst3) |

알려진 예외: `mixrelay-processor-host`가 플러그인 설명자·진단·편집기 프레임 타입 때문에 `mixrelay-control-protocol`에 의존한다. `control-server`는 제어 서버라서 계약을 구현하는 책임상 계약에 의존하고, 그 밖의 서비스 부품이 계약에 의존하는 것은 이것만 허용한다. 같은 종류의 의존을 새로 만들지 않는다. `mixrelay-processor-host`는 플러그인 형식과 편집기 표시 때문에 플랫폼별 코드(AppKit, X11, AU·LV2)도 가지며, 이 코드는 `cfg` 모듈 안에 둔다.

이름이 비슷한 엔진 크레이트 세 개는 책임이 다르다.
- `mixrelay-engine`: 엔진의 도메인 타입과 정책, 그 정책이 쓰는 port
- `mixrelay-audio-engine`: 준비한 그래프의 블록 처리와 렌더링
- `mixrelay-engine-process`: 엔진 프로세스의 수명과 소유권

## 의존 규칙
- 모델·계약·도메인·서비스 부품은 플랫폼 중립이다. OS 어댑터와 앱에 의존하지 않고 OS API를 직접 호출하지 않는다. OS 기능이 필요하면 trait을 정의하고 어댑터나 앱이 구현한다.
- 도메인 크레이트(`mixrelay-engine`)와 서비스 부품 크레이트는 같은 높이의 계층이다. 서로 의존하지 않고, 앱이 둘을 함께 조립한다.
- port(trait)는 그 port를 쓰는 안쪽 크레이트가 정의한다. 어댑터는 안쪽 크레이트의 port를 구현하며, 안쪽 크레이트는 구현하는 크레이트를 모른다.
- 엔진 어댑터는 `native/<os>/`에 두고 `mixrelay-engine`의 port를 구현한다. 기반·모델·계약, `mixrelay-engine`, 같은 OS의 어댑터에 의존할 수 있다. 행의 허용 의존은 `scripts/check-crate-layers.mjs`의 `engineAdapter`로 정한다. 엔진 어댑터가 아닌 어댑터는 `mixrelay-engine`에 의존하지 않는다.
- 앱은 조립만 한다. 도메인 서비스와 어댑터를 연결하고 실행 수명을 관리하며, 판단과 정책을 두지 않는다.
- 기반 크레이트 중 `realtime`(스레드 정책·호스트 시각), `local-socket`(Unix 소켓·peer 확인), `licensing`(네이티브 credential 저장)은 그 책임상 OS를 직접 호출한다. 이 호출은 플랫폼별 `cfg` 모듈 안에 두고, 그 위의 판단은 플랫폼 중립 API로 노출한다.
- OS 어댑터는 다른 OS의 어댑터에 의존하지 않는다. 앱은 다른 앱에 의존하지 않는다.
- 테스트 지원 크레이트는 `[dev-dependencies]`로만 쓴다. 제품 코드가 테스트 도구에 의존하지 않는다.

## 책임 배치
- 판단과 정책은 플랫폼 중립 크레이트나 어댑터의 안전한 Rust 부분에 둔다. C 브리지(`MixRelayHALBridge.c`, `*.h`)와 `extern "C"` 함수는 ABI 변환만 한다(M-FFI-TRANSLATES).
- 앱 크레이트는 부품을 조립하고 OS·프로세스 수명을 연결한다. 여러 앱이 쓰거나 테스트할 가치가 있는 로직은 계약·서비스 부품 크레이트로, 엔진의 판단·정책은 `mixrelay-engine`으로 옮긴다.
- 새 코드를 둘 곳:
  - 엔진의 판단·정책(도메인·애플리케이션 서비스)과 그 port → `mixrelay-engine`. `apps/service`에 두지 않는다
  - 시각 읽기·시간 대기 → `mixrelay-time`의 `MonotonicClock`·`Sleeper`를 주입받는다
  - 장치·OS에 무관한 오디오 처리 → `audio-engine`, `audio-graph`, `audio-io`
  - 프로세스 사이 메시지 형식 → `control-protocol`(편집 앱 ↔ 엔진), `driver-control`(↔ Driver 서비스)
  - CoreAudio·PipeWire 호출 → 해당 OS 어댑터. 엔진 쪽 OS 연결은 `mixrelay-engine` port를 구현하는 엔진 어댑터에 둔다. 아직 옮기지 않은 연결은 `apps/service`의 `macos_devices`·`linux_*`에 있다
  - HAL 동작 → `hal-core`(Rust), ABI 연결만 C 브리지
- 테스트 배치는 [Rust 작업 규칙](rust-guidelines.md)의 테스트 절을 따른다. 테스트용 가짜는 `#[cfg(test)]` 뒤에 두고 제품 코드의 가시성을 넓히지 않는다. 다른 크레이트의 테스트가 쓰는 가짜는 그 port를 정의한 크레이트의 `test-util` feature 뒤에 두고 dev 의존에서 켠다(예: `mixrelay-time`의 `ManualClock`).

## 네이티브 실행과 제어 채널

엔진의 네이티브 실행 계약은 다음 책임으로 나눈다. 하나의 네이티브 소유자가 세 계약을 구현하고
`RuntimeBackend`로 조립한다. 포맷 변경은 같은 장치 수명의 상태·정지·협상을 사용해야 한다.

| 계약 | 책임 |
| --- | --- |
| `RuntimeStatusBackend` | 실행 상태·권한 대기·제안된 포맷의 기능 조회 |
| `ProcessingControlBackend` | 오디오 경계에서 정지·재개와 실패한 rollback의 정지 유지 |
| `SessionFormatBackend` | 외부 클록 관찰·따라가기와 네이티브 포맷 협상 |

편집기와 런타임은 `control_channel`의 프레임 상한·프로토콜 버전·텔레메트리 mailbox 계약을 공유한다.
클라이언트의 `background::connection`은 요청 전달과 구독을, `background::process`는 자식 프로세스 수명을,
`background::directories`는 OS별 경로 결정을 맡는다. 서버 내부 구현을 클라이언트의 계약으로 사용하지 않는다.

`DriverClient`의 동기 전송은 첫 동기 조회에서 준비한 HTTP runtime·연결 pool을 같은 client와
clone 사이에서 공유한다. 준비 잠금은 소켓 I/O 전에 해제하며 실패한 준비는 다음 호출에서
재시도한다. macOS endpoint backend와 format monitor는 같은 client 소유자를 사용한다.
상태 응답은 매번 Driver에서 읽으며 권한 claim은 요청별로 적용한다. 이 전송은 비실시간
제어 작업이며 오디오 콜백에서 호출하지 않는다.

제어 전송 실패는 `ControlFailureContext`에 OS·경계·단계·실패 코드·전달 확실성·OS 오류 번호를 남긴다.
`editor_runtime`은 편집기와 사용자 런타임 사이의 소켓, `runtime_engine`은 런타임과 엔진 사이의 HTTP다.
연결 거부, 응답 본문 실패, 영수증 관찰 실패를 구분하며 전달이 불확실한 변경 명령은 재전송하지 않는다.
이 기록에는 요청 인자·경로·장치 이름을 넣지 않는다.

엔진의 `ExecutionChoice`는 재시작 후에도 유지하는 실행 선택·중단 이유·저장 오류를 소유한다.
그래프 저장이 성공해도 실행 선택의 저장 오류를 지우지 않으며, 재저장은 전원 변경과 직렬화한다.

`CatalogStorage`는 저장된 그래프 카탈로그·저장 경로·상태 blob 검증 캐시·쓰기 오류를 소유한다.
조회는 불변 값으로 한정하고 외부에 잠금이나 가변 저장 상태를 전달하지 않는다.
`CatalogRecall`은 네이티브 초기화 실패와 재시도 시각을 소유한다. 컨트롤러는 요금제·revision을
검사하고 네이티브 준비와 게시 순서를 직렬화하며, 저장과 복원 상태의 변경은 각 소유자에게 요청한다.

엔진의 `RecorderSessions`는 녹음 세션과 일시정지 중인 탭의 수명을 소유한다.
녹음 일시정지·재개·회수·종료와 정리 오류는 이 객체에서 처리한다.
`EngineGraphController`는 그래프의 노드·revision·요금제 조건을 확인하고 녹음 동작을 요청한다.
전원 제어와 텔레메트리는 녹음 상태를 직접 변경하지 않는다.

편집기의 `GraphReadback`은 읽기 전용 `ControlPort`로 그래프 선택과 적용 revision을 확인한다.
`GraphApplicationQueue`는 확인한 문서·사용자 초안·변경 요청의 순서를 소유한다.
조회 결과는 큐가 선택·취소 상태를 다시 확인한 뒤 화면에 전달한다.
`TelemetryHub`는 스트림이 끊기면 캐시를 무효화하고 마지막 엔진 incarnation을 전달한다.
revision 확인은 처음 받은 push 이후 읽기 응답으로 되돌아가지 않으며, 원래 제한 시간 안에서
같은 엔진의 새 상태를 기다린다. 다른 incarnation은 `engine_replaced`로 종료한다.

HAL의 `ClientMix`는 등록된 클라이언트별 PCM 큐와 등록·제거·교체 수명을 소유한다.
`MixOutput`은 해당 클라이언트의 canonical Float32 출력을 큐에 게시하고, `ReadInput`은 같은
장치 sample 좌표에서 게시된 큐를 한 번씩 합산한다. 큐의 게시 범위를 벗어난 프레임은 0을 더하며
다른 클라이언트의 공급을 기다리지 않는다. 입력의 네이티브 정수 변환과 출력 볼륨·mute는
`Device::process_io`가 맡는다. `ClientChange`와 `Transfer`는 등록 변경과 PCM 전송을 구분한다.
큐는 IO를 시작하는 클라이언트만 가진다. 장치가 이미 실행 중일 때 등록된 클라이언트는 자기 StartIO
전에도 렌더할 수 있으므로 등록 때 받는다. 큐가 없거나 ring epoch를 다 쓴 클라이언트는 StartIO 때
제어 스레드에서 freeze 밖에 새 큐를 매핑하고 미리 fault한 뒤, freeze 안에서는 게시만 한다.
`PerformDeviceConfigurationChange`의 freeze 구간은 큐를 매핑하지도, 메모리를 할당하지도 않는다.
샘플레이트만 바뀌면 큐와 grant를 유지하고 ring epoch·clock을 다시 게시해 이전 rate의 PCM을 버리며,
epoch가 남지 않은 큐는 떼어 낸다. 형식이 바뀌면 새 endpoint 매핑(인벤토리를 받을 때 미리 매핑)으로
바꾸고 이전 모양의 큐를 떼어 낸다. 떼어 낸 큐와 매핑은 freeze가 끝난 뒤 해제한다. coreaudiod는 이
호출 앞뒤로 IO를 일시정지할 뿐 StopIO·StartIO를 부르지 않는다. 그래서 같은 호출이 반환하기 전에, IO가
아직 멈춘 동안 IO를 가진 클라이언트의 새 큐를 freeze 밖에서 매핑하고 짧은 freeze에서 게시한다. IO를
가지지 않은 클라이언트는 다음 StartIO에서 받는다. 큐가 필요 없으면 freeze도 잡지 않는다. 큐 메모리
예산은 등록 때뿐 아니라 매핑·게시할 때마다 현재 형식 기준으로 다시 검사한다. 매핑하지 못하거나 예산을
넘는 클라이언트는 설정을 되돌리지 않고 수를 기록한다. 제어 tick이 다시 시도하고, Driver service에 그 수와 회복을 보고해 로그에 남긴다. endpoint
ring을 다시 열 수 없으면 rate 변경과 시작은 아무것도 바꾸지 않고 거부된다.
`Registry`는 장치의 시작 참조를 세고 최초 시작·최종 종료를 네이티브 런타임에 요청한다.
최초 시작은 모든 큐의 PCM 타임라인을 초기화한다. 다른 클라이언트의 시작·정지는 게시된 PCM을
지우지 않으며, 등록된 출력 클라이언트가 직접 StartIO를 받아야 한다는 조건을 두지 않는다.
HAL의 `Clock`은 host 시각에서 nominal timestamp를 계산하고 period·알고리즘·안정성 메타데이터를
정의한다. host에는 `raw` 알고리즘을 명시하며, 메타데이터 조회는 고정 슬롯 pin만 사용해
제어 잠금·할당 없이 콜백에서도 같은 값을 반환한다.
다중 큐 합산은 고정 크기 복사 scratch와 프레임별 coverage를 사용한다. 공통 전송 계층이
frame stamp·epoch를 검사하고 구간마다 읽기 통계를 갱신한다. 유효한 디지털 무음과 누락을
구분하며, 콜백에서 큐를 할당하거나 다른 클라이언트를 기다리지 않는다.

HAL의 클라이언트 PCM은 `client_pcm`의 공유 VM에 저장한다. `ClientMix`가 endpoint 전체의
clock publication과 membership revision을 소유하고, 개별 writer가 PCM·유효 범위·retirement를
게시한다. 독자는 incarnation·전체 native epoch·anchor·rate·membership이 일치한 창만 사용한다.
등록 변경 중인 snapshot이나 끝나지 않은 clock publication에서는 cursor를 진행하지 않는다.
`control::native::pcm`은 엔진 lease가 살아 있을 때만 별도 XPC peer로 완전한 grant 목록을 보낸다.
Driver 서비스는 장치 인벤토리의 소유자로 유지한다. coreaudiod의 engine Mach-service 접근은
HAL 번들에 선언하며 엔진은 audio-server UID의 grant 게시와 console 사용자 source 요청을 구분한다.

Driver 서비스의 구성 거래는 단계마다 6초 안에 HAL의 적용을 기다린다. HAL은 인벤토리 세대의
적용 또는 거부(검증·native 적용 실패 포함)를, 그리고 모든 rate·format·매핑 요청의 결과를 요청한
rate·format, HAL incarnation과 함께 순서대로 보고한다. 순번은 보관 큐의 잠금 안에서 매기므로 여러
장치가 동시에 끝나도 순서가 뒤바뀌지 않는다. 새 연결은 보관한 결과를 다시 보낸 뒤 완료 표시를 보낸다.
서비스는 완료 표시를 받은 가장 새 연결에서 기준을 정하고, 같은 incarnation의 연결에서 그 기준보다
나중에 같은 값을 요청한 거부만 그 요청의 거부로 보고 곧바로 이전 상태를 새 예산으로 되돌린다. 새
incarnation이 완료 표시를 보내면 이전 incarnation의 연결은 버린다. 두 쪽은 `macos-transport`의 한
보고 codec을 쓰고, 보고 버전이 다르면 부분 설치로 보고
즉시 재설치가 필요하다는 오류를 낸다. HAL이 요청한 매핑 교체는 매핑만 바꾸고 rate·format을 요청하지
않는다.
6초를 넘긴 단계는 최대 24초 더 기다린다. HAL은 수행 중인 단계가 끝나기 전에는 새 인벤토리를
처리하지 않기 때문이다. 그 안에 적용되면 경고만 남기고 거래를 계속한다. 그때까지도 수행 중이면
되돌림을 게시만 하고 `ConfigurationPending`으로 보고하며, 관리자는 이전 인벤토리와 revision을
유지한다. 다음 조회는 HAL readback으로 상태를 다시 관찰하고, rate·format 요청은 캐시한 의도가
아니라 readback과 다를 때만 보낸다.

엔진의 `endpoint_service::virtual_pcm`은 가상 입력 source registry와 endpoint별 pump 수명을
소유한다. `ClientPcmMixer`는 native sample 좌표의 큐 합산과 forward cursor만 관리하고,
pump의 `Backlog`는 비운 클라이언트 PCM의 native 보존을, `Converter`는 기존 SRC·포맷 승인·보류
출력을, `Scheduling`은 worker 정책과 제어 작업의
실시간 밖 실행을 맡는다. `cadence`는 coherent native clock을 완료된 128프레임 주기의
좌표로 투영한다. 이 주기는 worker의 실시간 정책과 같은 상수를 사용한다. 늦게 깨어나도
mixer의 forward cursor로 보존된 PCM을 순서대로 배출한다. 외부 HAL 독자의 부분 읽기는
이 cursor를 변경하지 않는다.
시간 credit은 게시된 PCM과 native playhead 범위로 제한한다. 타임스탬프는 worker가 깨어난
시각이 아니라 native anchor와 프레임 좌표에서 구한다. 같은 클록의 master만 SRC를 우회한다.
그 밖의 source는 독립 클록 bridge가 drift를 보정한다. 순서대로 읽는 소비자는 자기 클록으로 큐를 비우므로
큐 점유와 목표의 차이로 보정한다. 시각으로 읽는 소비자는 변환 비율과 관계없이 큐 점유를 일정하게 유지하므로,
현재 구간에서 요청 시각이 진행한 프레임 주기와 다음 미읽음 프레임의 위상(한 프레임 이내)을 소비자 위치와 함께
게시하고, 변환기는 그 주기로 비율을 직접 정하고 위상 항(프레임당 120ppm)으로 맞춘다. 건너뛴 프레임이나 반
프레임을 넘는 위상은 소비자 클록이 바뀌었다는 뜻이므로 구간을 마지막 걸음부터 다시 잰다. 커서 점프·재획득·새
epoch도 구간을 다시 시작하되, 그때까지 잰 주기는 계속 게시한다. 공칭 주기에서 변환기 보정 한계(±2,000ppm)를
벗어난 걸음은 프레임보다 작아도 커서 점프로 보고 주기에 넣지 않는다. 소비자의 목표 점유는 이번 읽기가 프레임을
소비한 뒤의 점유다. 시각으로 읽던 소비자의 timing을 게시 도중이라 읽지 못한 주기에는 마지막 보정을 유지하고 큐
점유로 되돌아가지 않는다.

`cadence::next`는 native anchor에서 다음 절대 128프레임 경계를 계산한다.
`Scheduling`은 그 deadline까지 기다리며 제어·종료 확인을 위해 대기를 5ms 이내로 제한한다.
누적 credit은 `Pace::Block.available_frames` → `MacTimeline` → `MacEngineExternalIo`로 전달한다.
shared source가 준 credit은 `Pace::Block.publication`으로 그 게시 snapshot을 함께 전달한다.
획득 frontier는 이번 읽기 quantum과 아직 처리하지 않은 전체 credit을 구분하고, 그 source의
frontier는 snapshot의 게시 끝에서 계산한다.
보존 용량 안의 가장 오래된 PCM부터 읽고, 같은 epoch에서 이미 읽은 시각으로 되돌아가지 않는다.

`NativePacers`는 그래프가 기다리는 shared source의 신원을 생산자 실행 전에 선택한다.
`ApplicationPacer`는 이미 기다리던 source의 첫 credit과 같은 source의 새 producer epoch를
0부터 보존 범위 안에서 소비한다. 첫 관찰까지 생산된 PCM을 준비 이전의 이력으로 버리지 않는다.
이미 실행 중인 source에 처음 연결할 때와 물리 장치의 `HardwarePacer`는 현재 credit을
시작 기준으로 사용한다. 다른 native pacing 소유자로 전환하면 대기 중인 application 상태를 회수한다.

앱 펌프의 `write_clocked`는 공유 PCM·획득 시각을 먼저 쓰고 같은 생산자 claim에서
native 단위 credit을 공개한다. 캡처 누적량·drop 통계는 입력 준비량으로 사용하지 않는다.
정지 때는 마지막 게시 위치를 보존하고, 새 producer epoch에서만 0부터 시작한다.
`SourceRegistry::pcm_progress`는 같은 source pin에서 게시된 그래프 프레임·보존량·클록 메타데이터를
분리해 전달한다. `ApplicationPacer`는 게시 프레임을 직접 사용하므로 SRC 출력의 끝을
native 단위로 재환산해 버리지 않는다. 물리 장치는 기존 하드웨어 credit으로 진행한다.

`audio-io::retention`은 최대 지원 샘플레이트에서도 최소 4초의 backlog 저장량을 정한다.
macOS 공유·native PCM과 앱 캡처, Linux 입력 링은 이를 2의 거듭제곱인 1,048,576프레임으로 올림한다.
가상 입력의 보존은 엔진 pump가 맡는다. pump는 매 주기 클라이언트 큐를 native playhead까지 비워 backlog에 넣는다.
얼마나 남길지는 그래프가 게시한 route(`ApplicationClock::consumer_route`)로 정한다. 그래프는 주기마다 활성 plan의
가상 source마다 route를 게시한다. 노드가 켜져 있고 출력까지 이어지면 routed다. head는 pacer가 기다렸다가 첫
credit부터 소비하는 source(`NativePacers::head`)이며, 장치 클록이 graph를 이끄는 동안에는 없다. pacer는 켜져 있고
출력까지 이어진 shared source만 master로 고르거나 기다린다. 실행 중인 master는 계속 pacing할 수 있는 동안
유지하고, 멈추거나 자격을 잃었을 때만 plan 순서대로 다음 실행 중인 source를 고른다. worker는 다음 plan에서 빠진 source와,
끝날 때 모든 source를 unrouted로 게시한다. 이 route는 checkpoint나 렌더링 대기 중에도 바뀌지 않는다. route가
아직 게시되지 않았으면 클라이언트 큐 하나(`QUEUE_FRAMES`)만큼만 변환 없이 남기며, 그 밖은 손실로 세지 않는다.
head로 시작한 stream도 매 변환 전에 route를 다시 읽어, head를 잃었으면 남은 backlog를 버리고 live edge에서 이어 간다.
unrouted source는 meter와 pacing을 위해 계속 게시하되 한 주기에 변환할 만큼만 남겨 live edge에 머문다. 이때
버린 프레임과 큐에서 덮어쓰인 프레임은 손실로 세지 않는다. routed source는 포맷 승인·변환기 준비와 관계없이
endpoint별 backlog에 native rate와 native 좌표 그대로 보존한다. backlog는 `retention::RING_FRAMES` ×
endpoint 캡처 채널 수의 Float32(채널당 4MiB, 64채널에서 256MiB)다. HAL이 게시한 장치 geometry를
엔진이 받으면 endpoint 잠금 밖의 제어 경로에서 source 매핑과 backlog를 확보하고 미리 fault한다.
클라이언트가 없는 장치도 geometry 목록에 포함된다.
geometry 준비는 전용 제어 worker 한 개가 처리하며 대기 중인 전체 snapshot은 최신 한 개로
합친다. 이미 준비된 endpoint의 큐는 이 worker를 기다리지 않고 바로 전달한다. 준비 중·대기
중인 목록은 geometry와 아직 pump가 없는 큐의 VM 매핑만 유지하며 PCM을 복사하지 않는다.
이미 pump에 전달한 큐 매핑은 준비 목록에 보관하지 않는다. 준비 중인 snapshot보다
새로운 큐 membership을 이전 준비 결과가 덮지 못하게 한다. geometry 시각과 큐 membership
시각은 따로 비교하므로, 같은 geometry의 grant는 더 늦게 끝난 geometry 준비 뒤에도 사용할 수 있다.
예약 실패는 새 HAL snapshot 없이 10ms부터 최대 1초 간격으로 다시 시도한다. HAL의
StartIO·실행 중 클라이언트 추가·geometry 변경 후 큐 재준비는 엔진의 응답이나 준비 상태를
기다리지 않는다. 엔진으로 보내는 geometry·grant snapshot은 비동기 단방향 메시지다.
엔진이 없거나 응답하지 않거나 저장소 준비에 실패해도 HAL은 자체 큐와 native IO를 시작한다.
이미 준비된 pump는 grant snapshot에서 큐만 넘겨받으므로 source·backlog의 할당이나 첫
page touch를 기다리지 않는다. publication을 받지 못했거나 엔진 재시작·준비 실패로 pump가
없는 endpoint의 첫 grant는 같은 제어 worker에서 저장소를 준비한 뒤 전달한다. 이 fallback의
준비가 HAL 큐 보존 시간을 넘기면 덮어쓴 프레임을 기존 손실 보고에 포함한다.
클라이언트가 모두 사라지면 pump는 native 정지와 남은 backlog 손실을 게시하고 저장소를 유지한다.
같은 geometry의 다음 세션은 이 저장소를 재사용한다. 재사용 전에 producer lease와 terminal
상태를 확인하고, epoch를 소진한 저장소는 다시 준비한다. 장치 삭제·geometry 변경·clear는 worker를
멈추고 join한 뒤 저장소를 회수한다. 준비 중 clear가 오면 준비 결과를 게시하지 않는다.
준비 실패는 부분 저장소를 회수하고 제어 worker에서 재시도한다. HAL의 geometry 관찰 host
tick으로 오래된 snapshot이 새 geometry를 덮거나 새 endpoint를 회수하지 못하게 한다.
endpoint 하나의 큰 저장소는 backlog와 source PCM 각각 채널당 4MiB, source frame tag·시각
sidecar 24MiB다. 합계는 캡처 채널 수 C에 대해 (8C + 24)MiB이며, stereo는 40MiB,
64채널은 536MiB다. 여기에 고정 scratch·run metadata, 변환기 저장소와 worker 한 개의 stack이
더해진다. 큐가 없으면 worker는 비실시간 상태에서 park한다. grant·종료 신호 또는 남은 손실
보고의 quiet 기한이 worker를 깨운다. 준비된
endpoint는 최대 64개라 큰 저장소의 합은 최대 33.5GiB다. 전용 준비 worker는 한 개다.
준비 경로는 endpoint마다 HAL 큐를 최대 두 세대 더 유지할 수 있으며, 각 세대는 기존
`MAX_PCM_BYTES_PER_ENDPOINT`(1GiB) 한도를 따른다. 같은 큐의 VM 매핑은 물리 PCM을 공유한다.
이전 source 매핑은 graph pin이 남아 있는 동안 기존 registry의 최대 128개 owner 한도 안에서 유지되며, pin 해제 후 제어 경로에서
회수한다. 저장소와 큐 세대 수는 세션 수에 따라 늘어나지 않는다.
같은 timeline의 공백과 timeline 변경은 앞선 run을 유지한다. 가득 차면 가장 오래된 프레임을
버린다. 새 변환 stream은 원래 좌표의 시각으로 순서대로 변환한다. 보존한 head 전체는 head
source에만 전달한다. 다른 source에는 첫 게시 전에 가장 새 run의 가장 새 한 주기만 남겨, 합류한 source가 공유
획득 좌표나 다른 입력을 보존된 head로 되돌리지 않게 한다. head stream의 run 경계에서는 그 주기의 변환을 멈춘다.
소비자가 현재 epoch를 다 읽거나 읽기를 멈추거나 200ms가 지난 뒤에만 새 epoch를 시작한다. 한 주기의 변환은 최대
8주기다. sinc 변환은 48kHz 기준 4,096 채널·프레임을 native 주기에 비례해 줄인 예산 안에서 하되 최소 2주기는
변환한다. 늦게 승인된 head source는 실시간의 8배 속도로 따라잡고, graph credit은 게시한 PCM만큼만 순서대로
늘어난다. 변환기 bridge의 입력 FIFO는 한 주기분의 따라잡기·priming·sinc 이력을 담는 2,048프레임이다. 변환기는
포맷 승인 뒤에 저장 공간을 준비하고, 같은 형식의 run 경계에서는 저장 공간을 그대로 다시 쓴다. 저장 공간을
준비한 뒤에만 포맷을 승인하고 새 epoch를 시작하므로, 준비 실패는 source의 epoch를 바꾸지 않는다. 할당 실패는
보존한 PCM을 유지하고 10ms부터 두 배씩 늘려 최대 1초 간격으로 다시 시도한다. 잘못된 변환 설정처럼 다시 시도해도
같은 오류는 endpoint를 끝낸다. native clock이
멈추면 포맷·backlog 상태와 관계없이 먼저 native 정지를 게시하고, 그 timeline의 남은 backlog와 변환 stream을
끝낸다. 끝난 stream의 변환 출력은 다시 쓰지 않는다. routed source의 큐에서 덮어쓰인 프레임
(`ClientPcmMixer::skipped_frames`), backlog에서 버린 프레임, 정지 때 routed였던 source가 게시하지 못한 프레임은 손실이 1초
동안 멈춘 뒤 손실 구간마다 한 번 제어 경로 경고로 남긴다. grant 수신, source 저장소 준비 시작·완료, backlog 준비 완료, worker의 첫 drain,
source 승인(변환기 준비·첫 게시)의 시각은 native sample 0 기준으로 기록한다. IO 시작 전에
완료된 준비 시각은 음수다. 로그는 제어 경로에서만 쓴다. 물리 입력의 `HardwarePacer`는 첫 그래프 승인 때 준비 중 PCM을
버린다. 앱이 가상 장치에 게시한 PCM은 위의 `ApplicationPacer` 계약대로 head source의 첫 프레임부터 보존한다.
HAL 클라이언트 렌더 큐와 endpoint 매핑 링은 `client_pcm::QUEUE_FRAMES`
(65,536프레임)로 준비한다. 쓰는 쪽은 최대 렌더 span(`MAX_RENDER_SPAN_FRAMES`, 장치의 zero timestamp
period인 16,384프레임)만큼 앞서고 `ReadInput`은 그만큼 뒤처질 수 있으며, 큐는 그 합의 두 배다. 새 익명 매핑은 커널의 0 채움을 사용하므로 ring을 지우지 않고, 준비 단계에서 모든 페이지를
미리 fault해 HAL IO 스레드가 page fault를 겪지 않게 한다.
녹음 전달 큐는 256프레임 × 3,000개이며 192kHz에서 4초다. 모두 제어 경로에서 준비하고,
실시간 중에는 확장하지 않는다. 이 저장 용량은 재생 priming·지연 목표와 별개다.
지연 목표는 관찰한 callback burst와 클록 관계에서 정한다. `QueueBounds.maximum_target_frames`는
저장량과 별도로 지정하며 macOS bridge는 `buffer_policy::MAXIMUM_TARGET_FRAMES`(8,192프레임)를
상한으로 쓴다. Linux 출력 링은
최대 RAW 전송 32,768프레임을 수용하도록 65,536프레임으로 준비한다. 가득 찬 private FIFO는 실패한 쓰기를
거부하며 기존 PCM을 보존한다. 시각 공유 링은 오래된 구간을 덮어쓰므로 독자가 누락·불연속을
확인한다. 녹음 큐가 한계를 넘으면 받아 둔 prefix를 마무리하고 `Failed`로 보고한다.

HAL의 호스트 I/O 요청 크기와 설정 가능한 버퍼 크기는 별개다. 한 요청은 준비한
1,048,576프레임 보존량으로 제한한다. 렌더 요청은 클라이언트 큐가 감당하도록 정한 span을 넘으면 독자가 읽기 전에 덮일 수 있으므로
`MAX_RENDER_SPAN_FRAMES`로 제한하고 같은 extent 거부로 기록한다. 2,048프레임을 넘는 요청은 같은 I/O pin 안에서
기존 scratch 크기로 나눠 처리한다. Float32·정수 변환·복수 클라이언트 믹스 모두 같은 경로다.
정상 quantum은 추가 scratch 조회 없이 기존 처리로 보낸다. HAL trace v5는 null 버퍼와
보존량 초과를 분리하고 마지막 거부의 크기·포인터 유무·방향을 기록한다.

공유 source의 `SourceClockRead`는 준비된 snapshot·소비자 갱신 중의 보류·생산자 grant 만료를
구분한다. 보류는 cursor와 생산자를 유지한다. `begin_epoch`는 소비자 snapshot 재조회 없이
게시한 생산자 epoch를 반환한다. clock 관찰·epoch 승인·native 메타데이터는
`application_source_transport::clock`이, PCM·획득 시각·fault 발행은 전송 소유자가 담당한다.

가상 입력의 `write_clocked_pcm`은 같은 producer claim 아래 PCM을 먼저 쓰고, 성공한 출력
프레임에 대응하는 native 단위 credit만 게시한다. SRC에 남은 입력은 credit에 포함하지 않는다.
변환기는 준비된 출력을 유한 횟수로 배출하고 미완료 쓰기를 보존한다. 새 native epoch의
초기 credit 0과 native rate는 ring epoch보다 먼저 게시하며, 재무장 실패 시 이전 진단값으로
되돌리고 승인은 해제한다. native 중지는 마지막 credit을 보존한다. master 선택은 그래프의
소스 신원을 따르므로 포맷 승인 중의 일시적인 비실행 상태로 선택을 해제하지 않는다.

`macos::external_io`는 native·application·virtual source 선택과 각 provider의 epoch·읽기를
담당한다. `VirtualEndpointTransport` 입력은 이 shared source를 사용하고 native 입력 IOProc을
만들지 않는다. 물리 입력·명시적 `AudioDeviceInput`과 출력은 기존 native 어댑터를 사용한다.
정지한 virtual source의 남은 PCM은 읽지 않으며, 연결 전 포맷 요청은 허용한다. 장치·client의
제거는 grant 무효화와 worker 종료·join을 거친 뒤 VM·변환 버퍼를 회수한다.

`CaptureFrontier`는 최소 안전 좌표를 제공한 입력의 신원·epoch와 획득 시각을 묶는다.
`CaptureAlignment`는 graph frame credit에 따라 연속 획득 좌표를 유지한다. 같은 입력의
스케줄링 burst·reserve 증가로 frontier가 과거로 움직여도 이미 읽는 좌표를 되돌리지 않는다.
다른 입력이 현재 좌표보다 오래된 frontier로 합류하거나 clock·rate·graph 연속성이 바뀌면
공통 좌표를 다시 획득한다. 개별 source reader의 cursor는 앞으로만 진행한다.
`SourceRegistry::read_audio_timed`는 시각 읽기 실패의 이유를 `ReadMiss`로 돌려준다. 그래프를
진행시키는 source가 최신 게시보다 앞선(`Ahead`) 읽기를 보고하면 `CaptureAlignment::reacquire`로
다음 frontier를 받고, 그 cycle의 정렬 입력은 불연속으로 이력을 초기화한다. 이유별 계수는
graph worker가 주기 보고에 모은다.

`HalIoContext`는 기존 공유 매핑의 클럭·epoch·producer activity를 게시한다.
공유 PCM 링은 현재 매핑 형식의 수명 메타데이터를 보존하며 클라이언트 PCM의 저장소로 사용하지 않는다.
이 소유자는 PCM의 복사·합산이나 채널 변환 scratch를 관리하지 않는다.

HAL 네이티브 어댑터는 `IoOutcome`으로 호출 수락과 공급된 capture 프레임 수를 구분한다.
`Device`가 소유한 `IoTrace`는 명시적으로 활성화한 측정 동안 Float32 쓰기·읽기의
장치 sample 좌표·클라이언트 ID·공급 프레임·0 프레임 수·PCM 해시를 메모리에 남긴다. 콜백에서 할당하거나
파일에 쓰지 않으며 원본 PCM을 보관하지 않는다. 속성 조회는 일관성이 확인된 행만 전달한다.
관찰 수가 고정 용량을 넘거나 기록 경쟁으로 행을 잃으면 관찰 수·용량·누락 수로 드러난다.
HAL의 사용자 정의 진단 속성은 `CFPropertyList`로 선언하고 `CFDataRef`로 전달한다.
프로브의 `MRProbeHalTrace`가 이 전송과 파일 저장을 맡으며, 실패 단계·OSStatus·파일 오류를 구분한다.

편집기·엔진·제거 도구는 `licensing`의 `read_local_license`·`write_local_license`로 같은 저장 항목에 접근한다.
Linux 어댑터는 Secret Service의 잠긴 항목을 해제하거나 프롬프트·collection 생성을 시작하지 않는다.
저장은 고유한 unlocked 항목을 갱신하거나 기존 unlocked collection 안에 항목을 만든다.
잠김·중복·읽기 실패는 엔진의 기존 무료 접근 판정으로 이어지며, 저장·제거는 오류를 반환한다.
읽을 수 있는 고유 항목은 원래 entitlement와 만료를 검사한다.

## 그래프 준비와 오디오 블록 처리

`audio-engine::render::prepare`는 검증한 그래프의 포트·라우트·커널을 낮추고 저장 공간과
플러그인을 준비한다. provider의 저장 layout·bus·채널 검증은 `prepare::providers`가 맡는다.
준비 결과인 `RenderPlan`에는 숫자 인덱스와 불변 topology를 보관한다.

`ExternalIo`는 블록에서 사용하는 실행 gate·클럭·프레임 수를 네이티브 어댑터에 전달한다.
`render::cycle`은 admission, 외부 PCM 전송, DSP 실행, 녹음·미터 게시 순서를 소유한다.
용량 부족으로 거절한 cycle은 PCM이나 DSP 이력을 전진시키지 않는다.

`HostedRack`은 준비한 플러그인 인스턴스·재사용 인덱스·multibus scratch를 함께 소유한다.
인덱스는 인스턴스 목록에서 생성하며 외부에 가변 목록을 전달하지 않는다.
활성화는 준비된 인스턴스 전체를 옮기고, 폐기는 기존 retired 소유자의 회수 경로에서 수행한다.
단일 bus 처리와 multibus gather·scatter 및 dry fallback도 이 소유자의 처리 계약에 속한다.

`mixrelay-service`의 조립은 native 계약·DSP 준비·그래프 게시·제어·저장을 각 소유 모듈에 위임한다.
`session::NativeFormat`은 native 형식 callback과 설정을, `ProcessTelemetry`는 실패 계수를 소유한다.
macOS `worker`와 Linux `processing`은 오디오 실행을 소유하고 typed 결과를 공통
`worker_completion::Completion`에 전달한다. 작업자가 먼저 실패하면 제어 admission을 닫고,
shutdown은 worker join·복구·회수·기록을 모두 마친 뒤 각 경계의 오류를 모은다.

`processor-host`의 `WorkerProcess`는 인스턴스별 child·부모 수명 pipe·감독·회수를 소유한다.
creator와 DSP의 native 호출 진행은 서로 다른 고정 lane에 게시한다. 같은 진행 토큰의 호출이
기존 5초 예산을 넘으면 해당 child만 실패 처리하고 회수한다. 정상 GUI mouse tracking은
부모의 명시 응답 요청이 없으면 이 timeout 대상이 아니다. native 오류·불량 PCM과 늦은 wet 응답은
블록 좌표가 맞는 dry 경로로 대체하며 다른 인스턴스의 실행을 중지하지 않는다.
형식 어댑터는 native instance·processing·editor를 각각 소유하고 RT 이벤트 저장소는 준비 때 확보한다.
부모 control owner는 catalog의 플러그인 형식을 보존한다. 첫 typed process 실패와 native 상태값은
고정 크기 공유 슬롯에 한 번 게시하고, 부모 monitor가 OS·형식·단계·인스턴스와 함께 기록한다.
원인과 상태값은 release/acquire로 함께 확정하며 오류 개수는 독립 계수로 계속 관찰한다.

## 크레이트를 만들 때
- 플랫폼 중립 크레이트는 `crates/`, OS 어댑터는 `native/<os>/`, 실행 파일은 `apps/`에 둔다. 이름은 `mixrelay-<역할>`이다. 기존 C 라이브러리 바인딩만 담는 새 크레이트는 `-sys`로 끝낸다(M-FFI-NAMING).
- 워크스페이스 `members`에 추가하고, 버전·edition·린트는 `workspace = true`로 물려받는다(M-CARGO-WORKSPACE, M-CRATES-IN-WORKSPACE). 형제 크레이트는 `path` 의존으로 연결한다.
- 컴파일 단위나 테스트 경계가 분명히 나뉘거나 다른 계층의 책임이 섞이기 시작하면 크레이트를 나눈다. 모듈은 한 가지 책임과 비슷한 크기로 유지한다(M-SMALLER-CRATES, M-BALANCED-MODULES).
- 새 크레이트를 위 표와 `scripts/check-crate-layers.mjs`의 계층 목록에 함께 추가한다. 엔진 어댑터 행의 허용 의존은 `engineAdapter(같은 OS 어댑터...)`로 쓴다.
