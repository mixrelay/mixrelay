# Linux 설치 E2E 3차 실패 조사

대상 실행은 [40034216825](https://github.com/mixrelay/mixrelay/actions/runs/40034216825),
후보와 suite 소스는 `05f7ac86bb5ae6e25dae95a7e7fb7bb06e9e7b22`다.
Baseline은 `8582d158455c56006e4288146dcc646100c34bb5`, Linux 빌드는 `39939576577`이다.
아래 증거의 공통 루트는 `/tmp/mr-e2e-r3/art/installed-target-`다.

| 실패 | 원인과 분류 | 수정 | 증거 경로(공통 루트 기준) |
| --- | --- | --- | --- |
| Arch UPGRADE-01 / 정리 PKG-02 | 첫 오류는 baseline workspace 저장의 HTTP 409 `graph_exchange_busy`, `workspaceOutcome=not_applied`다. 도구가 제출 revision만 확인하고 활성화를 기다리지 않았다. 이후 정리 중 **baseline** pacman 훅이 실행 중인 앱을 거부했다. 새 후보의 업그레이드 설치는 시작되지 않았다. | 저장 요청 전 활성 revision을 기다린다. 별도로 제품 훅은 편집기·컨트롤러에 정상 종료를 요청하고 엔진 drain 뒤 Driver를 중지한다. Linux SIGTERM도 기존의 정상 종료 경로로 전달한다. | `arch-rolling-x64/upgrade-report.json`, `upgrade/removal.log`, `upgrade/baseline/installed-binaries.json`, `upgrade/process-memory.txt` |
| Kali x64 upgrade GRAPH-01, Debian/Kali ARM64 fresh GRAPH-01 | AT-SPI 자식에 `None`이 들어오는데 탐색·덤프가 곧바로 `getRoleName()`을 호출했다(테스트 도구). Kali x64는 노드 연결 뒤 덤프에서, Debian ARM64는 Add node 탐색에서, Kali ARM64는 Compressor 탐색에서 실패했다. | null 자식, 열거 중 사라진 자식·프록시를 개별 처리한다. 유효한 형제 탐색과 엄격한 제어 이름·가시성 검증을 유지한다. 최초 예외와 진단 오류를 각각 기록한다. | `kali-rolling-x64/upgrade/installed-ui.log`, `debian-13-arm64/fresh/installed-ui.log`, `kali-rolling-arm64/fresh/installed-ui.log` |
| Debian ARM64 REC-02 | 녹음 중지 명령의 writer admission이 250ms 내에 잠금을 얻지 못했다. 텔레메트리 읽기가 Driver IPC 동안 같은 mutation 잠금을 보유하는 제품 결함이 있다. Driver 읽기는 최대 5초를 기다릴 수 있다. | Driver 상태 조회 전 mutation 잠금을 놓는다. 조회 후 다시 잠금을 얻어 revision이 같은 경우에만 스냅샷을 발행한다. | `debian-13-arm64/fresh/REC-02.failure.json`, `fresh/desktop.log`, `fresh/debian-13-arm64.mode.json` |
| Debian/Kali ARM64 fresh PKG-02 | apt 의존성 해석은 성공했다. 실패 지점은 `mix-relay`의 **prerm**이 실행 중인 편집기를 발견한 단계다. 그 전에 진단용 `sudo gdb`가 시간 초과돼 편집기를 추적 정지 상태로 남겼다(테스트 도구). | 수집기를 별도 프로세스 그룹으로 실행하고, 제한 시간에 GDB를 회수한 뒤 자신이 멈춘 동일 PID incarnation만 재개한다. 기존에 정지됐거나 다른 디버거가 추적 중인 프로세스는 보존한다. | `debian-13-arm64/fresh/removal.log`, `fresh/mixrelay-desktop-5983.stack.txt`, `upgrade/process-memory.txt`; `kali-rolling-arm64/fresh/removal.log`, `fresh/mixrelay-desktop-10537.stack.txt`, `upgrade/process-memory.txt` |

Debian의 mutation이 UI 도구 종료 뒤 영구히 남았다는 증거는 없다. GRAPH-01 실패 후
GRAPH-02 저장·재열기와 녹음 시작, AUDIO-01이 성공했다. REC-02 실패 후 그래프 복원도
실패로 기록되지 않았고, cleanupError는 `Installed app did not stop` 하나다.
따라서 관찰된 오류는 영구 잠금보다 일시 경합에 부합한다. 텔레메트리의 잠금 점유는
주입된 Driver 조회 경계에서 재현하며, 설치본의 당시 잠금 소유자를 심볼로 확인한 것은 아니다.

Debian의 `fresh/process-memory.txt`에서 편집기 5983은 `Sl`, GDB 시간 초과 후
`upgrade/process-memory.txt`에서는 `Tl`이다. Kali 편집기 10537도 같은 전이를 보인다.
SIGTERM은 정지된 편집기를 실행시키지 못한다. 제거 훅의 메시지를 apt 자체 오류나
앱 충돌의 증거로 해석해서는 안 된다. Debian의 실패 스크린샷도 실제로 열어 확인했으며,
빈 편집 화면이 렌더링돼 있다. Kali/Ubuntu의 실패 화면 이미지는 확인하지 못했다.

Baseline의 실행 중 앱 거부 훅은 `05f7ac86`에도 남아 있었다. 이 Linux 결함은
`55b5c8e5`에서 이미 수정된 HAL factory UUID 충돌과 구분한다. `8582d15`는 그 HAL 수정과
`d84f805e`의 Linux libdbus 심볼 격리 수정도 포함하지 않는다. 다음 후보의 baseline은
이 두 수정과 이번 Linux 종료 훅을 포함해 빌드·보존한 **선행 후보**로 정해야 한다.
현재 파일 중 `05f7ac86`은 두 기존 수정은 포함하지만 Linux 종료 훅 수정은 포함하지 않는다.
기존 baseline 아티팩트를 바꾸거나 baseline 훅을 도구로 덮어쓰지 않는다.
특히 Arch는 교체 전 설치된 훅을 실행하므로 새 패키지만으로 과거 훅의 실행 중 거부를
소급 해결할 수 없다. 과거 baseline에서의 실행 중 업그레이드 성공은 확인되지 않았다.

Ubuntu ARM64 작업 `120690112672`도 실패로 완료됐다. 아티팩트를
`/tmp/mr-e2e-r3/art/installed-target-ubuntu-26-04-arm64/`로 추가 수집했고,
원시 로그는 `/tmp/mr-e2e-r3/logs/120690112672.log`에 저장했다.
APP-01의 마지막 관측은 `installed_not_loaded`, 후속 `fresh/readiness-disagreement.json`에서는
엔진·Driver 모두 ready다. GRAPH-01은 `Missing installed UI control: Add node`이며
진단 덤프도 AT-SPI 응답 대기 오류로 실패했다. 이 두 오류를 null 자식 오류로 단정하지 않는다.
또 녹음 시작에서 동일한 mutation 409가 발생했고, GDB 시간 초과 뒤 편집기 8610이 `Tl`로
남아 prerm도 실패했다. APP-01과 Add node 탐색 실패의 설치 환경 재현은 남아 있다.
시간 제한과 성공 조건은 완화하지 않았다.

수정 검증은 소스 테스트와 패키지 훅 fixture로 수행한다. 원래 walker·훅·workspace 도구에서
각 회귀가 실패함을 확인했다. 실제 Linux 패키지 재빌드·재설치, 수정 후 hosted E2E,
실제 녹음 중 패키지 업그레이드는 이 조사에서 실행하지 않는다.

검증 결과는 다음과 같다. 실행 호스트는 macOS이며 Linux 전용 컴파일·실행 결과와 구분한다.

- 관련 Node 스크립트 테스트: 129개 통과(`linux-fix-script-tests.log`).
- `cargo test -p mixrelay-desktop --bins`: 226개 통과(`desktop-tests.log`).
- `cargo test -p mixrelay-engine-service --lib`: 341개 통과, 기존 벤치마크 1개 ignored(`linux-fix-engine-lib.log`).
- 같은 엔진 크레이트의 `--lib --bins` 전체 실행: 라이브러리는 통과했고 macOS 바이너리는 192개 통과, 기존 4개 ignored, 2개 실패(`engine-tests.log`). 실패는 `an_application_with_a_later_ready_frontier_joins_a_running_native_input`의 `Operation not permitted`와 `native_callback_and_hal_contracts`의 shared mapping attach assertion이다. 원본 `HEAD`의 텔레메트리 코드를 복원한 대조 실행에서도 각각 재현됐다(`baseline-*.log`).
- 두 크레이트 `clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, 지원 계약 검사와 크레이트 계층 검사 통과.
- 기존 walker·실행 중 거부 훅·workspace 저장 순서·텔레메트리 잠금에서 새 회귀 테스트가 실패하는 것을 확인한 뒤 수정본을 복원했다(`ui-before-fix.log`, `hook-before-fix.log`, `workspace-before-fix.log`, `telemetry-before-fix.log`).

위 검사 로그의 공통 루트는 `/tmp/mr-e2e-r3/`다. 테스트의 기존 ignored 항목은 변경하지 않았다.
문서는 `docs` 목적에 맞춰 근거·수치·미확인 범위를 대조하고 한국어 본문을 교정했다.
