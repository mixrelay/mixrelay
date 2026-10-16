# Rust 작업 규칙

[Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt) 중 MixRelay에 해당하는 항목만, 적용 조건과 함께 둔다. 괄호 안은 원문 항목 ID다. Rust 코드를 쓰거나 리뷰하는 사람과 에이전트는 작업에 해당하는 조건의 항목을 먼저 확인한다. 기존 설정(`Cargo.toml`의 `workspace.lints`: clippy `all`·`pedantic`·`perf` deny, `unreachable_pub` deny)은 그대로 지킨다.

## 테스트를 쓰거나 고칠 때
- **OS·장치·시계·IPC를 건드리는 코드는 주입 가능한 경계 뒤에 둔다(M-MOCKABLE-SYSCALLS).** CoreAudio·PipeWire·설치된 Driver·서비스 소켓·`proc_pidinfo`·호스트 시각이 해당한다. 기본 테스트는 합성 장치나 가짜 백엔드로 돌고, 실제 장치·설치본·환경 변수·외부 도구(`/usr/bin/leaks` 등)·실제 경과 시간에 기대지 않는다. 개발 Mac, macOS CI, Linux CI에서 같은 결과여야 한다. 기존 경계 예: `macos_devices::tests::synthetic_device`, `device_io` `Backend`·`ListenerBackend`, `transport::ProcessInspection`, `GraphResourceBackend`.
- **설치 환경의 제약은 테스트 기본값으로 둔다.** HAL 테스트는 샌드박스(`ProcessInspection::SANDBOXED`)로 돈다. 자세한 목록은 [개발 작업 규칙](development.md).
- **테스트 전용 가짜·우회는 제품 빌드에 들어가지 않는다(M-TEST-UTIL).** `#[cfg(test)]` 또는 테스트용 feature 뒤에 둔다. 제품 코드의 가시성을 테스트 때문에 넓히지 않는다.
- **정의를 되풀이하는 테스트를 쓰지 않는다(M-TAUTOLOGICAL-TESTS).** 상수를 그대로 비교하거나 구현의 분기를 따라 쓴 테스트 대신, 실제 신호·시각·상태로 관찰 가능한 동작을 확인한다. 예: 지연 보고는 임펄스가 실제로 걸린 시간과 비교한다. 새 테스트는 고치기 전 코드에서 실패하는지 확인한다.
- **`#[ignore]`로 실패를 숨기지 않는다.** 검증할 동작은 기본 테스트로 옮긴다.

## 실패·패닉을 다룰 때
- 릴리스 프로필은 `panic = "abort"`다. 패닉은 엔진이나 coreaudiod 안의 HAL 호스트 프로세스를 즉시 끝낸다(M-PANIC-IS-STOP).
- 패닉은 프로그래밍 오류(불변식 위반)에만 쓴다. 장치 실패·IPC 오류·사용자 입력·외부 데이터는 `Result`로 돌려준다(M-PANIC-ON-BUG). 오디오 콜백과 HAL 속성 경로에서는 패닉 가능 코드(`unwrap`, 인덱스 범위 초과, 산술 overflow 검사)를 피하고 실패를 안전한 기본값으로 처리한다.
- 의도한 패닉(`expect`·`assert!`·`unreachable!`)에는 무엇이 왜 잘못됐는지와 관련 값을 메시지로 남긴다(M-PANIC-MESSAGE). 테스트 안에서는 필요 없다.
- `catch_unwind`로 패닉을 잡아 계속 진행하지 않는다(M-PANIC-CONTINUATION).

## `unsafe`·FFI를 쓸 때
- `unsafe`는 FFI·플랫폼 호출, 측정된 성능상의 필요, 새 추상화에만 쓴다. `Send` 우회, 수명 우회, 편의를 위한 `transmute`에 쓰지 않는다(M-UNSAFE).
- 모든 `unsafe` 블록에 왜 안전한지 `// SAFETY:` 주석을 단다. 함수에 `unsafe`를 붙이는 것은 잘못 쓰면 미정의 동작이 생길 때뿐이다(M-UNSAFE-IMPLIES-UB). 안전한 API가 미정의 동작을 낼 수 있으면 안 된다(M-UNSOUND).
- 판단·정책은 안전한 Rust core에 두고, C 브리지(`MixRelayHALBridge.c`)와 `extern "C"` 함수는 변환만 한다(M-FFI-TRANSLATES).

## 실시간 경로를 고칠 때
- 오디오 콜백·IOProc·HAL IO 경로가 hot path다. 할당·잠금·시스템 호출 없이 원자 연산만 쓰는지 확인하고, 기존 할당 측정 테스트(`allocation_counter`)를 유지·추가한다(M-HOTPATH).
- 반복 사용 버퍼는 미리 잡고 재사용한다(M-MEM-REUSE).

## 타입·API를 만들 때
- 의미가 있는 값(장치 ID, PID와 그 incarnation, 프레임 수와 ns, 샘플레이트)은 원시 타입 대신 의미 있는 타입을 쓰고, 불변식이 있는 newtype은 생성자에서 검증한다. 약한 타입에서의 변환은 `TryFrom`이다(M-STRONG-TYPES, M-STRONG-TYPES-GUARD).
- 공개 항목은 한 경로로만 노출하고 glob 재노출(`pub use foo::*`)을 하지 않는다(M-SINGLE-ITEM-PATH, M-NO-GLOB-REEXPORTS).
- 다른 언어의 구조를 옮기지 말고 Rust 관용으로 쓴다(M-RUST-SHAPED). 전역 `static` 상태를 새로 만들지 않는다(M-AVOID-STATICS). 고정 대상 전용 호환 목록처럼 꼭 필요한 경우 이유를 문서에 남긴다.

## 상수·린트·로그를 쓸 때
- 숫자 상수에는 왜 그 값인지, 바꾸면 무엇이 달라지는지, 어떤 외부 시스템과 맞물리는지 주석을 단다. 이름 있는 상수를 우선한다(M-DOCUMENTED-MAGIC).
- 린트를 끄는 새 코드는 `#[allow]` 대신 `#[expect(lint, reason = "...")]`를 쓴다(M-LINT-OVERRIDE-EXPECT).
- 로그는 `tracing`의 이름 있는 필드와 고정 메시지로 쓴다. `format!`으로 문자열을 만들어 넘기지 않는다(M-LOG-STRUCTURED). 제품 코드에서 `println!`·`dbg!`를 쓰지 않는다(M-LOG-NOT-PRINT). 장치 이름·경로·인증 정보는 남기지 않는다.

## 문서·주석을 쓸 때
- 코드 문서와 설계 문서에는 최종 동작과 계약을 쓴다. "왜 X 대신 Y를 골랐나" 같은 작업 경위, 지켰다고 주장하는 규칙 표, 리뷰 이력은 넣지 않는다. 그런 내용은 커밋 메시지에 둔다(M-NO-META-DESIGN-DOCUMENTATION).
- 모듈과 공개 항목에는 문서를 단다. 첫 문장은 한 줄로 요약한다(M-MODULE-DOCS, M-FIRST-DOC-SENTENCE).
