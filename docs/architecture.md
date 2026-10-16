# 코드 구조와 책임

Rust 워크스페이스의 크레이트 책임, 의존 방향, 새 코드를 둘 위치를 정한다. 제품 수준의 실행 책임은 [제품 문서](product.md)의 "실행 책임과 수명", UI 구조는 [디자인 시스템](design-system.md)과 `check:boundaries`가 기준이다. Rust 코드 작성 규칙은 [Rust 작업 규칙](rust-guidelines.md)이다.

## 계층과 크레이트

의존은 위 계층에서 아래 계층으로만 향한다. 같은 계층 안의 의존은 표에 적은 것만 허용한다. `pnpm check:crate-layers`가 `cargo metadata`로 일반·build·dev 의존 모두에 대해 이 표를 검사한다. dev 의존에는 테스트 지원 크레이트가 추가로 허용된다.

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
| 서비스 부품 | `mixrelay-processor-host` | VST3·CLAP 플러그인 호스팅 | 기반, 모델, 계약, `vendor/truce-rack-*` |
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
| 테스트 지원 | `mixrelay-test-support`, `mixrelay-macos-test-support`, `tests/fixtures/*` | 테스트 도구 | dev-dependency로만 사용 |

알려진 예외: `mixrelay-processor-host`가 플러그인 설명자·진단·편집기 프레임 타입 때문에 `mixrelay-control-protocol`에 의존한다. 검사는 이 의존만 허용하고, 같은 종류의 의존을 새로 만들지 않는다.

## 의존 규칙
- 모델·계약·서비스 부품은 플랫폼 중립이다. OS 어댑터와 앱에 의존하지 않고 OS API를 직접 호출하지 않는다. OS 기능이 필요하면 trait을 정의하고 어댑터나 앱이 구현한다.
- 기반 크레이트 중 `realtime`(스레드 정책·호스트 시각)과 `local-socket`(Unix 소켓·peer 확인)은 그 책임상 OS를 직접 호출한다. 이 호출은 플랫폼별 `cfg` 모듈 안에 두고, 그 위의 판단은 플랫폼 중립 API로 노출한다.
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
- 테스트는 단위 테스트를 모듈 옆(`#[cfg(test)]`, `*_tests.rs`)에, 공개 API만 쓰는 테스트를 크레이트의 `tests/`에 둔다(M-INTEGRATION-TESTS). 테스트용 가짜는 `#[cfg(test)]` 뒤에 두고 제품 코드의 가시성을 넓히지 않는다.

## 크레이트를 만들 때
- 플랫폼 중립 크레이트는 `crates/`, OS 어댑터는 `native/<os>/`, 실행 파일은 `apps/`에 둔다. 이름은 `mixrelay-<역할>`이다. 기존 C 라이브러리 바인딩만 담는 새 크레이트는 `-sys`로 끝낸다(M-FFI-NAMING).
- 워크스페이스 `members`에 추가하고, 버전·edition·린트는 `workspace = true`로 물려받는다(M-CARGO-WORKSPACE, M-CRATES-IN-WORKSPACE). 형제 크레이트는 `path` 의존으로 연결한다.
- 컴파일 단위나 테스트 경계가 분명히 나뉘거나 다른 계층의 책임이 섞이기 시작하면 크레이트를 나눈다. 모듈은 한 가지 책임과 비슷한 크기로 유지한다(M-SMALLER-CRATES, M-BALANCED-MODULES).
- 새 크레이트를 위 표와 `scripts/check-crate-layers.mjs`의 계층 목록에 함께 추가한다.
