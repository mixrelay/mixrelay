# control-ui 병렬 테스트 안정화 검증

2026-10-31, 기준 커밋 `5df1c6f1`. Node.js 24.21.0, pnpm 11.9.0,
Vitest 4.1.11, Testing Library React 16.3.3, user-event 14.6.6에서 확인했다.

## 원인과 수정

Testing Library React의 `asyncWrapper`는 입력 뒤 microtask를 배출하려고 0ms 타이머를
기다린다. 설치된 버전은 Jest API로 가짜 시계를 감지하므로, user-event에 Vitest의
`advanceTimers`만 전달하면 이 마지막 타이머가 멈춘다.
[상류 이슈 #1197](https://github.com/testing-library/react-testing-library/issues/1197)에도 같은 경계가 설명돼 있다.

기존 다섯 테스트 파일의 `input` 도우미는 입력과 `eventually`를 병렬로 실행해 이를 우회했다.
이때 가짜 시계가 250ms씩 전진하면서 MIDI 상태 폴링과 전체 화면 렌더를 반복했고,
React의 `act` 범위가 겹칠 수 있었다. 실패하면 `Promise.all`의 다른 입력 작업은 취소되지 않아
다음 테스트의 렌더·정리와 경쟁할 수 있었다.

`test-setup.ts`에서 Testing Library가 감지하는 `jest.advanceTimersByTime` 경계에 Vitest를
연결했다. 기존 React 비동기 래퍼는 유지하며 입력을 직접 await한다. 입력별 시간 전진은
`vi.advanceTimersByTimeAsync`로 처리하고, 타이머를 정리하다 실패해도 `finally`에서 시계와 mock을 복원한다.
MIDI 테스트는 매번 새 사용자 입력 상태를 만들고 모듈 mock의 호출 이력을 비운다.

`eventually`는 성공한 질의 값을 반환하므로 `findIn`이 같은 DOM 질의를 두 번 실행하지 않는다.
App의 실제 노드 매핑 검증과 헤더 포커스 검증은 각각 필요한 그래프로 실행한다.
Minimum·Maximum·Volume mode는 원래도 서로 상태를 갱신하지 않던 독립 사례였으므로
각각 실행해 실패를 분리했다. 기존 명령·포커스·상태·ID 보존 assertion은 유지했다.
제품 코드, 타이머 주기, 테스트 제한 시간, 워커 수와 커버리지 기준은 바꾸지 않았다.

## 회귀 확인과 측정 조건

새 입력 회귀를 원본 `test-setup.ts`에 실행하면 1초의 진단 상한 안에 입력이 완료되지 않아
`completed` assertion이 실패한다. 정리 과정에서 미완료 입력을 배출하므로 테스트 제한 시간을
소진하지 않는다. 수정 후에는 입력이 배경 폴링을 실행하지 않고 끝나는지, 비동기 작업 거절 뒤에도
다음 렌더와 입력이 동작하는지 확인한다. 변경한 여섯 파일의 테스트 43개가 통과했다.

변경 전 표는 원본 소스를 `/tmp/mixrelay-control-ui-baseline`으로 내보내 같은 의존성을 참조한
단독 전체 실행이다. 다른 worktree나 브랜치는 만들지 않았다. 이 실행은 2,293개가 통과했고
커버리지도 네 항목 100%였다. 간헐 오류는 이 단독 실행에서 발생하지 않았으며, 최장 시간은 5.325초였다.
원본 설정에서의 입력 회귀 실패는 별도로 확인했다.

변경 후에는 기본 4개 워커로 전체 실행을 다섯 번 연속 수행했다. 측정 중 다른 검사나
프로파일링 실행을 병행하지 않았다. 아래 시간은 Vitest의 테스트별 duration이며,
변경 후 상위 10개는 다섯 실행에서 각 테스트가 기록한 최댓값을 정렬했다.

실행 명령은 다음과 같다. `--reporter`와 `--outputFile.json`은 측정 자료만 추가하며 테스트를 걸러내지 않는다.

```sh
pnpm --filter @mixrelay/control-ui test --reporter=default --reporter=json --outputFile.json=/tmp/mixrelay-ui-final-N.json
```

원본 내보내기에서는 `pnpm_config_verify_deps_before_run=false`를 지정해 공유하는 의존성을
재설치하지 않고 같은 명령을 실행했다. 최종 다섯 실행에는 이 설정을 지정하지 않았다.
JSON과 원시 로그는 `/tmp/mixrelay-ui-before-clean.{json,log}` 및
`/tmp/mixrelay-ui-final-{1,2,3,4,5}.{json,log}`에 있다.

## 연속 전체 실행

| 회차 | 통과 / 실패 | 전체 시간(초) | 최장 테스트(초) | 커버리지 |
| --- | --- | ---: | ---: | --- |
| 1 | 2299 / 0 | 72.66 | 1.488 | 네 항목 100% |
| 2 | 2299 / 0 | 67.84 | 1.997 | 네 항목 100% |
| 3 | 2299 / 0 | 58.21 | 1.292 | 네 항목 100% |
| 4 | 2299 / 0 | 55.70 | 1.379 | 네 항목 100% |
| 5 | 2299 / 0 | 55.46 | 1.384 | 네 항목 100% |

커버리지는 statements 7,695/7,695, branches 6,177/6,177,
functions 2,230/2,230, lines 6,585/6,585다.
다섯 실행 모두 `overlapping act()` 및 `not configured to support act` 경고가 없었다.
기존 Focusable·controlled/uncontrolled·IPC 진단 stderr는 일부 테스트에 남아 있다.

## 변경 전 상위 10개

| 순위 | 파일 (`src/` 기준) | 테스트 | 시간(초) |
| --- | --- | --- | ---: |
| 1 | `app/App.midi.test.tsx` | undoes graph deletion from the graph list with the original target IDs, then redoes and restores again | 5.325 |
| 2 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | inserts Tunnel with connected stereo channels and undoes the entire chain in one step | 2.717 |
| 3 | `app/App.midi.test.tsx` | maps actual node controls, replaces the sidebar and restores it (narrow: false) | 2.431 |
| 4 | `app/App.midi.test.tsx` | maps actual node controls, replaces the sidebar and restores it (narrow: true) | 2.335 |
| 5 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | applies a Reverb preset atomically and separates preset selection from the following knob edit in Undo | 2.104 |
| 6 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | inserts Fishbowl with connected stereo channels and undoes the entire chain in one step | 1.946 |
| 7 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | inserts Karaoke with connected stereo channels and undoes the entire chain in one step | 1.934 |
| 8 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | loads externally disabled EQ settings and offers only recovery to processing | 1.852 |
| 9 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | selects channel mode 4 with a truthful result description and preserves Undo | 1.812 |
| 10 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | selects channel mode 5 with a truthful result description and preserves Undo | 1.807 |

## 변경 후 상위 10개

| 순위 | 파일 (`src/` 기준) | 테스트 | 시간(초) |
| --- | --- | --- | ---: |
| 1 | `app/App.midi.test.tsx` | maps actual node controls, replaces the sidebar and restores it (narrow: false) | 1.997 |
| 2 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | loads externally disabled EQ settings and offers only recovery to processing | 1.488 |
| 3 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | recovers stored Off and Mute on Compressor separately while retaining its bypass and DSP settings | 1.414 |
| 4 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | recovers stored Off and Mute on Limiter separately while retaining its bypass and DSP settings | 1.244 |
| 5 | `app/App.midi.test.tsx` | undoes graph deletion from the graph list with the original target IDs, then redoes and restores again | 1.241 |
| 6 | `screens/StudioDiagnostics.integration.test.tsx` | shares one scan session between the bottom-right diagnostics and processor menu without stopping audio | 1.227 |
| 7 | `app/App.midi.test.tsx` | maps actual node controls, replaces the sidebar and restores it (narrow: true) | 1.202 |
| 8 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | adjusts an EQ number with drag, wheel and keyboard while keeping the selected band and Undo coherent | 0.920 |
| 9 | `features/routing-workspace/RoutingWorkspace.builtinEffects.test.tsx` | recovers a saved Off and muted EQ in separate steps without changing DSP settings | 0.863 |
| 10 | `features/midi/MidiControls.test.tsx` | edits Minimum only on its selected mapping through the Core select | 0.848 |

## 필수 검사와 확인 범위

- `pnpm --filter @mixrelay/control-ui check`: 종료 코드 0. 경계·타입 검사,
  일반 테스트 2,299개와 커버리지 네 항목 100%, 접근성 테스트 244개,
  앱 빌드·WebKit 구문 검사, Storybook 빌드·디자인 시스템 문서 검사를 통과했다.
- `pnpm check:ui-quality`: 종료 코드 0. 609개 파일을 검사했다.
- 변경한 테스트 여섯 파일의 Vitest 실행: 43개 통과. 원본 설정에 같은 입력 회귀만
  실행했을 때는 완료 assertion이 실패했으며, 수정 후 전체 실행 다섯 번에서 통과했다.
- `git diff --check`: 통과. 변경은 커밋하지 않았다.

부분 파일만 실행한 커버리지 프로파일링은 전체 소스의 100% 기준에 미달해 실패했다.
이 진단 실행을 전체 커버리지 검증으로 사용하지 않았으며 기준을 낮추지 않았다.

제품 동작 변경과 매니저가 확인할 제품 결정은 없다. 이번 변경은 테스트 환경·fixture·질의와
개발 문서에 한정한다. 제품 화면과 Storybook 프리뷰의 수동 시각 확인,
실제 MIDI 장치·설치본 검증은 수행하지 않았다. 빌드·자동 접근성 검증과 구분한다.

문서·주석은 docs/technical 목적으로 교정했다. 영문 검출기의 독립 테스트 제목 길이 유사성
알림은 제목 목록의 특성으로 판단해 유지했다. 개발 문서의 인라인 코드 보존 검사와
따옴표 확인을 마쳤으며, 남은 교정 문제는 없다.
