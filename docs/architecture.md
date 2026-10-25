# 코드 구조와 책임

Rust 워크스페이스의 크레이트 책임, 의존 방향, 새 코드를 둘 위치를 정한다. 제품 수준의 실행 책임은 [제품 문서](product.md)의 "실행 책임과 수명", UI 구조는 [디자인 시스템](design-system.md)과 `check:boundaries`가 기준이다. Rust 코드 작성 규칙은 [Rust 작업 규칙](rust-guidelines.md)이다.

## 계층과 크레이트

의존은 위 계층에서 아래 계층으로만 향한다. 같은 계층 안의 의존은 표에 적은 것만 허용한다. `pnpm check:crate-layers`가 `cargo metadata`로 일반·build·dev 의존 모두를 크레이트별 허용 목록과 비교하고, 표에 없는 워크스페이스 크레이트도 실패로 처리한다. dev 의존에는 테스트 지원 크레이트가 추가로 허용된다.

| 계층 | 크레이트(경로) | 책임 | 허용 의존 |
| --- | --- | --- | --- |
| 기반 | `mixrelay-audio-io`(crates/audio-io) | 장치·스트림·형식·클럭 모델, FIFO·residence·클럭 브리지, 백엔드 trait | 없음 |
| 기반 | `mixrelay-pcm-ring`(native/pcm-ring) | 단일 생산자·단일 소비자 PCM 링 | 없음 |
| 기반 | `mixrelay-realtime`(crates/realtime) | 실시간 스레드 정책, 호스트 시각 | 없음 |
| 기반 | `mixrelay-local-socket`(crates/local-socket) | 사용자 소유 Unix 소켓, peer 확인 | 없음 |
| 기반 | `mixrelay-licensing`(crates/licensing) | 라이선스 확인·저장 | 없음 |
| 기반 | `mixrelay-processor-state`(crates/processor-state) | 플러그인 상태 저장 형식 | 없음 |
| 기반 | `mixrelay-engine-process`(crates/engine-process) | 엔진 프로세스 수명·소유권 | 없음 |
| 모델 | `mixrelay-audio-graph` | 그래프 문서, 검증, 실행 계획 컴파일 | 기반 |
| 모델 | `mixrelay-audio-recorder` | 녹음 파일·작업자 | 기반 |
| 모델 | `mixrelay-audio-engine` | 블록 처리, 렌더 계획, 지연 보상·측정 | 기반, `audio-graph`, `audio-recorder` |
| 계약 | `mixrelay-control-protocol` | 편집 앱 ↔ 엔진 제어 메시지 | 기반, 모델 |
| 계약 | `mixrelay-driver-control` | 엔진·앱 ↔ Driver 서비스 제어, 지연 권한 | 기반 |
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
| 앱 | `mixrelay-engine-service`(apps/engine-service) | 엔진 실행 파일: 조립·OS 연결·실행 수명 | 앱 외 전부 |
| 앱 | `mixrelay-driver-service`(apps/driver-service) | Driver 서비스 실행 파일 | 앱 외 전부 |
| 앱 | `mixrelay-desktop`(apps/desktop/src-tauri) | 편집 앱과 백그라운드 실행부(Tauri) | 앱 외 전부 |
| 테스트 지원 | `mixrelay-test-support`, `mixrelay-macos-test-support`, `tests/fixtures/*`, `mixrelay-pipewire-observer`(native/linux/pipewire/tests/buffer-observer) | 테스트 도구·진단용 preload | dev-dependency로만 사용 |
| vendor | `truce-rack-core`·`truce-rack-clap`·`truce-rack-vst3`(vendor/) | 플러그인 형식 어댑터(외부 코드) | `truce-rack-core`(clap·vst3) |

알려진 예외: `mixrelay-processor-host`가 플러그인 설명자·진단·편집기 프레임 타입 때문에 `mixrelay-control-protocol`에 의존한다. `control-server`는 제어 서버라서 계약을 구현하는 책임상 계약에 의존하고, 그 밖의 서비스 부품이 계약에 의존하는 것은 이것만 허용한다. 같은 종류의 의존을 새로 만들지 않는다. `mixrelay-processor-host`는 플러그인 형식과 편집기 표시 때문에 플랫폼별 코드(AppKit, X11, AU·LV2)도 가지며, 이 코드는 `cfg` 모듈 안에 둔다.

