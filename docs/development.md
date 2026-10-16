# 개발 작업 규칙

폐기된 `Library/Caches/io.mixrelay.signing/driver-20261008/release.keychain-db`는 사용자 키체인 검색 목록에서도 제외한다. `pnpm signing:check --repair-search-list`로 이 검색 항목만 정리한다. 키체인 파일이나 개인 키를 삭제하거나 자동 잠금을 해제하지 않는다. 서명 도구는 빌드 종료 시에도 이 항목을 되살리지 않고 나머지 검색 목록을 복원한다. 기본 검색 목록은 다른 프로젝트와 공유된다.

**릴리스 빌드 전 서명 준비가 최우선이다.** 정상 복원 설정은 `/Users/state303/Library/Application Support/MixRelay/Signing/background-stop-20261011/config.json`이다. `pnpm signing:check`는 이 설정으로 키체인을 열고 실제 코드·PKG 서명을 검증한다. `pnpm release:macos --output EMPTY_DIRECTORY`는 같은 정상 키체인을 사용하며 종료 시 원래 키체인 검색 목록을 복원한다. 이전 `Library/Caches/io.mixrelay.signing/driver-20261008/release.keychain-db`는 저장된 암호로 열리지 않는다고 기존 운영 지침에 명시돼 있으므로 기본 서명 경로로 재사용하지 않는다.

인증창이 남으면 즉시 조치를 선택한다. 정상 설정으로 잠금 해제·재시도하거나, 자신이 시작한 막힌 서명 프로세스만 안전하게 취소하고 재시작하거나, 필요한 암호·행동을 구체적으로 사용자에게 요청한다. 창을 방치한 채 다른 작업으로 넘어가지 않는다. 정상 전용 키체인도 300초 뒤 잠기므로 빌드 동안만 45초 간격으로 잠금을 갱신한다. 빌드 종료 시 갱신 프로세스를 종료하며 로그인 키체인과 전역 잠금 정책은 변경하지 않는다.

빌드를 막는 열린 인증창의 처리 방법은 에이전트가 능동적으로 결정한다. 사용자가 다시 지적할 때까지 기다리지 않는다. 취소 전에는 창을 만든 자신의 빌드·서명 프로세스인지 확인한다. 사용자 입력이 꼭 필요하면 어느 창에서 무엇을 해야 하는지 즉시 요청하고, 실제 빌드가 대기 중임을 알린다. 조치 뒤에는 인증 대기가 해소되고 빌드가 재개 또는 종료됐는지 확인한다.

코드 서명·키체인 인증창은 다른 작업보다 먼저 즉시 확인한다. 사용자가 제공한 기존 암호와 승인 범위 안에서 입력·승인을 바로 처리하고, 창을 방치한 채 다른 작업으로 미루지 않는다. 서명 로그가 멈추면 인증창 대기 여부부터 확인하며 빌드가 진행 중인 것처럼 보고하지 않는다. 도구가 인증창 조작을 차단하거나 암호가 없으면 즉시 정확한 원인과 필요한 사용자 입력을 알린다. 암호를 문서·로그에 저장하지 않는다. 현재 CUA는 macOS `SecurityAgent` 조작을 보안상 차단할 수 있으므로 실제 도구 결과를 기준으로 판단한다.

임시 파일 생성은 가급적 피한다. 짧은 일회성 조회·수정은 기존 도구와 직접 실행하는 명령으로 처리한다.

반복해서 사용할 긴 스크립트는 프로젝트 도구로 관리한다. 매번 임시 스크립트를 새로 만들지 않는다.
도구는 해당 프로젝트의 `scripts/` 등 기존 도구 위치에 두고, 사람이 짧은 명령으로 실행할 수 있게 연결한다.
실행 방법·입력·출력·실패 조건을 문서나 `--help`로 안내한다.

검사 도구는 기본적으로 결과를 터미널에 출력하고, 파일 저장이 필요할 때 명시적으로 선택하게 한다.
반복 실행 시 불필요한 파일을 누적하지 않는다. 검증 증거 등 보존할 산출물은 용도와 위치를 분명히 한다.

UI 조작 위치 목록은 `pnpm --filter @mixrelay/control-ui audit:ui`로 확인한다.
`--json`은 표준 출력에 구조화된 목록을 제공하며 파일을 생성하지 않는다. `--help`로 사용법을 확인한다.
목록은 소스 검토의 범위이며 실제 접근성 이름·키보드 동작은 테스트와 Storybook에서 검증한다.

