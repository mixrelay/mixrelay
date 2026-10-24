# Linux 설치본 E2E: 86d50c75

후보 `86d50c7530c87ab45c5304a9a81c1bde04e4ae50`, Linux 빌드
`40047005724`, 네 번째 설치본 실행
[40048258432](https://github.com/mixrelay/mixrelay/actions/runs/40048258432)의 조사 기록이다.
수정은 `0db9d7e1` 기반 `codex/e2e-r4-linux` 작업 트리에 미커밋 상태로 남겼다.
패키지를 다시 만들거나 워크플로를 실행하지 않았다.

## 수집한 설치본 결과

원시는 `/tmp/mr-e2e-r4/art/installed-target-<target>/`에 있다.
아래의 성공은 수정 전 후보에 대한 결과다.

| 대상 | fresh | upgrade |
| --- | --- | --- |
| Debian 13 x64, Kali rolling x64, Ubuntu 26.04 x64, Arch rolling x64 | 전체 통과 | 전체 통과 |
| Debian 13 ARM64 | APP-01·REC-02·PKG-02 통과. GRAPH-01은 Add node 탐색 실패. 연결 오류 화면이 보인다. | APP-01·PKG-02 통과. GRAPH-01은 Recorder 탐색 실패. Compressor 노드와 Output 하위 메뉴가 보인다. AUDIO-01·REC-02는 차단됐다. |
| Kali rolling ARM64 | APP-01·REC-02·PKG-02 통과. GRAPH-01은 Compressor 탐색 실패. | APP-01·REC-02·PKG-02 통과. GRAPH-01은 Compressor 탐색 실패. |
| Ubuntu 26.04 ARM64 | 작업 실행 중, 결과 아티팩트 미공개 | 결과 미수집 |

Debian과 Kali의 `fresh/installed-ui-failure.png`, `upgrade/installed-ui-failure.png`를
직접 확인했다. Debian upgrade를 연결 오류로 분류하지 않는다. Kali fresh는
Add node 메뉴만, upgrade는 Compressor가 보이는 Processor 하위 메뉴까지 열려 있다.
Kali upgrade의 실패 덤프에도 `menu item Compressor`가 있다. 기존 덤프에는
SHOWING 상태와 조회 오류가 기록되지 않아 이름이 존재해도 탐색을 거부한 이유를 알 수 없다.

## 확인된 연결 복구 결함

`debian-13-arm64/fresh/app-ready.json`은 엔진의 `driverState=ready`,
`enginePower.enabled=true`, Driver의 `state=ready`를 보존한다. 같은 모드의
`installed-ui-failure.png`는 `Cannot connect to MixRelay`와 `Retry connection`을
표시한다. `GRAPH-01.failure.json`은 Add node 탐색 실패이며,
`graph-reopen.json`과 후속 AUDIO-01·REC-02는 엔진 API가 이후에도 작동했음을 보여 준다.

UI의 [`useRuntimeConnection`](../../apps/control-ui/src/features/runtime-connection/useRuntimeConnection.ts)은
최초 연결의 `unavailable` 응답이나 예외를 오류 화면으로 바꾼다. 수정 전에는
`state === 'ready'` 또는 `opened && state === 'unavailable'`일 때만 상태를 확인했다.
최초 실패의 `opened=false`에서는 `port.status()`를 한 번도 호출하지 않으므로
엔진이 나중에 준비돼도 수동 Retry 전에는 작업 화면을 열 수 없었다.
`unavailable` 응답과 연결 예외 각각을 주입한 회귀 테스트가 수정 전 같은 이유로 실패했다.
증거는 `/tmp/mr-e2e-r4/ui-recovery-before.log`다.

네이티브 소유자는
[`background/startup.rs`](../../apps/desktop/src-tauri/src/background/startup.rs)의
명시적 연결과 읽기 전용 상태 확인이다. `connect_until_ready`는 최대 30초 동안
호환되는 컨트롤러의 hello와 엔진 상태 응답을 확인한다. 각 연결 교환은
[`background.rs`](../../apps/desktop/src-tauri/src/background.rs)의 3초 한도를 쓴다.
`runtime_status`는 같은 준비 상태를 읽으며 프로세스를 실행하지 않는다.
UI가 이 읽기 경로를 계속 사용하는 것이 복구의 소유 경계다.

**이번 설치본의 최초 연결 실패 원인은 확정하지 못했다.** `desktop.log`에는
엔진 경고와 GTK 표준 오류가 있지만, 편집기의 연결 진단은
`~/.local/state/mixrelay/logs/desktop-<pid>.log`에 따로 기록된다.
기존 Linux 수집기는 이 파일을 복사하지 않았다. 스택 수집도 시간 초과되어
정지 지점을 제공하지 않는다. `process-memory.txt`의 WebKit CPU 84.9%와
TCG 게스트라는 사실만으로 30초 또는 3초 시간 초과, 소켓 권한 경쟁을 단정할 수 없다.
설치본에서 최초부터 실패했는지, 열렸던 편집기가 나중에 연결을 잃었는지도
이 로그만으로 구분되지 않는다. 확인된 복구 결함과 설치본의 최초 실패 계기를 구별한다.

## 수정

- 최초 연결이 `unavailable`이어도 기존 2초 간격으로 `RuntimePort.status()`를 읽는다.
  준비가 확인되면 작업 화면을 처음 마운트한다. 이후 연결 단절 시에는 기존 편집 상태를 보존한다.
- 자동 확인은 `connect()`를 다시 호출하지 않는다. 사용자가 멈춘 오디오를 다시 시작하지 않는다.
  설치 누락·비호환은 자동 확인하지 않으며 수동 Retry와 설치·업데이트 안내를 유지한다.
- Feature와 실제 `App/Workspace`에 `StartupRecovery` Storybook 상태를 추가했다.
  문구·레이아웃·디자인 토큰은 바꾸지 않았다.
- Linux 진단에 `desktop-*.log`, `driver-runtime-*.log` 복사를 추가했다.
  심볼릭 링크를 따라가지 않고 기존 증거를 덮어쓰지 않는다.
- AT-SPI 실패 시 기존 트리와 스크린샷을 보존한 뒤 캐시를 비운
  `installed-ui-a11y-refreshed.txt`를 추가로 수집한다. 두 실패 덤프에 SHOWING 상태,
  자식 수, null 자식, 자식 수·인덱스·설명 조회 예외를 표시한다.
  탐색·클릭·가시성 조건·제한 시간은 바꾸지 않았다.

## AT-SPI 조사와 한계

Debian fresh의 트리는 웹 문서 아래 이름 있는 landmark에서 끝난다. 깊이는 약 9로,
검사기의 깊이 60 제한에 걸린 결과가 아니다. 오류 화면 소스에는 제목, alert, 실제 Retry
버튼이 있으며 `hidden`·`inert`는 이미 열었던 작업 화면의 별도 형제에만 적용된다.
Debian upgrade도 화면의 Recorder가 덤프에서 빠진다. 기존 검사기는 조회 예외와
null 프록시를 버리므로 이 덤프만으로 WebKit의 노출 누락과 클라이언트 조회 실패를
구분할 수 없다.

오디오를 실행하지 않는 별도 Debian 13 ARM64 컨테이너에서 빌드된 Storybook을
GTK WebKit으로 열었다. 설치본과 같은 libatspi `2.56.2-1+deb13u2`,
pyatspi `2.46.1-1`, WebKitGTK `2.54.0-1~deb13u3`를 사용했다.
실제 오류 컴포넌트와 `App/Workspace/StartupRecovery`에서 제목·Retry 버튼이
AT-SPI에 노출됐고 자동 복구 뒤 작업 화면의 Add node와 노드 조작도 트리에 나타났다.
오류 설명 전체도 AT-SPI Text 인터페이스로 읽었다.
오류 화면과 복구된 작업 화면의 실제 렌더링을 직접 확인했다.

이 확인은 포트를 주입한 UI 검증이며 설치된 Tauri·정상 로그인 세션·TCG 환경의 검증은 아니다.
동적으로 자식을 추가한 작은 WebKit 화면에서도 이벤트 루프 변경 없이 자식이 노출됐다.
따라서 GLib 이벤트 처리 누락이라는 가설을 해결책으로 적용하지 않았다.
추가 UI 검사는 D-Bus activation 환경에 DISPLAY·XAUTHORITY를 전달한 Openbox 세션에서
원래 `linux-ui.main`의 Compressor·Recorder 추가, 채널 연결, 케이블 이름 확인을 통과했다.
호스트 이름이 다른 테스트용 GTK 창을 선택하는 경계만 바꿨으며 탐색·클릭·제한 시간은 유지했다.
초기 보조 진단의 실패는 입력 버스 환경 누락과 진단 래퍼의 검색 옵션 누락을 포함하므로
제품 재현으로 분류하지 않는다. 이 UI 검사를 설치본 GRAPH-01의 성공으로 보고하지 않는다.
호스티드 환경의 접근성 누락 원인과 실제 스크린 리더 읽기는 미확인이다.

시각·AT-SPI 증거:

- `/tmp/mr-e2e-r4/atspi-story.log`, `storybook-unavailable.png`
- `/tmp/mr-e2e-r4/atspi-story-text.log`
- `/tmp/mr-e2e-r4/atspi-app-recovery.log`, `app-startup-unavailable.png`, `app-startup-recovered.png`
- `/tmp/mr-e2e-r4/atspi-cache-ab.log`, `atspi-menu-unmodified.log`, `storybook-graph-menu.png`

## 검사

| 명령 | 결과 |
| --- | --- |
| `pnpm --filter @mixrelay/control-ui check` | 통과. UI 테스트 2,202개, 접근성 테스트 243개, 커버리지 100%, 타입·경계·앱/Storybook 빌드 통과 |
| `pnpm check:ui-quality` | 통과 |
| `pnpm --filter @mixrelay/control-ui typecheck` 및 `build-storybook` | 실제 App 복구 스토리 추가 후 통과 |
| `node scripts/lab/native-check.mjs cargo test -p mixrelay-desktop --bin mixrelay-desktop background::startup::tests` | 6개 통과. 느린 엔진 준비, 중복 실행 방지, 명시적 실행의 재시도 한도 확인 |
| `node scripts/lab/native-check.mjs cargo clippy -p mixrelay-desktop --all-targets -- -D warnings` | macOS 호스트에서 통과 |
| `node scripts/lab/native-check.mjs cargo fmt --all --check` | 통과 |
| `node --test scripts/e2e/*.test.mjs` | 98개 통과. 로그 보존과 AT-SPI 진단 회귀 포함 |
| `git diff --check` | 통과 |

검사 로그는 `/tmp/mr-e2e-r4/`에 있다. Rust 제품 코드는 수정하지 않았다.
수정된 패키지의 Linux ARM64 fresh·upgrade와 호스티드 AT-SPI 문제는 아직 검증하지 않았다.
Google Chrome의 CUA 접근은 자동 승인 검토가 거절했다. 이유는 도구가 제공하지 않았다.
시각 확인은 위의 격리된 GTK WebKit 렌더링으로 수행했다.
문서는 기술 기록용 `docs` 설정으로 근거·확신 수준·문장을 최종 교정했다.