## 의존 규칙
- 모델·계약·서비스 부품은 플랫폼 중립이다. OS 어댑터와 앱에 의존하지 않고 OS API를 직접 호출하지 않는다. OS 기능이 필요하면 trait을 정의하고 어댑터나 앱이 구현한다.
- 기반 크레이트 중 `realtime`(스레드 정책·호스트 시각), `local-socket`(Unix 소켓·peer 확인), `licensing`(네이티브 credential 저장)은 그 책임상 OS를 직접 호출한다. 이 호출은 플랫폼별 `cfg` 모듈 안에 두고, 그 위의 판단은 플랫폼 중립 API로 노출한다.
- OS 어댑터는 다른 OS의 어댑터에 의존하지 않는다. 앱은 다른 앱에 의존하지 않는다.
- 테스트 지원 크레이트는 `[dev-dependencies]`로만 쓴다. 제품 코드가 테스트 도구에 의존하지 않는다.

## 책임 배치
- 판단과 정책은 플랫폼 중립 크레이트나 어댑터의 안전한 Rust 부분에 둔다. C 브리지(`MixRelayHALBridge.c`, `*.h`)와 `extern "C"` 함수는 ABI 변환만 한다(M-FFI-TRANSLATES).
- 앱 크레이트는 부품을 조립하고 OS·프로세스 수명을 연결한다. 여러 앱이 쓰거나 테스트할 가치가 있는 로직은 계약·서비스 부품 크레이트로 옮긴다.
- 새 코드를 둘 곳:
  - 장치·OS에 무관한 오디오 처리 → `audio-engine`, `audio-graph`, `audio-io`
  - 프로세스 사이 메시지 형식 → `control-protocol`(편집 앱 ↔ 엔진), `driver-control`(↔ Driver 서비스)
  - CoreAudio·PipeWire 호출 → 해당 OS 어댑터. 엔진 쪽 OS 연결은 `apps/engine-service`의 `macos_devices`·`linux_*`
  - HAL 동작 → `hal-core`(Rust), ABI 연결만 C 브리지
- 테스트 배치는 [Rust 작업 규칙](rust-guidelines.md)의 테스트 절을 따른다. 테스트용 가짜는 `#[cfg(test)]` 뒤에 두고 제품 코드의 가시성을 넓히지 않는다.

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

HAL 네이티브 어댑터는 `IoOutcome`으로 호출 수락과 공급된 capture 프레임 수를 구분한다.
`Device`가 소유한 `IoTrace`는 명시적으로 활성화한 측정 동안 Float32 쓰기·읽기의
장치 sample 좌표·공급 프레임·0 프레임 수·PCM 해시를 메모리에 남긴다. 콜백에서 할당하거나
파일에 쓰지 않으며 원본 PCM을 보관하지 않는다. 속성 조회는 일관성이 확인된 행만 전달한다.
관찰 수가 고정 용량을 넘거나 기록 경쟁으로 행을 잃으면 관찰 수·용량·누락 수로 드러난다.
HAL의 사용자 정의 진단 속성은 `CFPropertyList`로 선언하고 `CFDataRef`로 전달한다.
프로브의 `MRProbeHalTrace`가 이 전송과 파일 저장을 맡으며, 실패 단계·OSStatus·파일 오류를 구분한다.

편집기·엔진·제거 도구는 `licensing`의 `read_local_license`·`write_local_license`로 같은 저장 항목에 접근한다.
Linux 어댑터는 Secret Service의 잠긴 항목을 해제하거나 프롬프트·collection 생성을 시작하지 않는다.
저장은 고유한 unlocked 항목을 갱신하거나 기존 unlocked collection 안에 항목을 만든다.
잠김·중복·읽기 실패는 엔진의 기존 무료 접근 판정으로 이어지며, 저장·제거는 오류를 반환한다.
읽을 수 있는 고유 항목은 원래 entitlement와 만료를 검사한다.

## 크레이트를 만들 때
- 플랫폼 중립 크레이트는 `crates/`, OS 어댑터는 `native/<os>/`, 실행 파일은 `apps/`에 둔다. 이름은 `mixrelay-<역할>`이다. 기존 C 라이브러리 바인딩만 담는 새 크레이트는 `-sys`로 끝낸다(M-FFI-NAMING).
- 워크스페이스 `members`에 추가하고, 버전·edition·린트는 `workspace = true`로 물려받는다(M-CARGO-WORKSPACE, M-CRATES-IN-WORKSPACE). 형제 크레이트는 `path` 의존으로 연결한다.
- 컴파일 단위나 테스트 경계가 분명히 나뉘거나 다른 계층의 책임이 섞이기 시작하면 크레이트를 나눈다. 모듈은 한 가지 책임과 비슷한 크기로 유지한다(M-SMALLER-CRATES, M-BALANCED-MODULES).
- 새 크레이트를 위 표와 `scripts/check-crate-layers.mjs`의 계층 목록에 함께 추가한다.