UI 테스트(`apps/control-ui`)는 확장자로 실행 환경을 나눈다. `*.test.tsx`는 jsdom(`dom` 프로젝트, `src/test-setup.ts`)에서, `*.test.ts`는 DOM 없는 Node(`node` 프로젝트, `src/test-setup.node.ts`)에서 실행한다. 렌더·`renderHook`·`window` 등 DOM이 필요한 테스트는 `.test.tsx`로 둔다. 커버리지는 두 프로젝트를 한 번에 측정한다. UI 테스트(`apps/control-ui`)는 실제 시간을 기다리지 않는다. 제품 코드의 폴링·재확인 타이머(노드 제어 1초, 런타임 연결 2초 등)에 의존하는 테스트는 `vi.useFakeTimers()`와 `src/test-support/fakeTimerQueries.ts`의 `find`/`findIn`/`eventually`/`advance`를 사용한다. Testing Library의 `findBy`/`waitFor`는 가짜 타이머에서 끝나지 않으므로 이 도우미가 가짜 시계를 넘기며 확인한다. 화면 전체(App·StudioScreen·RoutingWorkspace)를 렌더하는 테스트는 `src/test-support/integration`을 import해 통합 테스트 제한 시간(15초)을 쓴다. 경계 검사가 이 import를 강제한다. 제한 시간은 멈춤 방지용이며 속도는 따로 측정한다. Storybook 스토리의 axe 검사는 `test:a11y`로 일반 테스트와 분리해 실행한다.

디자인 시스템 계약 도구는 `docs:api <source> <Name>`(계약 API 발췌 출력)과 `docs:page <Name> [--force|--stdout]`(표준 컴포넌트 문서 생성)이다. 둘 다 `--help`로 사용법을 확인한다.

브라우저 개발 화면(`pnpm --filter @mixrelay/control-ui dev`)은 엔진에 바로 닿지 못한다. 엔진과 Driver는 사용자 소유 Unix 소켓만 열기 때문이다. 실제 엔진이 필요하면 `MIXRELAY_DEV_ENGINE_BRIDGE=1`로 실행해 loopback dev 서버가 `/engine` 요청을 엔진 소켓으로 전달하게 한다. 켜 두는 동안 dev 서버에 닿는 다른 계정도 개발자 권한으로 엔진을 조작할 수 있으므로 필요할 때만 켠다. 시험과 lab은 `MIXRELAY_IPC_DIR`로 격리된 소켓 디렉터리를 쓰고, 설치된 세션 소켓을 건드리지 않는다.

Rust 코드는 [Rust 작업 규칙](rust-guidelines.md)을, 크레이트 책임과 의존 방향은 [코드 구조와 책임](architecture.md)을 따른다. `pnpm check:crate-layers`가 의존 방향을 검사한다.

설치 환경의 제약은 로컬 테스트의 기본값으로 둔다. 설치본에서만 드러나는 문제를 설치·확인을 반복하며 찾지 않기 위해서다.
- HAL은 coreaudiod 샌드박스(`deny default`, `process-info-pidinfo` 없음) 안에서 돈다. 다른 프로세스의 `proc_pidinfo`·`proc_pidpath`·번들 조회는 실패한다. HAL의 프로세스 조회는 `transport::ProcessInspection` 하나로만 하고, Registry 테스트 픽스처는 `SANDBOXED`를 쓴다. 신원 판단은 coreaudiod가 `AddDeviceClient`로 준 PID·번들처럼 샌드박스 안에서 얻는 정보로도 성립해야 한다.
- 엔진은 자기가 발행하는 가상 장치에 직접 쓰는 클라이언트다. 장치 쓰기로 동작이 바뀌는 판단(지연 보고 등)은 엔진 자신의 쓰기를 포함한 상황을 테스트한다.
- 서로 다른 장치를 잇는 경로는 클럭이 달라 엔진이 리샘플링한다. 비트 일치를 요구하는 왕복 측정은 같은 클럭 경로에서만 쓰고, 그 밖에는 `pnpm probe:macos-roundtrip --impulse`로 잰다.
- 엔진은 root 소유 launchd 데몬이고 HAL·앱 캡처가 고정 XPC 이름 `io.mixrelay.audio.engine`으로 연결하므로, 로컬 빌드 엔진으로 설치본 HAL을 대체할 수 없다. 설치본 확인은 위 테스트를 통과한 변경의 최종 확인으로 한 번에 묶는다.
