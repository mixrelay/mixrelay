# Rust 작업 규칙

MixRelay의 Rust 코드에 적용하는 규칙이다. 작업에 해당하는 조건의 절을 먼저 읽는다. 크레이트 책임과 의존 방향은 [코드 구조와 책임](architecture.md)이 정한다. 괄호 안은 근거다: MS = [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt), API = [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html), DP = [Rust Design Patterns](https://rust-unofficial.github.io/patterns/), Book = [The Rust Programming Language](https://doc.rust-lang.org/book/).

## 강제 수단
- **지금 검사가 막는 것:** rustc `rust_2018_idioms`·`unreachable_pub`·`unused`, clippy `all`·`pedantic`·`perf`(deny), `clippy.toml`의 함수 80줄·인자 5개 한도, `cargo fmt`, `pnpm check:crate-layers`(의존 방향). CI(`pnpm lab:test`, macOS·Linux)는 의존 방향 검사, fmt, clippy, `cargo test --workspace --lib --bins`, 제어 UI 테스트를 실행하고, macOS에서는 `pnpm check:native`, 왕복 프로브 분석 자체 시험(`probe:macos-roundtrip --self-test`), 빌드·서명 도구 테스트(`pnpm test:scripts`)를 더한다. `pnpm check:rust`는 로컬 검사로 fmt, `cargo machete`(쓰지 않는 의존), `--all-features` clippy, Rust 커버리지를 실행한다.
- **규칙이지만 아직 검사가 막지 않는 것:** 아래의 `unwrap`·`expect` 금지, SAFETY 주석, 문자열 에러 금지, 모듈 크기. 리뷰에서 확인하고, 검사를 추가할 때 이 목록을 갱신한다.
- 린트를 끄는 새 코드는 `#[allow]` 대신 `#[expect(lint, reason = "...")]`를 쓴다(MS M-LINT-OVERRIDE-EXPECT). `restriction`·`nursery` 그룹은 통째로 켜지 않고 항목을 골라 켠다([Clippy](https://doc.rust-lang.org/clippy/lints.html)). 코드에 `#![deny(warnings)]`를 쓰지 않는다(DP 안티패턴).

## 구조를 만들거나 옮길 때
- 크레이트 배치·이름·의존 방향은 [코드 구조와 책임](architecture.md)을 따른다. 워크스페이스 설정(버전·edition·린트·의존 버전)은 루트 `Cargo.toml`에서 물려받는다(MS M-CARGO-WORKSPACE).
- 실행 파일의 `main.rs`는 얇게 두고 로직은 라이브러리 모듈에 둔다. 통합 테스트는 라이브러리만 가져올 수 있다(Book 11.3).
- feature는 더하기만 한다. 서로 배타적인 feature를 만들지 않는다([Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)).
- 모듈 파일은 `foo.rs` + `foo/` 디렉터리 형식이다(`mod.rs`를 새로 만들지 않는다). 모듈은 한 책임과 비슷한 크기를 유지하고 `errors`·`utils`·`traits` 같은 잡동사니 모듈을 만들지 않는다(MS M-BALANCED-MODULES, DP). 한 파일이 여러 책임을 갖거나 1,000줄을 넘기 시작하면 책임별로 나눈다.

## 이름·타입·공개 범위를 정할 때
- 이름은 RFC 430 대소문자 규칙을 따른다. 변환은 `as_`(비용 없음)·`to_`(비용 있음)·`into_`(소유권 이동), getter에 `get_`을 붙이지 않는다(API C-CASE, C-CONV, C-GETTER).
- `Manager`·`Service`·`Factory`·`Helper` 같은 뜻 없는 접미사를 피하고 짧게 짓는다(MS M-WEASEL-WORDS, M-SHORT-NAMES).
- 장치 ID, PID와 그 incarnation, 프레임 수와 ns, 샘플레이트처럼 의미가 있는 값은 원시 타입 대신 의미 있는 타입을 쓴다. `bool` 인자 대신 enum을 쓴다. 불변식이 있는 newtype은 실패할 수 있는 생성자(`TryFrom`·`from_*`)에서 검증하고 약한 타입에서의 무조건 `From`을 두지 않는다(API C-NEWTYPE, C-CUSTOM-TYPE, MS M-STRONG-TYPES-GUARD).
- 공개 범위는 가장 좁게 둔다(`pub(crate)`·`pub(super)`). 불변식이 있는 구조체의 필드는 비공개로 두고 setter를 만들지 않는다. 공개 항목은 한 경로로만 노출하고 glob 재노출·prelude를 만들지 않는다(API C-STRUCT-PRIVATE, MS M-SINGLE-ITEM-PATH, M-NO-GLOB-REEXPORTS, M-NO-PRELUDE).
- 공개 타입에는 `Debug`를 구현하고, 의미가 맞으면 `Clone`·`PartialEq` 등 공통 trait을 구현한다(API C-DEBUG, C-COMMON-TRAITS).
- 한 번 쓰고 버리는 "실행 객체" 대신 함수를 쓴다. 분기는 호출자 쪽으로 올리고 반복은 안쪽으로 내린다([rust-analyzer style](https://rust-analyzer.github.io/book/contributing/style.html), [push ifs up, fors down](https://matklad.github.io/2023/11/15/push-ifs-up-and-fors-down.html)).

## 설계·의존을 정할 때
- 판단 로직은 입출력 없이 데이터를 받아 결과를 돌려주는 순수한 core로 두고, OS·소켓·파일·시계를 다루는 껍질은 얇게 둔다(functional core / imperative shell, MS M-IMPL-IO).
- OS·장치·시계·IPC·프로세스 조회는 주입 가능한 경계 뒤에 둔다(MS M-MOCKABLE-SYSCALLS). 기존 경계: `macos_devices::host::Host`, `audio-io`의 `*Backend`, `device_io` `Backend`·`ListenerBackend`, `transport::ProcessInspection`, `GraphResourceBackend`. 입출력 타입에 실제 OS에 연결하는 `Default`를 두지 않는다.
- 주입 방법은 구체 타입 → 제네릭 → `dyn` 순으로 고른다. 경계 하나에 trait 하나를 두고 trait을 작게 유지한다(MS M-DI-HIERARCHY).
- C 브리지와 `extern "C"` 함수는 ABI 변환만 하고 정책은 안전한 Rust에 둔다(MS M-FFI-TRANSLATES).
- 상태를 가진 전역 `static`(`Mutex`·`OnceLock`·전역 원자 변수)을 새로 만들지 않는다. 상태는 소유자를 통해 전달한다. 성능용 캐시처럼 정확성에 영향이 없는 것만 예외다(MS M-AVOID-STATICS).
- 빌림 검사기를 피하려고 `clone`하거나 `Deref`로 상속을 흉내 내지 않는다. 구조체를 나눠 독립적으로 빌린다(DP 안티패턴, Struct decomposition).

## 에러를 다룰 때
- 실패할 수 있는 동작은 `Result`를 돌려준다(Book 9.3).
- **제품 코드에는 `unwrap()`·`expect()`를 쓰지 않는다. 테스트에서만 쓴다.** `?`·`let … else`·`ok_or`로 전달하거나 불변식을 타입으로 보장한다. 잠금 poison은 기존 `lock()` 도우미처럼 `into_inner`로 처리한다.
- 라이브러리 크레이트의 에러는 `thiserror` enum이나 구조체로 정의하고 `Error + Send + Sync + Display + Debug`를 구현한다. 앱 크레이트의 최상위는 `anyhow`를 써도 된다(API C-GOOD-ERR, MS M-APP-ERROR).
- 새 코드에서 `Result<_, String>`이나 `map_err(|e| e.to_string())`으로 에러를 문자열로 바꿔 전달하지 않는다. 호출자가 원인별로 처리할 수 없게 된다. 변환은 `From`을 한 번 구현해 `?`로 하고, 문맥 추가가 필요할 때만 `map_err`·`context`를 쓴다(MS M-FROM-ERROR).

## 패닉을 다룰 때
- 릴리스 프로필은 `panic = "abort"`다. 패닉은 엔진이나 coreaudiod 안의 HAL 호스트 프로세스를 즉시 끝낸다(MS M-PANIC-IS-STOP).
- 장치 실패·IPC 오류·사용자 입력·외부 데이터는 `Result`로 돌려준다(MS M-PANIC-ON-BUG). 오디오 콜백과 HAL 속성 경로에서는 인덱스 범위 초과·산술 overflow 같은 패닉 가능 코드를 피하고 실패를 안전한 기본값으로 처리한다.
- 남는 의도한 패닉(`assert!`·`unreachable!`)은 프로그래밍 오류에만 쓰고, 무엇이 왜 잘못됐는지와 관련 값을 메시지로 남긴다(MS M-PANIC-MESSAGE).
- `catch_unwind`로 패닉을 잡아 계속 진행하지 않는다(MS M-PANIC-CONTINUATION). FFI 경계에서 unwind가 C로 넘어가지 않게 막는 용도만 허용하고, 그 이유를 주석에 쓴다.

## `unsafe`·FFI를 쓸 때
- `unsafe`는 FFI·플랫폼 호출, 측정된 성능상의 필요, 새 추상화에만 쓴다. `Send`·`Sync` 우회, 수명 우회, 편의를 위한 `transmute`에 쓰지 않는다(MS M-UNSAFE). `unsafe impl Send/Sync`에는 왜 안전한지 근거를 쓴다.
- 모든 `unsafe` 블록에 왜 안전한지 `// SAFETY:` 주석을 단다. 함수에 `unsafe`를 붙이는 것은 잘못 쓰면 미정의 동작이 생길 때뿐이고, 그 조건을 `# Safety` 문서에 쓴다(MS M-UNSAFE-IMPLIES-UB). 안전한 API가 미정의 동작을 낼 수 있으면 안 된다(MS M-UNSOUND).
- unsafe는 가장 작은 모듈 안에 가두고 공개 범위로 경계를 만든다([Nomicon](https://doc.rust-lang.org/nomicon/working-with-unsafe.html)). edition 2024 규칙(`unsafe extern`, `#[unsafe(no_mangle)]`, `unsafe fn` 안의 명시적 `unsafe` 블록)을 따른다([Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/)).
- FFI로 받은 포인터·버퍼는 쓰기 전에 null, 정렬, 길이, 형식(layout)을 확인하고, 그 전제를 함수 문서에 쓴다. 누가 메모리를 소유하고 해제하는지 정한다.
- 콜백을 등록한 쪽은 해제 순서를 지킨다. 새 I/O를 막고, 진행 중인 콜백이 끝나기를 기다린 뒤 버퍼를 해제하고, 늦게 도착하는 종료 호출이 쓰는 정보는 마지막 참조까지 유지한다([제품 문서](product.md) "실행 책임과 수명").
- FFI가 없는 순수 Rust unsafe(링 버퍼, 원자 연산 구조)는 Miri로 확인할 수 있게 테스트를 둔다([Miri](https://github.com/rust-lang/miri)).

## 실시간 경로를 고칠 때
- 오디오 콜백·IOProc·HAL IO 경로에서는 할당·해제, 잠금 대기, 파일·소켓 I/O, 시스템 호출, 다른 스레드 대기를 하지 않는다. 버퍼는 미리 잡고 단일 생산자·단일 소비자 큐와 원자 연산을 쓴다([Real-time audio programming 101](http://www.rossbencina.com/code/real-time-audio-programming-101-time-waits-for-nothing)). 기존 할당 측정 테스트(`allocation_counter`)를 유지·추가한다(MS M-HOTPATH).
- 공유 PCM과 측정 탭은 쓰는 쪽을 하나로 두고 명시적 수명과 epoch를 유지한다([제품 문서](product.md) "실행 책임과 수명"). 여러 스레드가 갱신하는 회수용 카운터(예: reader 수)는 이 규칙의 예외지만, 증가·감소의 짝과 회수 시점을 주석으로 설명한다.
- 원자 연산마다 메모리 순서를 고른 근거를 주석으로 남긴다. 여러 값을 함께 읽어야 하면 sequence 번호로 일관성을 확인하되, 실시간 경로에서는 재시도 횟수를 제한하고 실패하면 그 창을 건너뛰는 등 실패 시 동작을 정한다.
- tokio 런타임에서 `.await`로 양보하지 않는 CPU 루프나 계속 블로킹하는 작업은 전용 스레드에 둔다. `spawn_blocking`은 끝나는 짧은 블로킹 작업에만 쓰고, 시작된 작업은 `abort`로 멈추지 않으므로 종료를 기다리는 방법을 함께 정한다. `.await`로 양보하는 서버·구독 루프는 일반 task로 둔다([tokio](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)).
- 실시간 경로가 쓰는 PCM·공유 메모리 버퍼는 비실시간에서 기존 공통 fallible 예약(`try_reserve*`)으로 준비하고, 실패하면 부분 할당을 회수하고 활성 오디오를 유지한다([제품 문서](product.md) "공통 안전·이력 정책"). 그 밖의 컬렉션은 반복 사용 시 재사용하고 크기를 알면 `with_capacity`로 만든다(MS M-MEM-REUSE, [Rust Performance Book](https://nnethercote.github.io/perf-book/heap-allocations.html)).

## 테스트를 쓰거나 고칠 때
- 기본 테스트는 개발 Mac, macOS CI, Linux CI에서 같은 결과여야 한다. 실제 장치·설치본·서비스 소켓·사용자 키체인·환경 변수·외부 도구(`/usr/bin/leaks` 등)·실제 경과 시간에 기대지 않고, 위 경계의 가짜나 합성 장치(`macos_devices::tests::synthetic_device`, `simulated`)로 돈다. 설치 환경의 제약(HAL 샌드박스 등)은 테스트 기본값으로 둔다([개발 작업 규칙](development.md)).
- 라이브러리의 `#[cfg(test)]`는 그 크레이트 자신의 단위 테스트에만 적용된다. 같은 패키지의 `--bins` 테스트나 다른 크레이트의 테스트가 링크하는 라이브러리는 제품 설정으로 컴파일되므로, 키체인·OS 서비스 같은 실제 의존을 `cfg(test)` 기본값으로 피하지 않는다. 생성 설정의 필수 인자로 받고 제품 진입점만 실제 구현을 넘긴다(예: `EngineGraphControllerConfig::paid_access`). 테스트 빌드마다 서명이 바뀌어 키체인 승인 창이 매번 뜬다.
- `#[ignore]`로 실패를 숨기지 않는다. 검증할 동작은 기본 테스트로 옮긴다. 장시간 soak·벤치마크만 ignore할 수 있고, 그 이유를 `#[ignore = "..."]`에 쓴다.
- 테스트 전용 가짜·생성자·우회는 `#[cfg(test)]`나 테스트용 feature 뒤에 둔다. 테스트 때문에 제품 코드의 공개 범위를 넓히지 않는다(MS M-TEST-UTIL).
- 정의를 되풀이하는 테스트(상수 비교, 구현 분기 따라 쓰기)를 쓰지 않는다. 실제 신호·시각·상태로 관찰 가능한 동작을 확인하고, 결함을 막는 테스트는 결함을 넣었을 때 실패하는지 확인한다(MS M-TAUTOLOGICAL-TESTS).
- 단위 테스트는 모듈 옆에 둔다. 새 코드는 `foo/tests.rs`(`#[cfg(test)] mod tests;`) 형식을 쓴다. CI는 `--lib --bins`, 커버리지(`pnpm coverage:rust`)는 `--lib` 대상만 실행한다. 실행 파일(`--bins`) 테스트는 커버리지에 잡히지 않고, 크레이트의 `tests/` 통합 테스트는 어느 쪽도 실행하지 않으므로 두 명령에 `--tests`가 더해질 때까지 새로 만들지 않는다(Book 11.3, MS M-INTEGRATION-TESTS).
- 복구 가능한 실패는 `Err`·`None`을 확인하고, 의도한 panic 계약은 `#[should_panic(expected = "...")]`로 확인한다(Book 11.1).
- 반복을 기다리는 테스트 루프에는 횟수 상한을 두고, 진행이 없으면 실패하게 한다. 테스트가 멈추면 안 된다.

## 로그·상수·문서를 쓸 때
- 로그는 `tracing`의 이름 있는 필드와 고정 메시지로 쓴다. 메시지에 `format!`으로 만든 문자열을 넣지 않는다(MS M-LOG-STRUCTURED). 라이브러리·서비스 코드에서 `println!`·`dbg!`를 쓰지 않는다(CLI 출력은 예외, MS M-LOG-NOT-PRINT). 장치 이름·경로·인증 정보는 남기지 않는다.
- 숫자 상수에는 왜 그 값인지, 바꾸면 무엇이 달라지는지, 어떤 외부 시스템과 맞물리는지 주석을 단다. 이름 있는 상수를 우선한다(MS M-DOCUMENTED-MAGIC).
- 모듈 첫머리에 `//!` 문서를 둔다. 문서 첫 문장은 한 줄 요약이고, 필요하면 `# Errors`·`# Panics`·`# Safety` 절을 둔다(MS M-MODULE-DOCS, M-CANONICAL-DOCS, API C-FAILURE).
- 코드 문서와 설계 문서에는 최종 동작과 계약을 쓴다. 작업 경위, "왜 X 대신 Y" 경위, 지켰다고 주장하는 규칙 표, 리뷰 이력은 커밋 메시지에 둔다(MS M-NO-META-DESIGN-DOCUMENTATION).

## 피해야 할 것 요약
제품 코드의 `unwrap`·`expect`, 문자열 에러, 테스트를 위한 공개 범위 확대, 실제 장치·환경에 기대는 기본 테스트, `#[ignore]`로 숨긴 실패, 상태를 가진 전역 static, SAFETY 근거 없는 unsafe, 실시간 경로의 할당·잠금, 빌림을 피하려는 clone, 잡동사니 모듈, 수천 줄짜리 파일, 설계 경위를 담은 문서, `#[allow]` 남발.
