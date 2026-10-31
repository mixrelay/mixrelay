# MIDI 매핑 모드 소스 검증

2026-10-30, `/Volumes/dev/mixrelay-midi`의 기존 미커밋 MIDI 작업 위에서 검사했다.
아래 결과는 소스와 모의 제어 경계의 검증이며 설치본·물리 MIDI 장치의 수락 결과가 아니다.
동작 사양은 [product.md](../product.md#midi-컨트롤러), 참조 근거는
[Ableton 연구](../research/ableton-midi-map-mode.md)에 있다. 커밋·푸시·릴리스·서명 작업은 하지 않았다.

## 변경 파일

| 파일·디렉터리 | 변경 |
|---|---|
| `crates/midi-control/src/{lib,learn,runtime}.rs`와 인접 테스트 | 입력+대상+동작 식별, 정확한 중복 유지, 연속 캡처, 매핑별 범위·상태 |
| `apps/desktop/src-tauri/src/midi.rs`, `midi/{store,worker,native}.rs`와 테스트 | 모드와 대상 선택, 단일 매핑 편집·삭제, 다중 대상 발행, 이름 영속화, 장치 재연결 |
| `apps/desktop/src-tauri/src/control_proxy/local/midi/` | 현재 대상 상태로 토글, revision·capability·누락 대상 검증 |
| `apps/desktop/src-tauri/src/{app_menu,workspace_menu}.rs`, `app_menu/` 로케일 | 네이티브 Cmd/Ctrl+M 전달, macOS 최소화 단축키 해제, 11개 언어 메뉴 |
| `apps/control-ui/src/core/midi/`, `features/midi/` | 공통 매핑 문맥·IPC, MIDI 토글·단축키, 목록·선택·옵션·삭제·포커스, 상태별 Storybook |
| `apps/control-ui/src/core/ui/{AudioNodeCard,NodeSignalControls,TransportButton,NodeOptionsSection}.tsx`와 CSS | 실제 조작 위치의 매핑 버튼·레이블·선택 표시, 설정 비활성화 |
| `apps/control-ui/src/core/{audio-graph,theme,design-system}/` | 그래프 편집 차단, 매핑 토큰, Core 계약·문서의 실제 매핑 예시 |
| `apps/control-ui/src/features/routing-workspace/model.ts`, `RoutingWorkspace.tsx` 및 인접 파일 | 새 노드 UUID, 그래프·노드 문맥, 매핑 중 변경 차단 |
| `apps/control-ui/src/screens/StudioScreen.tsx`, `core/control-port/{types,StorybookMockControlPort}.ts` | 그래프 삭제 실행 취소·다시 실행, 선택한 매핑의 그래프 이동 |
| `apps/engine-service/src/graph_catalog{.rs,/deletion_history.rs,/deletion_history/}`와 테스트 | 최대 100개 그래프 삭제 이력, 원래 그래프·노드 ID·배치 복원, 변경된 빈 대체 그래프 보존 |
| `apps/engine-service/src/{node_control_snapshot,graph_layout_tests}.rs` | 노드 표시 이름을 제어 스냅샷으로 전달 |
| `crates/control-protocol/src/lib.rs` | 그래프 삭제 실행 취소·다시 실행 명령 |
| `apps/control-ui/src/core/i18n/locales/` | 11개 언어와 pseudo의 매핑·삭제 안내 |
| `apps/website/manual/*/{midi,workspace}.html`, `apps/website/check-site.mjs` | 11개 언어의 모드·공유 입력·옵션·누락·복원 안내 및 계약 검사 |
| `docs/{product,development,design-system}.md`, `docs/research/ableton-midi-map-mode.md` | 동작·랩 절차·디자인 계약·공식 참조 |

## 보장과 회귀 테스트

| 보장 | 소유 경계·테스트 |
|---|---|
| 정확한 중복의 옵션 유지, 같은 입력의 여러 대상 | MIDI 크레이트 `tests.rs`, 데스크톱 `midi/store/tests.rs`의 영속화 왕복 |
| 대상 선택 중 최근 입력 교체, 기존 중복 보존, 동작 편집 뒤 캡처 유지 | `midi/tests.rs`의 모드·캡처·중복 회귀 |
| 매핑별 pick-up·상대 단위, 대상별 34ms 제한 | `midi/worker/tests.rs`, MIDI 크레이트 `runtime/tests.rs` |
| 누름 순서와 다중 대상 실행 순서, 제거 후 배출 배치 무효화, 모드 중 실행 차단 | `midi/worker/tests.rs`의 fan-out·press order·drained batch 회귀 |
| 대상별 실제 상태 토글, 노드·전체 그래프 누락 시 실행 없음, 같은 ID 복원 뒤 재개 | `control_proxy/local/midi/tests.rs` |
| 마지막 이름 유지와 갱신, 내부 ID 없는 누락 목록 | `midi/tests.rs`, `graph_layout_tests.rs`, `MidiControls.test.tsx` |
| 재시작 뒤 ID·옵션 유지, 장치 주소가 바뀐 재연결 | `midi/store/tests.rs`, `midi/native/tests.rs`, `graph_catalog_tests/removal.rs` |
| 노드 삭제 undo/redo의 ID 유지와 실제 그래프 적용 | `features/routing-workspace/nodeIdentity.test.tsx`의 history → GraphApplicationQueue → 제어 포트 경계 |
| 전체 그래프 삭제 undo/redo의 ID·배치·영속화 | `graph_catalog/deletion_history/tests.rs`, `graph_catalog_tests/removal.rs`, `app/App.midi.test.tsx` |
| 실제 노드 매핑·그래프 편집 차단·단축키·포커스·단일 매핑 옵션/삭제 | `app/App.midi.test.tsx`, `features/midi/MidiControls.test.tsx`, `useMidiGraphSelection.test.tsx`, `useMidiSession.test.tsx` |

## 실행 결과

Rust 명령에는 `RUSTC_WRAPPER=`를 붙였다. 엔진 테스트·워크스페이스 clippy는
`node scripts/lab/native-check.mjs`로 네이티브 fixture를 준비했다.

| 명령 | 결과 |
|---|---|
| `cargo llvm-cov -p mixrelay-midi-control --lib --show-missing-lines --fail-under-functions 100 --fail-under-lines 100 --fail-under-regions 100 --fail-uncovered-functions 0 --fail-uncovered-lines 0 --fail-uncovered-regions 0` | 25개 통과, 368줄·586영역·53함수 모두 100% |
| `node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --lib graph_catalog` | 93개 통과 |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop midi::` | 32개 통과, local MIDI dispatch 포함 |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop control_proxy::` | 87개 통과 |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop lifecycle` | 16개 통과 |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop workspace_menu::` | 1개 통과 |
| `cargo test -p mixrelay-control-protocol --lib` | 27개 통과 |
| `node scripts/lab/native-check.mjs cargo clippy --workspace --all-targets --all-features -- -D warnings` | 통과 |
| `cargo fmt --all --check` | 통과 |
| `pnpm --filter @mixrelay/control-ui check` | 동작 2,282개·접근성 243개 통과, 줄·분기·함수·문장 100%, 앱·Storybook 빌드와 69개 Core 계약 통과 |
| `pnpm check:ui-quality` | 608개 파일 통과 |
| `pnpm support:check` | 통과 |
| `node apps/website/check-site.mjs` | 183페이지 통과 |
| `git diff --check` | 통과 |

초기 직접 엔진 테스트는 네이티브 fixture 부재로 실패했고 준비 wrapper로 다시 실행해 통과했다.
중간 타입·커버리지·clippy 실패는 수정 후 같은 검사를 통과했다. 기존 게이트는 낮추지 않았다.
문서는 docs 목적·technical 문체로 최종 교정했다. 영어 매뉴얼 검출기의 문장 리듬 경고는
절차 문단의 구조에 따른 것으로 유지했고, 따옴표 정규화는 변경 없었다. 제품 문서 보존 검사는 통과했으며
숫자 변경 경고는 시간 제한 삭제·100개 삭제 이력 추가라는 요구 범위와 대조했다.

## 직접 확인한 범위와 미확인 범위

Ableton 공식 MIDI 버튼·Mapping Browser·Scene Selector의 지정 완료 이미지를 직접 열어 확인했다.
설치된 Live와 Chrome은 컴퓨터 도구가 `Computer Use was not approved`로 접근을 거부했고,
연결된 브라우저도 없었다. Storybook 개발 서버와 정적 빌드는 실행했으나 새 스토리의 실제 렌더링 화면을
직접 보지 못했다. DOM·접근성·커버리지 통과를 시각 검수로 판정하지 않는다.

물리 MIDI 장치, 실제 Quit/relaunch, macOS 네이티브 Cmd+M과 최소화 메뉴,
Linux ALSA·Ctrl+M, 설치본·스크린리더는 이 세션에서 확인하지 못했다.

관리자 확인 사항은 볼륨 Min/Max의 반전 허용, 새 노드 UUID,
그래프 삭제 이력의 실행부 수명·100개 제한, macOS Cmd+M의 MIDI 예약이다.
가져오기는 저장 ID를 보존하며 같은 그래프·노드 ID면 기존 매핑이 연결된다.
붙여넣기·그래프 복제는 현재 제품에 없다. 통합 전 실제 사용처와 요청된 모든 Storybook 상태의
시각·포인터·키보드 검수가 필요하다.

## 단일 버튼·사이드바·실제 작업 화면 스토리 후속 수정

헤더의 별도 목록 버튼과 모드 밖 열기 상태를 제거했다. `MidiAccess`의 패널을
`MidiPanelContext`로 `StudioFrame`에 전달하고 왼쪽 그래프 목록 자리에서만 표시한다.
모드 종료 뒤에는 기존 그래프 목록 표시 상태를 복원한다. 좁은 창의 패널도 기존 사이드바 영역을
사용하고 내용을 내부 스크롤한다. 그래프 목록 안의 조작에 포커스를 두고 단축키를 실행해도 MIDI 토글로 옮긴다.
옵션·상태·행 선택과 대상 강조·매핑 하나 삭제·Escape·네이티브 단축키는 유지한다.

`Features/MIDI/Controls`의 소스는 `app/MidiControls.stories.tsx`로 옮겼다.
기존 독립 카드·중첩 iframe 조합을 없애고 `App/Workspace`의 실제 App·포트·그래프 배치를
재사용한다. 매핑 fixture의 대상도 실제 `graph-1`의 `trim`·`app`에 연결했다.
스토리마다 포트를 새로 만들며 키보드 스토리는 패널의 옵션에 포커스를 남긴다.
Core 문서 다섯 곳의 import는 새 스토리 경로를 따른다.
제품 문서·Ableton 연구의 채택/조정 항목·11개 언어 매뉴얼·번역의 빈 상태 안내를 갱신하고
쓰이지 않는 MIDI 로딩 문자열을 제거했다. Rust는 이번 후속 수정에서 변경하지 않았다.

최종 `pnpm --filter @mixrelay/control-ui check`는 동작 2,283개·접근성 243개,
네 가지 커버리지 100%, 앱 및 Storybook 빌드와 69개 Core 계약을 통과했다.
`pnpm check:ui-quality`, `node apps/website/check-site.mjs`, `pnpm storybook:build`와
`git diff --check`도 통과했다. 처음 추가한 포커스 시험은 비활성 버튼에 포커스를 주어 실패했고,
활성화 확인을 추가했다. 좁은 폭 시험의 media query mock이 다음 시험에 남던 문제도 복원 처리로 수정했다.

### 화면 확인과 캡처 제한

이전 리뷰의 `app-workspace--midi-controls.png`와 좁은 카드 스토리 PNG,
Ableton의 MIDI 버튼·Mapping Browser 이미지를 직접 열어 원래 문제와 참조 형태를 확인했다.
이는 수정 후 화면의 확인 결과가 아니다.

정적 Storybook 서버는 `python3 -m http.server 6108 --bind 127.0.0.1 --directory apps/control-ui/storybook-static`으로
실행했다. 요청된 Chromium 실행 파일로 아래 10개 스토리를 각각 캡처 시도했으나 모두
`bootstrap_check_in … MachPortRendezvousServer … Permission denied (1100)`로 시작 전에 종료했다.
`--no-sandbox --disable-gpu` 재시도도 같은 제한에 걸렸다. PNG는 생성되지 않았으며,
각 스토리의 폭·종료 코드·원시 오류는 `target/midi-story-shots/capture-results.json`에 보존했다.
연결된 브라우저는 없고 Chrome의 컴퓨터 접근도 도구가 거부했다.
따라서 수정 후 실제 사용처·Storybook의 시각 검수와 포인터·키보드 상태 확인은 미완료다.

관리자가 캡처할 스토리 ID는 아래와 같다. 기본 폭은 1280, 높이는 860이며 좁은 스토리만 폭 420이다.
주소는 `http://127.0.0.1:6108/iframe.html?id=<ID>&viewMode=story`다.

- `app-workspace--midi-controls`
- `features-midi-controls--map-mode-off`
- `features-midi-controls--map-mode-on`
- `features-midi-controls--selected-target-waiting`
- `features-midi-controls--mapped-controls`
- `features-midi-controls--one-control-several-targets`
- `features-midi-controls--target-missing`
- `features-midi-controls--midi-unavailable`
- `features-midi-controls--narrow-long-translation`
- `features-midi-controls--keyboard`

이번 후속 수정에서 관리자가 확인할 결정은 모드 종료 시 사이드바의 기존 열림/닫힘 상태를
복원하고 좁은 창에서도 모드 중에는 매핑 패널을 표시하는 것이다. 설치본·실제 MIDI·보조 기술의
미확인 범위는 앞 절과 같다.
문서는 docs 목적·technical 문체로 최종 교정했다. 영어 매뉴얼 검출 점수는 0이며,
기존 제목의 em dash 경고는 유지했다. 변경 산문의 따옴표 정규화는 변경 없었고,
제품 문서와 영어 매뉴얼의 변경 전후 보존 검사는 모두 통과했다.

## 2차 리뷰 회귀와 매핑 모드 UI 수정

기존 미커밋 변경 위에서 아래 항목을 수정했다. 커밋·푸시·rebase·다른 worktree 변경은 하지 않았다.
제품 동작은 [product.md](../product.md#midi-컨트롤러)에 반영했으며 별도 backlog를 만들지 않았다.

### 파일별 변경

| 파일 | 변경 |
|---|---|
| `features/routing-workspace/model.ts`, `model.test.ts`, `endpointSettings.test.ts`, `virtualEndpointModel.test.ts`, `useVirtualEndpointCreation.test.tsx` | 가상 runtime 노드도 UUID를 발급한다. 같은 장치의 다른 쪽만 기존 형제의 ID를 공유한다. 장치 자체의 ID·형식·meter 연결 검증은 유지했다. |
| `control_proxy/local/midi.rs`, `midi/tests.rs`, `midi/tests/dispatch_regressions.rs` | 실제 Unix 소켓 경계에서 Pending 수락 뒤 활성 revision을 기다린다. 스냅샷 후 admission에서 세대를 다시 확인한다. 수락된 쓰기는 한 번만 완료하며 활성화 대기는 취소할 수 있다. |
| `src/midi.rs`, `midi/admission.rs`, `midi/worker.rs`, `midi/tests.rs`, `background/dispatch.rs` | 매핑 명령과 MIDI 쓰기를 비동기 gate로 직렬화한다. 스냅샷·활성화 대기는 종료·매핑 변경을 확인한다. 읽기 실패한 설정의 학습 진입과 캡처 저장을 거부한다. |
| `crates/midi-control/src/mailbox.rs`, `mailbox/tests.rs` | 소비 cursor·게시 tail·셀 stamp가 있는 고정 용량 누름 큐를 사용한다. 모든 포트에 같은 수신 경계를 적용하고 이전 콜백이 게시 중이면 배출을 미룬다. 초과 시 최신 1,024개 유지와 콜백 무할당을 검증했다. |
| `src/app_menu.rs` | 프랑스어·독일어·스페인어·포르투갈어·이탈리아어 MIDI 메뉴와 locale별 assertion을 수정했다. |
| `graph_catalog_tests/removal.rs` | 비기본 위치·크기·별칭·viewport를 저장하고, 반복 undo/redo와 재시작 뒤 값·layout revision을 확인한다. |
| `features/midi/useMidiSession.ts`, `useMidiSession.test.tsx` | 이전 명령 응답과 변경 중 시작한 상태 응답을 무시한다. 모드·호출자 선택·busy·오류를 최신 요청만 갱신한다. |
| `features/midi/MidiBindingsList.tsx`, `MidiControls.test.tsx` | 입력별 그룹, 단일 제목 선택 버튼, 상태·선택 옵션·제거 순서를 검증한다. 하드웨어 정보는 그룹 제목에 한 번 표시한다. |
| `screens/StudioScreen.tsx`, `StudioScreen.test.tsx`, `features/routing-workspace/RoutingWorkspace.tsx` | MIDI를 편집 헤더 오른쪽 끝으로 옮긴다. 일반 chrome과 제목은 비활성화하고 native 그래프 목록 명령도 무시한다. 진입으로 사라지거나 비활성화된 조작의 포커스는 MIDI 토글로 옮긴다. |
| `core/audio-graph/AudioGraphCanvas.tsx`, `features/midi/MidiAccess.module.css` | 모드 중 캔버스 이동·확대를 차단하고 일반 조작의 비활성 표현을 적용한다. |
| `core/theme/ComponentTheme.module.css`, `core/ui/SystemControls.test.tsx` | 일반 aria-pressed 버튼의 중립 반전 채움을 Core에 정의하고 파란 매핑 대상과 분리한다. Core CSS·토큰을 적용한 DOM에서 배경·글자색의 구분을 검사한다. 이는 브라우저 시각 수락과 별도다. |
| `app/App.midi.test.tsx`, `app/App.midi.a11y.test.tsx` | 실제 App의 비활성 경계·제목 잠금·포커스 복귀, 공유 입력 그룹, 정방향·역방향 Tab 및 axe를 검사한다. |
| `app/App.stories.tsx`, `app/MidiControls.stories.tsx`, `core/ui/ActionButton.stories.tsx` | 실제 App 사용처의 비활성 상태를 확인하는 play와 다크·좁은 폭의 모드 밖 상태·Core pressed 예시를 추가했다. 기존 story ID는 유지했다. |
| `core/design-system/componentContracts.ts`, `components/ActionButton.mdx` | Core pressed 계약과 MIDI 사용례를 갱신했다. |
| `docs/{product,development,design-system}.md`, `docs/design/interaction-accessibility.md`, `docs/research/ableton-midi-map-mode.md` | 동작·배치·입력 그룹·비활성·포커스 계약을 갱신했다. |
| `apps/website/manual/*/midi.html` | 11개 언어에서 버튼 위치를 헤더 오른쪽 끝으로 고쳤다. 영어·한국어에 그룹·단일 제목 선택을 설명하고 영어에 비활성 조작을 명시했다. |

UI 경로는 `apps/control-ui/src/`, desktop 경로는 `apps/desktop/src-tauri/src/`,
엔진 경로는 `apps/engine-service/src/` 기준이다. `control_proxy/local/midi/tests.rs`와
`src/midi/tests.rs`는 서로 다른 테스트 파일이다.

### 실패를 먼저 확인한 근거

컴파일·환경·fixture 오류는 아래 실패 재현에 포함하지 않는다.

| 지적 | 수정 전 assertion 실패 |
|---|---|
| 1. 가상 ID 재사용 | `model.test.ts`의 입력·출력 두 경우 모두 `expected 'virtual-1' not to be 'virtual-1'`로 실패했다. |
| 2. Pending 다중 대상 | `pending_application_is_acknowledged_before_the_next_fan_out_target`가 실제 소켓 응답을 받아 `[true, false]`를 반환해 `[true, true]` assertion에 실패했다. 최종 테스트는 서로 다른 노드에 두 쓰기를 보낸다. |
| 3. 스냅샷 후 무효 쓰기 | `mapping_changes_during_snapshot_io_fence_mute_recording_and_gain_writes`에서 스냅샷 읽기 중 매핑을 바꾼 뒤 `obsolete Mute reached write boundary: PUT .../control`로 실패했다. 최종 테스트는 mute·record·gain과 map 진입·binding 제거의 여섯 경우를 확인한다. |
| 4. 배출 경계 | `presses_published_after_a_drain_boundary_wait_for_the_next_drain_in_order`가 `an empty drain cannot acquire later presses`로 실패했다. 경계 이후 누름은 다음 배출에서 원래 순서로 확인한다. 초과 중 배출과 포트 공통 경계도 검사한다. |
| 5. 늦은 UI 응답 | 실제 `useMidiSession` 훅에서 이전 learn이 선택을 true로 복원하거나 오류를 true로 만들었다. cancel 완료 뒤 learn 성공도 true를 반환했다. 변경 중 시작한 상태 읽기는 모드를 false로 되돌렸다. 모두 수정 후 통과했다. |
| 6. 읽기 실패 설정 | `learning_cannot_replace_unreadable_preferences`가 learn 진입에서 `Err(Preferences)`를 기대했으나 성공해 실패했다. 최종 테스트는 이미 준비된 캡처의 저장도 거부하고 원본 바이트가 유지되는지 확인한다. |
| 7. 메뉴 locale | `midi_menu_uses_each_locales_own_translation`이 fr-FR에서 독일어 `MIDI-Zuweisungsmodus`를 반환해 실패했다. 다섯 locale 모두 정확한 번역을 확인한다. |
| 8. 배치 검증 공백 | authored layout을 넣은 뒤 복원 코드를 기본 layout으로 바꾸는 통제된 변이에서 assertion이 빈 positions·sizes·labels·viewport를 잡아 실패했다. 변이는 제거했고 기존 복원 구현은 유지했다. |
| UI 비활성·그룹 | 실제 App에서 테마의 inert 경계 assertion, 목록에서 하드웨어 그룹 region assertion이 각각 실패했다. |
| UI pressed 구분 | 이전 MIDI-toggle CSS 선택자를 되살린 통제된 변이에서 토글·매핑 대상의 배경이 모두 `rgb(237, 245, 251)`이어서 구분 assertion이 실패했다. 변이는 제거했다. |
| UI 포커스 | 테마에서 단축키로 진입할 때 MIDI 토글 대신 body에 포커스가 남아 넓은·좁은 화면 두 경우 모두 실패했다. 테마·그래프 제목·진단 조작에서 진입하는 최종 테스트가 통과했다. |

### 최종 검사

모든 Rust 명령은 `RUSTC_WRAPPER=`를 사용했다. 아래 `cargo test`의 desktop 대상은
`-p mixrelay-desktop --bin mixrelay-desktop`이다. coverage gate나 기존 테스트를 낮추거나 생략하지 않았다.

| 명령 | 결과 |
|---|---|
| `cargo llvm-cov -p mixrelay-midi-control --lib --show-missing-lines --fail-under-functions 100 --fail-under-lines 100 --fail-under-regions 100 --fail-uncovered-functions 0 --fail-uncovered-lines 0 --fail-uncovered-regions 0` | 28개 통과, 404줄·650영역·57함수 모두 100% |
| desktop `cargo test ... midi::` | 37개 통과 |
| desktop `cargo test ... control_proxy::` | 91개 통과 |
| desktop `cargo test ... lifecycle` | 16개 통과 |
| desktop `cargo test ... app_menu::` | 4개 통과 |
| desktop `cargo test ... workspace_menu::` | 1개 통과 |
| `node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --lib graph_catalog` | 93개 통과 |
| `node scripts/lab/native-check.mjs cargo clippy --workspace --all-targets --all-features -- -D warnings` | 통과 |
| `cargo fmt --all`, `node scripts/lab/native-check.mjs cargo fmt --all --check` | 서식 적용·검사 통과 |
| `pnpm --filter @mixrelay/control-ui check` | 동작 2,293개·접근성 244개 통과, 줄·문장·분기·함수 모두 100%, 앱·Storybook 빌드와 Core 계약 69개 통과 |
| `pnpm check:ui-quality` | 609개 파일 통과 |
| `pnpm support:check` | 통과 |
| `node apps/website/check-site.mjs` | 183페이지 통과 |
| `pnpm storybook:build` | 통과 |
| `git diff --check` | 통과 |

중간에는 순차 ID를 기대하던 기존 테스트 여섯 개, 새 비활성 분기의 coverage 공백,
`matchMedia` fixture 복원, 테스트의 임시 값 수명·CSS fixture 로딩·서식 오류를 고쳤다.
서식 적용용 wrapper 호출은 `--check`만 허용해 거부됐으며 적용은 직접 `cargo fmt --all`로 했다.
`pnpm --filter @mixrelay/control-ui storybook --ci` 개발 서버는 시작했으나 `EMFILE` watcher 경고를 냈고 직접 중지했다.
최종 검사는 위 표와 같다.

문서는 docs 목적·technical 문체로 전체 변경 산문을 교정했다. 영어 변경 문단의 검출 점수는 0이며
따옴표 정규화 결과가 실제 HTML과 일치했다. 변경한 문서·11개 언어 매뉴얼·ActionButton MDX의
17개 전후 보존 검사는 모두 통과했다. 이 검증 보고서의 추가 섹션은 별도 보존 검사에서
제목 수 증가(7→12)로 실패했고 새 수치·residual 증가 경고도 받았다. 요청된 2차 검증 결과의 추가 범위와
대조해 제목·수치의 근거를 확인했으며 기존 보고서 본문은 유지했다. 기계적 검사가 번역의 모든 의미나 시각 적합성을 증명하지는 않는다.

### 화면 확인과 관리자 수락

기존 보관 MIDI 캡처와 Ableton의 MIDI 버튼·매핑 목록 PNG를 직접 열어 참조했다.
새 화면을 열기 위한 Chrome 접근은 컴퓨터 도구가 `Computer Use was not approved to use Google Chrome`으로 거부했다.
따라서 수정 후 실제 브라우저·Storybook의 화면, hover, 좁은 폭·긴 번역의 시각 수락은 확인하지 못했다.
DOM·CSS·axe·키보드 시험과 정적 빌드의 통과를 직접 시각 검수로 보고하지 않는다.
물리 MIDI·실제 설치본·Linux ALSA·네이티브 메뉴 조작·스크린리더 확인도 이번 범위에서 수행하지 않았다.

관리자는 다음 선택과 화면을 확인한다.

- 같은 하드웨어 입력의 정보는 그룹 제목에 한 번 표시하고, 각 매핑 제목만 선택 버튼으로 사용한다.
- 이미 admission을 통과한 쓰기는 완료한 뒤 매핑 변경을 응답한다. Pending 활성화 대기는 gate 밖에서 취소할 수 있다.
- 오른쪽 끝 compact 토글과 중립 pressed 표현, inactive chrome, 420px pseudo의 켠/끈 상태, 다크·포커스·선택·옵션·내부 스크롤을 확인한다.

정적 산출물은 `apps/control-ui/storybook-static`에 있다. MIDI 상태 story ID는 다음 12개다.

- `app-workspace--midi-controls`
- `features-midi-controls--map-mode-off`
- `features-midi-controls--map-mode-on`
- `features-midi-controls--selected-target-waiting`
- `features-midi-controls--mapped-controls`
- `features-midi-controls--dark-map-mode`
- `features-midi-controls--one-control-several-targets`
- `features-midi-controls--target-missing`
- `features-midi-controls--midi-unavailable`
- `features-midi-controls--narrow-long-translation`
- `features-midi-controls--narrow-long-translation-off`
- `features-midi-controls--keyboard`

관련 Core pressed 예시는 `core-controls-actionbutton--pressed`다.

## 3차 리뷰: MIDI 입력 경쟁 조건

기존 미커밋 변경 위에서 입력 경계·포트 간 누름 순서·학습 재선택을 수정했다.
커밋·푸시·rebase·다른 worktree 변경은 하지 않았다. UI 파일은 이번 수정에서 변경하지 않았다.
동작 계약은 [product.md](../product.md#midi-컨트롤러)에 반영했다.

### 파일별 변경

| 파일 | 변경 |
|---|---|
| `crates/midi-control/src/mailbox.rs` | 콜백 진입에서 수신 stamp를 예약하는 `Publication`을 사용한다. clear는 같은 수신 counter에 epoch 경계를 예약하고 이전 stamp의 누름·볼륨을 거부한다. 경계 게시 중 시작한 입력은 액션을 게시하지 않고 CC 임계값만 기억한다. 덮어쓴 셀을 건너뛰면서 캡처한 누름 prefix를 끝까지 배출한다. |
| `crates/midi-control/src/mailbox/tests.rs` | 슬롯 CAS 뒤·누름 게시 전 clear, clear 뒤 늦은 콜백, 슬롯 CAS 중 clear, epoch 게시 중 입력을 정확한 seam에서 재현한다. 새 epoch의 상대 이동·누름 수와 CC 임계값 보존도 검사한다. |
| `crates/midi-control/src/relative.rs` | 새 epoch의 첫 이동이 이전 epoch의 누적 변환을 상속하지 않도록 수신 stamp를 검사한다. |
| `crates/midi-control/src/relative/tests.rs` | 기존 전체 게인 범위·클리핑·경합 시험에 epoch 인자를 전달한다. 기존 assertion은 유지한다. |
| `crates/midi-control/src/learn.rs` | 원자 capture 상태에 62비트 epoch를 포함하고 선택·취소·캡처마다 증가시킨다. 입력 identity는 별도로 보관하며, 원자 소유권을 한 번만 시도해 게시 중 읽기·쓰기를 건너뛴다. arm/cancel은 진행 중 게시를 기다리지 않는다. |
| `crates/midi-control/src/learn/tests.rs` | WAITING 읽기 뒤 CAS 직전 재선택과 취소 후 재선택, CAS 뒤 재선택, 경합 중 읽기·offer, epoch rollover와 전체 u32 포트 identity를 검사한다. |
| `apps/desktop/src-tauri/src/midi/native.rs` | 학습 활성 여부를 읽기 전에 콜백의 수신 stamp를 예약한다. |
| `apps/desktop/src-tauri/src/midi/tests.rs` | 매핑 모드에서 시작해 게시 전에 멈춘 콜백을 실제 `midi_cancel` 명령 뒤 재개하고, 이전 누름 폐기와 새 누름 수용을 확인한다. |
| `apps/desktop/src-tauri/src/midi/worker.rs` | 각 포트의 누름 iterator가 경계를 캡처한 직후를 재현하는 seam을 추가한다. 제품 배출은 같은 경로를 빈 hook으로 실행한다. |
| `apps/desktop/src-tauri/src/midi/worker/tests.rs` | A의 0–1023·B의 1024를 준비하고, A의 배출 경계 캡처 뒤 cursor 0을 덮어쓴다. 실제 `process_inputs`에서 A의 남은 Record 1,023개 → B의 Stop → 다음 배출의 A Record 순서를 확인한다. |
| `docs/product.md` | 이전 콜백 폐기, 학습 세대 분리, 포트별 최신 1,024개와 초과 중 포트 간 순서 계약을 명시한다. |
| `docs/validation/2026-10-30-midi-map-mode.md` | 이 절에 실패 우선 근거·검사 결과·환경 제한을 기록한다. |

수신 epoch는 기존 공통 수신 counter의 경계로 표현한다. 누름·절대 값·상대 이동의 stamp를
같은 경계와 비교하며, 콜백 완료 시점에 새 세대를 붙이지 않는다. 저장 공간은 포트 생성 때만 준비한다.
콜백의 각 aggregate는 최대 8회 CAS, 학습 offer는 소유권 CAS와 상태 CAS를 각각 최대 1회 시도한다.
경합 시 기다리거나 재시도 루프를 늘리지 않으며 기존 dense input 무할당 시험을 유지했다.

### 수정 전 실패 근거

아래 시험은 sleep 없이 seam에서 경쟁 단계를 멈춘다. 컴파일·환경 오류는 실패 우선 근거로 세지 않았다.

| 지적 | 정확한 중단점과 수정 전 assertion 실패 |
|---|---|
| 1. 모드 종료를 넘긴 입력 | `a_press_published_across_a_clear_boundary_is_discarded`: 슬롯 CAS를 성공한 뒤 press cell·tail 게시 전에 clear했다. `pre-clear press survived cancellation`에서 실제 1개, 기대 0개로 실패했다. 콜백 진입 stamp API만 먼저 도입한 상태의 desktop `cancelling_map_mode_rejects_a_callback_that_has_not_published_yet`도 실제 `midi_cancel` 뒤 `old callback survived midi_cancel`, 실제 1개·기대 0개로 실패했다. |
| 2. 두 포트의 초과 중 역순 | `overflow_during_a_two_port_drain_preserves_transport_order`: A iterator의 0..1024 캡처 직후 A에 한 누름을 추가했다. `the retained prefix must precede the other port`에서 실제 발행 1개·기대 1,024개로 실패했다. 기존 구현은 A의 1–1023을 남기고 B의 Stop만 먼저 발행했다. |
| 3. 학습 재선택 ABA | `an_offer_from_an_earlier_selection_cannot_capture_a_rearmed_target`: A의 WAITING을 읽은 offer를 CAS 직전에 멈추고 B를 arm했다. `old offer captured target B; cancel=false`에서 실제 `Some((1, 0, Note, 60))`·기대 `None`으로 실패했다. 최종 시험은 직접 재선택과 cancel 후 재선택을 모두 검사한다. |

최종 코드에서는 위 assertion과 후속 새 입력·후속 배출 assertion이 모두 통과한다.
epoch 갱신 중 입력과 캡처 CAS 뒤 재선택도 별도 시험으로 검사한다.

### 검사 결과와 제한

모든 Rust 검사는 `RUSTC_WRAPPER= node scripts/lab/native-check.mjs` 뒤에 아래 명령을 전달했다.
서식 적용만 `RUSTC_WRAPPER= cargo fmt --all`을 직접 실행했다. 기존 테스트·coverage gate를 낮추거나 생략하지 않았다.

| 전달한 명령 | 결과 |
|---|---|
| `cargo llvm-cov -p mixrelay-midi-control --lib --show-missing-lines --fail-under-functions 100 --fail-under-lines 100 --fail-under-regions 100 --fail-uncovered-functions 0 --fail-uncovered-lines 0 --fail-uncovered-regions 0` | 37개 통과, 473줄·767영역·69함수 모두 100%, 누락 0 |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop midi::` | 34개 통과·5개 실패. 새 취소·두 포트 회귀는 통과했다. 실패 5개는 모두 Unix 소켓 bind의 `PermissionDenied: Operation not permitted`다. |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop control_proxy::` | 61개 통과·30개 실패. 실패 30개는 모두 Unix 소켓 bind의 같은 권한 오류다. |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop lifecycle` | 16개 통과 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 통과 |
| `cargo fmt --all --check` | 통과 |
| `git diff --check` | 통과 |

중간 커버리지 검사는 학습 비교 seam의 제네릭 인스턴스에 미실행 영역 1개를 보고했다.
기존 mailbox·relative와 같은 `dyn FnMut` seam으로 성공·실패가 같은 본문을 지나게 했으며
최종 100% gate를 통과했다. UI를 변경하지 않아 `pnpm --filter @mixrelay/control-ui check`는 실행하지 않았다.

소켓 제한은 시스템 임시 디렉터리와 이 worktree의 `target` 아래에서 별도 AF_UNIX bind로도 확인했다.
현재 실행 환경에서는 권한을 확장할 수 없어 두 desktop 검사 전체의 통과를 확인하지 못했다.
테스트를 skip하거나 소켓 경계를 mock으로 바꾸지 않았다. 물리 MIDI·설치본·Linux ALSA는 확인하지 않았다.
릴리스·서명·공증·키체인 명령은 실행하지 않았다.

추가 제품 결정은 없다. 통합 전 관리자는 Unix 소켓 bind가 허용되는 환경에서 `midi::`와
`control_proxy::` 전체 통과를 확인해야 한다. 초과 시 덮어쓴 입력만 건너뛰고 캡처한 나머지 prefix를
완료하는 방식을 적용했다. 문서는 docs 목적·technical 문체로 최종 교정했다.
변경 산문의 따옴표 정규화는 변경 없었고 영어 주석 검출 점수는 0이었다.
제품 문서 보존 검사는 통과했으며 새 수치 1,024 경고를 제품 계약과 대조했다.
이 검증 문서의 보존 검사는 제목 수 증가(12→16)로 실패했고 새 수치 경고를 냈다.
요청된 3차 검증 절의 추가 범위와 대조했으며 기존 본문 전체가 그대로 앞부분에 남는지도 확인했다.


## 집중 리뷰 후속: 취소 세대·상대 이동·실제 콜백 회귀

이번 변경은 아래 파일에 한정했다. 기존 미커밋 작업은 유지하고 커밋·푸시·rebase·다른 worktree 변경은 하지 않았다.
동작은 `docs/product.md`에 기록했다. UI·릴리스·서명·공증·키체인 작업은 수행하지 않았다.

| 파일 | 변경 |
|---|---|
| `crates/midi-control/src/mailbox.rs` | 취소마다 증가하는 64비트 세대를 공통 수신 순서와 분리했다. 세대는 동등 비교만 한다. 슬롯·누름 셀은 버전으로 세대와 payload의 일관성을 확인하며, 부분 게시 중 읽기는 기다리지 않고 다음 배출로 미룬다. |
| `crates/midi-control/src/relative.rs` | 배출이 캡처한 수신 stamp까지 조건부 CAS로 소비한다. 더 최신 이동은 그대로 남긴다. 새 취소 세대의 첫 게시만 이전 누적 변환을 초기화한다. |
| `crates/midi-control/src/mailbox/tests.rs` | 원시 값의 세대 확인 뒤 취소·새 상대 입력, `2^37`로 시드한 수신 순서, 확인 뒤 원시 값 교체, 부분 세대 게시를 결정적 seam으로 검증한다. |
| `crates/midi-control/src/relative/tests.rs` | 소비 CAS 직전 새 이동과 CAS 재시도 상한을 검증하고 기존 클리핑·전체 게인 범위 assertion을 유지했다. |
| `apps/desktop/src-tauri/src/midi/native.rs` | 실제 midir 콜백이 사용하는 `receive` 함수에 예약·학습 판정·게시 경로를 모았다. 상태 확인 직전 hook의 제품 인자는 빈 함수다. |
| `apps/desktop/src-tauri/src/midi/tests.rs` | 위 제품 함수를 별도 스레드로 구동하고 `capture.active()` 직전 채널로 멈춘다. 실제 `midi_cancel` 후 재개해 이전 누름 거부와 새 콜백 수용을 확인한다. |
| `docs/product.md` | 취소 세대와 수신량의 독립성, 이전 배출이 새 상대 이동을 소비하지 않는 계약을 추가했다. |
| `docs/validation/2026-10-30-midi-map-mode.md` | 이 절에 재현·검사 결과와 실패한 소켓 테스트를 기록했다. |

수신 순서는 기존 38비트 순환 비교를 유지하며 짧은 배출 창의 순서에만 사용한다.
취소는 별도 64비트 세대로 판정하므로 마지막 clear의 나이와 관계없다.
세대 변경 중에는 버전의 홀수 상태로 불완전한 payload를 거부한다. 같은 세대의 원시 값 소비도
캡처한 값과 CAS로 대조하므로 새 게시를 지우지 않는다. 각 상대 aggregate의 게시·소비는 최대 8회 CAS다.
콜백은 미리 할당한 저장소와 원자 연산만 사용하며 대기·동적 할당을 추가하지 않았다.
기존 dense input의 할당 0회 assertion도 통과했다.

### 수정 전 실패 확인

모든 중단점은 seam 또는 채널로 제어하며 sleep을 쓰지 않았다.

| 지적 | 재현과 실패 근거 |
|---|---|
| 1. 이전 배출이 새 상대 이동 소비 | `an_old_drain_leaves_post_cancellation_movement_for_the_next_drain`은 원시 값의 세대 확인·소비 뒤, 상대 이동 소비 전에 clear와 새 CC `2`를 게시한다. 수정 전 이전 배출의 이동이 `Some(2.0)`으로 나와 기대 `Some(0.0)`에 실패했다. 최종 테스트는 다음 배출이 새 원시 값 `2`와 이동 `2dB`를 함께 받는 것도 확인한다. |
| 2. 오래된 epoch와 반범위 비교 | `fresh_input_survives_half_a_receipt_range_without_a_clear`은 취소 세대 0을 유지하고 수신 counter를 `2^37`로 시드한다. 수정 전 새 누름 결과가 `[]`여서 기대 `[60]`에 실패했다. 최종 테스트는 연속 Note 두 누름, 원시 누름 수 2, 경계 전후 상대 입력 `1+2+3`의 합계 `6dB`도 검사한다. |
| 3. 네이티브 예약 위치 검증 | `cancelling_map_mode_rejects_a_callback_that_has_not_published_yet`은 활성 학습 상태에서 제품 `native::receive`를 구동한다. 예약을 `capture.active()` 뒤의 비활성 분기로 옮긴 변이에서 `old callback survived midi_cancel`, 실제 1개·기대 0개로 실패했다. 예약을 상태 확인 앞으로 복원한 최종 코드는 통과했다. 테스트는 `mailbox.begin()`을 직접 호출하지 않는다. |

### 최종 검사 결과

아래 Cargo 명령은 모두 `RUSTC_WRAPPER= node scripts/lab/native-check.mjs`로 실행했다.
서식 적용은 `RUSTC_WRAPPER= cargo fmt --all`을 사용했다. 테스트·커버리지 기준은 낮추지 않았다.

| 전달한 명령 | 결과 |
|---|---|
| `cargo llvm-cov -p mixrelay-midi-control --lib --show-missing-lines --fail-under-functions 100 --fail-under-lines 100 --fail-under-regions 100 --fail-uncovered-functions 0 --fail-uncovered-lines 0 --fail-uncovered-regions 0` | 42개 통과. 536줄·864영역·77함수 모두 100%, 누락 0. |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop midi::` | 34개 통과·5개 실패, 종료 101. 새 제품 콜백 취소 회귀는 통과했다. 실패는 모두 Unix 소켓 bind의 `PermissionDenied: Operation not permitted`다. |
| `cargo test -p mixrelay-desktop --bin mixrelay-desktop control_proxy::` | 61개 통과·30개 실패, 종료 101. 실패는 모두 같은 소켓 bind 권한 오류다. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 통과. |
| `cargo fmt --all --check` | 통과. |
| `git diff --check` | 통과. |

초기 회귀 실행은 위 두 로직 결함과 예약 위치 변이를 의도대로 검출했다.
중간 컴파일·확장한 시드 테스트의 준비 누락·커버리지·clippy 실패는 수정 후 다시 검사했다.
커버리지 보강은 부분 세대 게시, 원시 값 교체, 상대 소비 재시도에서 새 입력이 보존되는지 확인한다.
최종 desktop 원시는 `/private/tmp/midi-race-midi.log`, `/private/tmp/midi-race-control_proxy.log`에 남겼다.

### 소켓 권한으로 실패한 테스트

`midi::`에서 실패한 5개는 아래와 같다. `control_proxy::`에도 같은 5개가 포함된다.
접두사는 `control_proxy::local::midi::tests::`다.

- `dispatch_regressions::accepted_pending_waits_yield_to_mapping_changes_and_shutdown`
- `dispatch_regressions::mapping_changes_during_snapshot_io_fence_mute_recording_and_gain_writes`
- `dispatch_regressions::mapping_changes_wait_for_already_admitted_writes_then_fence_the_next_write`
- `dispatch_regressions::pending_application_is_acknowledged_before_the_next_fan_out_target`
- `midi_and_stream_deck_reach_the_same_in_process_node_control_boundary`

`control_proxy::exchange::tests::`의 20개는 다음과 같다.

- `body_transport_failures_resolve_once_without_resubmission`
- `changed_receipt_is_rejected_without_resubmitting`
- `client_ids_engine_fences_and_fast_acknowledgements_are_forwarded`
- `empty_completed_response_is_returned_and_acknowledged`
- `failed_acknowledgement_preserves_terminal_error`
- `fast_completion_of_another_operation_is_rejected_without_acknowledgement`
- `initial_connection_refusal_is_distinct_from_a_lost_poll`
- `invalid_json_is_not_a_transport_failure`
- `lost_poll_is_uncertain_even_when_connect_fails`
- `lost_send_missing_and_other_resolution_failures_are_uncertain`
- `lost_send_resolves_completed_or_pending_client_id_without_replay`
- `lost_send_returns_stored_terminal_404_and_acknowledges_it`
- `missing_receipt_after_restart_retains_uncertainty`
- `polls_once_submitted_work_and_preserves_terminal_accepted_status`
- `quick_accepted_status_is_not_an_operation_receipt`
- `receipt_observation_does_not_poll_or_acknowledge_the_writers_result`
- `receipt_other_than_the_client_id_is_rejected_without_polling`
- `recovery_body_failure_is_uncertain_and_does_not_lookup_again`
- `ui_operation_identity_survives_native_preparation_without_reusing_the_preview_receipt`
- `unknown_receipt_is_not_a_definitive_mutation_rejection`

`control_proxy::local::socket::tests::`의 5개는 다음과 같다.

- `a_live_slow_read_reports_pending_before_the_editor_liveness_deadline`
- `one_connection_carries_sequential_requests_until_the_peer_closes`
- `one_connection_runs_commands_reads_and_a_subscription_together`
- `quit_cancels_async_reads_but_joins_accepted_writes_and_native_discovery`
- `shutdown_cancels_idle_readers_and_removes_only_owned_socket`

권한 확장은 허용되지 않는 실행 환경이어서 이 두 desktop 검사 전체의 통과를 확인하지 못했다.
실패한 테스트를 skip하거나 소켓 경계를 대체하지 않았다. 물리 MIDI·설치본·Linux ALSA는 미확인이다.
새 제품 결정은 없다. 관리자는 통합 전에 소켓 bind가 허용되는 환경에서 위 두 검사를 다시 실행해야 한다.
문서는 docs 목적·technical 문체로 최종 교정했다.

변경 산문의 따옴표 정규화는 변경 없었고 영어 주석 검출 점수는 0이었다.
제품 문서 보존 검사는 통과했다. 이 검증 문서의 보존 검사는 제목 수 증가(16→20)로 실패했고
새 수치·패턴 증가 경고를 냈다. 요청된 후속 검증 절의 추가 범위와 실제 검사 결과를 대조했으며,
이전 본문이 모두 그대로 앞부분에 남는 것도 확인했다.
