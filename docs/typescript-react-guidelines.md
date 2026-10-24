# TypeScript·React 작업 규칙

`apps/control-ui`(React 19.2, TypeScript 7.0, Vite, Vitest, Storybook, React Aria, React Flow)에 적용하는 규칙이다. 작업에 해당하는 조건의 절을 먼저 읽는다. UI 디자인·접근성 계약은 [디자인 시스템](design-system.md), [조작·접근성 계약](design/interaction-accessibility.md), [리뷰 관점](design/ui-reviewers.md)이 정하고, 테스트 실행 환경은 [개발 작업 규칙](development.md)이 정한다. 근거: [Rules of React](https://react.dev/reference/rules), [You Might Not Need an Effect](https://react.dev/learn/you-might-not-need-an-effect), [Choosing the State Structure](https://react.dev/learn/choosing-the-state-structure), [TSConfig](https://www.typescriptlang.org/tsconfig/), [Google TypeScript Style Guide](https://google.github.io/styleguide/tsguide.html), [Testing Library](https://testing-library.com/docs/queries/about), [Bulletproof React 구조](https://github.com/alan2207/bulletproof-react/blob/master/docs/project-structure.md).

## 강제 수단
- **지금 검사가 막는 것:** `tsc`의 `strict`·`noUncheckedIndexedAccess`·`noFallthroughCasesInSwitch`·`isolatedModules`. Biome `recommended`와 `noExplicitAny`·`noNonNullAssertion`·인지 복잡도 15·함수 80줄. `check:boundaries`(UI 라이브러리는 Core만 import, 원시 컨트롤은 Core 전용, 색·토큰·CSS 규칙). 테스트 커버리지 100%.
- **규칙이지만 아직 검사가 막지 않는 것:** 기능 폴더 사이 import 금지, 렌더 중 ref 접근, Effect로 상태 파생, IO 응답 런타임 검증, `exactOptionalPropertyTypes`. 리뷰에서 확인하고, 검사를 추가할 때 이 목록을 갱신한다.
- 린트를 끄는 주석은 이유를 함께 쓴다. 의존성 배열 규칙(`useExhaustiveDependencies`)을 `void x;` 같은 우회로 맞추지 않는다.

## 구조를 만들거나 옮길 때
- Feature는 Core를, Screen은 Feature와 Core를, App은 그 모두를 import한다. 역방향(Core가 Feature를, Feature가 Screen·App을 import)은 금지한다. React Aria·React Flow는 Core만 직접 쓴다. Core는 디자인 시스템과 함께 도메인 모델(`core/audio-graph`), IO 포트(`core/control-port` 등), 테마 토큰을 소유한다.
- 기능 폴더는 서로 import하지 않는다. 둘 이상의 기능이 쓰는 타입·로직은 Core로 옮기고, 기능의 조합은 Screen이나 App에서 한다. 테스트도 같은 방향을 지킨다(기능 테스트가 `app/App`을 렌더하지 않는다).
- barrel(`index.ts` 재노출) 파일과 경로 별칭을 만들지 않고 모듈을 직접 import한다([Vite](https://vite.dev/guide/performance)). named export만 쓴다(Storybook 스토리 제외).
- 한 파일은 한 컴포넌트나 한 책임을 갖는다. 파일이 여러 책임을 갖거나 수백 줄을 넘기 시작하면 하위 컴포넌트·훅·순수 모듈로 나눈다.

## 타입을 쓸 때
- `any` 대신 `unknown`을 쓰고 좁혀서 사용한다. `enum`을 쓰지 않고 문자열 리터럴 union이나 `as const` 객체를 쓴다. 객체 모양은 `interface`, union·별칭은 `type`으로 쓴다.
- 타입 단언(`as`)과 non-null 단언을 쓰지 않는다. `(x as HTMLElement)`처럼 단언으로 null 검사를 우회하지 않고 실제로 검사한다. 단언이 꼭 필요하면 왜 안전한지 주석을 단다.
- 프로세스 밖에서 온 데이터(HTTP·Tauri 응답, 저장소, 메시지)는 포트 어댑터에서 런타임 검증을 거쳐 타입을 얻는다. `body as T`로 신뢰하지 않는다.
- 상태는 서로 모순될 수 없는 형태로 모델링한다. 여러 `boolean` 대신 `status` 같은 구분 union을 쓴다.
- 타입만 쓰는 import는 `import type`으로 쓴다. 던지는 값은 `Error`(또는 그 하위 클래스)만 쓴다.

## 컴포넌트를 쓸 때
- 렌더는 순수해야 한다. 같은 props·상태에 같은 결과를 내고, 렌더 중에 외부 값을 바꾸거나 부수 효과를 일으키지 않는다. props·상태·JSX에 넘긴 값은 바꾸지 않는다. 컴포넌트를 함수처럼 직접 호출하지 않고, 컴포넌트 안에서 컴포넌트를 정의하지 않는다(Rules of React).
- 렌더 중에 `ref.current`를 읽거나 쓰지 않는다(지연 초기화 제외). Effect 안의 이벤트성 로직이 최신 값을 읽되 그 값이 바뀌어도 다시 동기화하면 안 될 때만 `useEffectEvent`를 쓴다. 재구독이 필요한 값(장치·그래프·포트)은 의존성으로 두고, `useEffectEvent`를 의존성 누락의 해결책으로 쓰지 않는다([useRef](https://react.dev/reference/react/useRef), [useEffectEvent](https://react.dev/reference/react/useEffectEvent)).
- 훅은 컴포넌트·훅의 최상위에서만 호출한다. 목록의 `key`는 안정적인 ID를 쓰고 인덱스나 난수를 쓰지 않는다.
- 판단 로직은 컴포넌트 밖의 순수 모듈(예: `features/routing-workspace/model.ts`)에 두고 단위 테스트한다. 컴포넌트는 상태를 연결하고 화면을 그린다. 외부 상태 저장소는 `useSyncExternalStore`로 구독한다.
- 화면 단위에 에러 경계를 두어 한 영역의 렌더 오류가 앱 전체를 비우지 않게 한다.

## 상태·Effect를 쓸 때
- Effect는 화면에 보이는 동안 외부 시스템(포트 구독, DOM API, 타이머)과 동기화할 때만 쓴다.
- props나 다른 상태에서 계산할 수 있는 값은 렌더 중에 계산한다. 비싸면 `useMemo`를 쓴다. Effect에서 파생 값을 `setState`하지 않는다.
- prop이 바뀔 때 상태 전체를 초기화하려면 `key`를 바꾼다. 일부만 맞추려면 렌더 중에 조정하거나, 선택한 항목의 ID만 저장하고 나머지를 파생한다.
- 사용자 행동이 원인인 처리(요청 전송, 부모 알림)는 이벤트 핸들러에서 한다. Effect가 Effect를 부르는 연쇄를 만들지 않는다.
- Effect의 설정과 정리는 짝을 맞춘다. 비동기 요청은 정리 함수나 취소 플래그로 늦게 도착한 결과를 버린다. 해제 함수를 비동기로 돌려주는 구독(Tauri listener, 미터 구독)은 정리 후에 등록이 끝나면 받은 해제 함수를 즉시 실행한다. Strict Mode의 두 번 실행과 unmount 경쟁을 테스트로 확인한다([Synchronizing with Effects](https://react.dev/learn/synchronizing-with-effects)).
- 관련 있는 상태는 묶고, 중복·모순 상태를 두지 않고, 평평하게 유지한다. context는 테마·계정·포트처럼 넓게 쓰이는 값에만 쓰고, 먼저 props나 `children` 구성을 고려한다([Passing Data Deeply with Context](https://react.dev/learn/passing-data-deeply-with-context)).
- React Compiler를 쓰지 않으므로 `useMemo`·`useCallback`·`memo`는 실제로 비용이 크거나 참조 안정성이 필요한 곳에만 쓴다.

## IO를 다룰 때
- 엔진·Driver·라이선스·업데이트와의 통신은 Core의 포트 인터페이스(`ControlPort` 등)를 통해서만 한다. 구현(`HttpControlPort`, `TauriControlPort`, `StorybookMockControlPort`)은 어댑터이고, 포트는 props로 주입한다. 컴포넌트와 기능 코드는 `fetch`·Tauri API를 직접 부르지 않는다.
- 어댑터는 응답을 검증하고, 검증 실패를 포트 오류로 바꿔 알린다. 변경 명령의 응답 검증이 실패해도 서버에서 변경이 일어나지 않았다는 뜻은 아니다.
- 조회와 변경 명령을 구분한다. 화면과 다음 조작의 기준은 실행부가 확인한 상태다. revision이 명확히 거부되면 최신 문서에 실제 편집한 항목만 다시 병합해 재시도하고, 결과가 불확실한 변경 명령은 자동으로 반복하지 않고 편집을 보존한 채 확인 후 재시도한다([제품 문서](product.md)의 확인된 설정 규칙). 일반적인 자동 재시도나 낙관적 갱신을 변경 명령에 붙이지 않는다.

## 테스트를 쓰거나 고칠 때
- 사용자가 보는 방식으로 찾는다. 질의 우선순위는 `ByRole` → `ByLabelText` → `ByText` → … → `ByTestId`(최후 수단)다. `container.querySelector`로 DOM 구조에 기대지 않는다.
- 상호작용은 `userEvent.setup({ advanceTimers })`로 실제 입력 순서를 재현하고, user-event로 만들 수 없는 이벤트에만 `fireEvent`를 쓴다([user-event](https://testing-library.com/docs/user-event/options)).
- 자기 앱의 컴포넌트를 `vi.mock`으로 바꿔 끼우지 않는다. 경계는 포트이므로 가짜 포트(`StorybookMockControlPort` 등)를 주입한다.
- 없음을 확인할 때만 `query*`를 쓰고, `waitFor` 안에서 부수 효과를 일으키지 않는다. 불필요한 `act`로 감싸지 않는다([common mistakes](https://kentcdodds.com/blog/common-mistakes-with-react-testing-library)).
- 실제 시간을 기다리지 않는다. 가짜 타이머와 `src/test-support/fakeTimerQueries.ts`를 쓰고, 가짜 타이머를 끝내기 전에 남은 타이머를 실행한다. DOM이 필요한 테스트는 `*.test.tsx`, 나머지는 `*.test.ts`다([개발 작업 규칙](development.md)).
- 정의를 되풀이하는 테스트를 쓰지 않는다. 결함을 막는 테스트는 결함을 넣었을 때 실패하는지 확인한다.

## 접근성·스타일을 다룰 때
- 역할·이름·상태는 [조작·접근성 계약](design/interaction-accessibility.md)을 따른다. 맞는 네이티브 요소와 React Aria 컴포넌트를 먼저 쓰고, ARIA 속성을 덧붙여 의미를 만들지 않는다([APG](https://www.w3.org/WAI/ARIA/apg/practices/read-me-first/)).
- 색·간격·글꼴은 `core/theme/tokens.ts`의 역할 토큰과 CSS Modules로만 쓴다. 공통 규격은 Core에서 바꾸고 사용처에서 덮어쓰지 않는다.

## 피해야 할 것 요약
Effect로 상태 파생·초기화, 렌더 중 ref 읽기·쓰기, `void x;` 같은 의존성 우회, IO 응답의 `as T` 단언, 단언으로 null 검사 우회, 기능 폴더 사이 import, barrel 파일, 수천 줄짜리 컴포넌트, context 남용, 자기 컴포넌트 `vi.mock`, `fireEvent` 위주의 상호작용 테스트, `querySelector` 기반 테스트, 실제 시간 대기, 에러 경계 없는 화면.
