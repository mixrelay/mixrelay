# MixRelay 디자인 시스템

Carbon의 **역할 토큰, 컴포넌트 계약, 상태별 검증**을 차용한다. 색·글꼴·간격·밀도·모양은 MixRelay의 제품 판단으로 정한다. 구현 기반은 React Aria Components와 React Flow다. React Aria는 조작·포커스·접근성 동작을 제공하고, 외관은 Core의 역할 토큰과 CSS가 정의한다. Carbon 패키지·IBM 팔레트는 도입하지 않는다.

디자인 시스템 문서의 진입점은 Storybook `Design System`이다. 문서 구조·운영은 Storybook 공식 문서화·구조·Docs addon 가이드를 따른다([근거](#근거)). 적용한 기준과 기여·호환성 절차는 Storybook `Design System/Contributing`, 실행·컴포넌트 선택은 `Get Started`에 둔다. 운영 방식은 [Storybook 문서와 운영](#storybook-문서와-운영)에 정리한다.

## 기준과 위치

| 기준 | 위치 | 책임 |
| --- | --- | --- |
| 제품 동작 | [product.md](product.md) | 실행·제어·무료/유료 정책의 단일 기준 |
| 시각 값 | [tokens.ts](../apps/control-ui/src/core/theme/tokens.ts) | 팔레트·글꼴·역할 및 컴포넌트 토큰 |
| 테마 연결 | [MixRelayProvider.tsx](../apps/control-ui/src/core/theme/MixRelayProvider.tsx) | 앱과 Storybook에 같은 CSS 역할 변수를 공급; 라이트·다크·시스템 선택과 저장 |
| 공통 UI | [Core UI](../apps/control-ui/src/core/ui) | 버튼·입력·상태·미터·노드·메뉴 계약 |
| 그래프 UI | [Core audio graph](../apps/control-ui/src/core/audio-graph) | 연결·포트·선택·그래프 조작 |
| 사용례 | Storybook `Core/*`, `Features/*` | 실제 컴포넌트의 상태·번역·화면 폭 확인 |
| 리뷰 | [ui-reviewers.md](design/ui-reviewers.md) | 람스·아이브·Teenage Engineering 관점과 출처 |

의존 방향은 `React Aria / React Flow → Core → Feature → Screen`이다. Feature는 엔진 상태와 동작을 연결하고, Core는 표시와 조작 규격을 제공한다. Screen은 조합한다.

## 시각 방향과 공통 규격

**텍스트 중심의 정밀하고 절제된 오디오 도구.** 고급스러움은 글자·간격·정렬의 완성도로 만든다. 일반 명령은 짧은 텍스트, 재생·녹음·전원·음소거는 익숙한 기호를 사용한다. 앱 노드의 실제 앱 아이콘은 유지한다. 색은 선택과 실제 오디오 상태·오류를 설명한다. 포커스는 중립색의 얇은 표시로 구분한다.

사용자가 확인한 참고 후보는 [Monome](https://monome.org/), [TE TX–6](https://teenage.engineering/products/tx-6), [Vitsœ / Rams](https://www.vitsoe.com/us/about/good-design), [Ableton Live](https://www.ableton.com/en/live/)다. Valhalla는 시각 레퍼런스에서 제외한다. 텍스트·정렬·반복 조작·이름/값/단위의 계층을 참고하며, 브랜드 외형이나 하드웨어 노브를 복제하지 않는다.

라이트는 따뜻한 오프화이트, 다크는 깊은 그래파이트를 사용한다. 배경·노드·입력은 DSUB 팔레트의 서로 다른 표면 역할로 구분하고, 글자·선택·포커스는 각 테마의 중립색으로 표시한다. 녹음·경고·신호 레벨의 색은 의미를 유지하며 정상적인 명령 버튼까지 상태 색으로 칠하지 않는다. 이 팔레트는 레퍼런스에서 얻은 절제와 기능 구분을 MixRelay에 적용한 결정이다.

| 규격 | 기준 |
| --- | --- |
| 글자 | 제목 16, 본문 13, 명령/입력 12, 레이블 11, 보조 값 10px. 숫자·단위는 고정폭/등폭 숫자. 조밀한 미터 눈금만 `--meter-scale-font-size` 8px 예외 |
| 조작 크기 | 일반 명령·필드 28, 헤더의 compact 명령·노드 아이콘 24px. 보이는 모양과 클릭 영역을 구분 |
| 형태 | 컨트롤·떠 있는 화면 2px 모서리, 1px 경계. 노드는 평면; 그림자는 떠 있는 메뉴·모달에만 사용 |
| 명령 | 일반은 투명한 중립색 텍스트, 제출/확인은 중립색 반전 채움. 상시 강조색 배경·버튼 크기 변화·불필요한 대문자 사용을 피함 |
| 상태 | 짧은 상태 텍스트 + 작은 표식. 색만으로 상태를 전달하지 않음. 녹음은 빨강, 일시정지는 주의 색 |
| 포커스 | 중립색 1px 표시 하나. 입력은 기존 경계 색만 바꾸며 바깥 링을 겹치지 않음. 버튼은 키보드 포커스에 1px 윤곽·2px 간격, 조밀한 조작은 안쪽 윤곽 허용. 선택·pressed와 별개 |

공통 시각 규칙은 [ComponentTheme.module.css](../apps/control-ui/src/core/theme/ComponentTheme.module.css)와 Provider에 둔다. 컴포넌트 CSS에는 배치·고유 좌표와 의미별 상태만 둔다. Feature의 글자와 모서리도 같은 역할 토큰을 사용한다.

## 토큰과 테마

### DSUB 팔레트와 역할

팔레트 → 의미 역할 → 테마별 할당 → Core 컴포넌트 → 제품 화면 순서로 적용한다. `tokens.ts`의 `palette`는 neutral·graphite·red·green·blue·yellow·orange 각각 50–900의 10단계를 보유한다. 단계는 상대 순서이며 계열 사이 같은 번호가 같은 명도라는 뜻은 아니다. 사용자 제공 HEX는 웹용 결정값으로, 제조사 공식 도장색이나 실측값으로 소개하지 않는다. DSUB 로고 원색은 `logoColor / --logo-source`의 red-500 (`#b02d23`)으로 고정한다. 테마별 틴트·필터를 적용하지 않는다. MixRelay 제품 아이덴티티도 같은 red-500을 사용하고 외부 앱 아이콘의 색 소유권은 유지한다. 주황은 현재 상태 의미를 배정하지 않는다.

| 역할 | Light | Dark |
| --- | --- | --- |
| background | neutral-100 | graphite-900 |
| surface | neutral-50 | graphite-800 |
| surface-muted | neutral-200 | graphite-700 |
| surface-hover | neutral-300 | graphite-600 |
| surface-active | neutral-400 | graphite-500 |
| text-primary | neutral-900 | graphite-50 |
| text-secondary | neutral-800 | graphite-100 |
| text-muted | neutral-700 | graphite-200 |
| border-default | neutral-500 | graphite-400 |
| border-strong | neutral-600 | graphite-300 |
| status-accent | blue-700 | blue-300 |
| status-positive | green-700 | green-300 |
| status-warning | yellow-800 | yellow-300 |
| status-danger | red-700 | red-300 |

`themeRoles.light / dark`는 동일한 역할 이름을 제공한다. Core CSS는 이 정식 역할(`--background`, `--text-primary`, `--text-muted`, `--border-default`, `--status-*` 등)을 직접 사용하며 같은 값의 별칭을 두지 않는다. 이전 별칭 `--bg`, `--text`, `--muted`, `--line`, `--line-strong`, `--red`, `--amber`, `--positive`, `--accent`, `--surface-raised`, `--signal`은 제거했다. 노드·미터·transport·메뉴·feedback의 컴포넌트 토큰도 이 역할을 참조한다. 선택 표시는 `--node-border-selected`·`--cable-selected`·`--canvas-resize-text-hover`·`--node-port-dot`처럼 컴포넌트 역할로 이름 붙인다. 의미 있는 입력 경계는 `--border-control`(Light neutral-700, Dark graphite-300)을 사용해 장식 경계와 구별한다. 중립색 focus는 `--text-secondary`로 1px 규격을 유지한다.

### 상태 의미

일반 설명·분류·로딩은 중립, 의미 있는 정보·처리 진행은 accent, 인증·연결·저장 등 확인된 성공은 positive, 사용자 조치·부분 완료·복구 가능한 중단은 warning, 실패·잘못된 입력·거부는 critical로 표시한다. Core의 `critical`은 `status-danger`에 매핑된다. 상태에는 문구·접근성 이름 또는 아이콘을 함께 제공하고 화면 선택을 실행 성공으로 표시하지 않는다. 녹음의 빨강은 오디오 transport 의미이며 오류 문구와 구별한다. 노드 Off·Mute·앱 전체 일시정지·녹음 일시정지의 기존 실제 상태 계약은 유지한다. Off 노드는 전체 opacity를 낮추지 않고 Core `--node-background-off` 표면과 실제 Off 버튼 상태로 표시해 남은 조작과 글자의 대비를 보존한다.

`StatusBadge / StatusIndicator / Text / InlineNotice`는 같은 상태 언어를 사용하며, UpdateAccess의 최신·발견·조회 실패는 각각 positive·accent·critical을 선택한다. 정상 발표가 없는 상태는 neutral이다. 공통 컴포넌트에서 확장한 규격을 StudioScreen, AudioNodeCard, Meter, 메뉴·필드·모달과 GraphCreateDialog, NameDialog, LicenseDetails, UpdateAccess가 소비한다.

### 명령 버튼

일반 버튼은 투명한 중립색이며 기본 테두리·밑줄 없이 hover·pressed 표면 명도로 반응한다. 업데이트 다운로드 열기와 오류 재시도도 일반 명령을 사용한다. `ActionButton emphasis="strong"`은 Light neutral-900/neutral-50, Dark graphite-50/graphite-900 반전 채움이다. 새 그래프 생성·이름 확정·라이선스 활성화가 이를 사용한다. 위험 명령의 문구·확인 흐름은 유지하고 일반 ActionButton의 critical 의미를 일괄 빨간 글자·채움으로 바꾸지 않는다. 오류 메시지·노드 상태·녹음 표시는 상태색을 유지한다. 실제 링크의 밑줄·조작 의미와 버튼을 구별한다. 일시정지의 active 배경 혼합 비율·경계·글자도 `--transport-pause-*-active`가 소유한다.

### 테마 전환

`theme.ts`는 토큰에서 CSS를 매 빌드마다 자동 재생성해 Provider의 style에 투영한다. 별도 CSS 색 사본을 수작업으로 관리하지 않는다. `MixRelayProvider`의 기본값은 `system`이며 OS의 `prefers-color-scheme` 변경을 CSS 미디어 쿼리로 즉시 따른다. 헤더 Core `ThemePicker`는 시스템·라이트·다크를 checked 메뉴로 제공한다. `mixrelay.theme` 로컬 저장값을 시작 시 복원하고 변경·다른 창의 storage 이벤트를 반영한다. 저장소 접근 실패 때도 해당 세션에서 선택할 수 있다. Storybook의 제어된 `theme`와 toolbar는 앱의 저장값을 쓰지 않는다. Screen과 Feature는 HEX나 테마별 분기를 지정하지 않는다. 커스텀 테마 편집·파일 로딩은 이번 기본 테마 범위에 포함하지 않는다.

빌드된 앱 `index.html`의 `<head>`에는 토큰 테마 CSS와 bootstrap 스크립트가 들어간다. Storybook iframe 문서에는 넣지 않아 Storybook의 제어된 theme가 유지된다. 원본은 `src/core/theme/themeDocument.ts`이며 `vite.config.ts`의 Vite 플러그인이 Vite `runnerImport`로 불러와 주입한다. 저장된 라이트·다크 선택은 첫 화면에서 OS 테마로 깜박이지 않는다. 이후 변경은 계속 `MixRelayProvider`가 소유한다. Core `NativeWindowTheme`은 Tauri 창 테마를 같은 선택과 동기화하고 시스템 선택은 `null`로 전달한다. 데스크톱 capability는 `core:window:allow-set-theme`를 포함한다.

Storybook `Foundations/Light·Dark`는 역할과 전체 팔레트, `Design Language`는 정보·성공·주의·오류·입력·강한 명령, `App/Workspace/Light·Dark`는 실제 앱 조합을 보여준다. 모든 기존 스토리도 테마 toolbar로 전환할 수 있다. `tokens.test.ts`는 단계 명도 순서·로고 유지·역할 대칭, 기본·hover·pressed 표면 텍스트 4.5:1, 기능 경계·선택·focus 3:1, 강한 명령과 feedback의 대비를 검사한다. 실제 브라우저·키보드·좁은 화면·긴 번역 결과와 설치본 검증은 구별한다. React Flow의 고정 다크 배경은 Core 그래프 CSS가 `--xy-background-color → --canvas-background`로 연결한다.

### 토큰 규칙

- 팔레트와 글꼴은 원시 값, `--surface`, `--text-primary`, `--status-*` 등은 공통 역할이다. 같은 값을 다른 이름으로 부르는 별칭을 만들지 않는다.
- `--transport-*`, `--meter-*`, `--port-*`, `--node-*`는 해당 컴포넌트의 역할이다. 같은 색이라는 이유만으로 다른 의미를 합치지 않는다.
- `--space-*`, `--control-gap`, `--node-inset`은 반복 간격이다. Feature·Screen CSS의 간격도 `--space-*`를 사용한다. 포트 좌표·SVG 경로·계산된 노드 크기는 해당 레이아웃에 둔다.
- 새 색은 `tokens.ts`에 역할로 추가한다. 컴포넌트의 CSS·JSX에 색 값을 직접 쓰지 않는다. 투명도·hover·active·disabled도 역할에 포함한다.
- 모든 `z-index`는 `var(--layer-*)`, 모든 전환·애니메이션 시간은 `--motion-*` 토큰, 글자 크기·글꼴은 역할 토큰을 사용한다.
- 팔레트와 글자·크기·형태·강조 방식은 위 시각 방향에 맞춰 공통 토큰으로 관리한다. 전체 접근성 적합이나 모든 설치 환경의 검증 완료를 뜻하지 않는다.

### 간격·글자·모션·쌓임

`tokens.ts`의 `spacing / typography / fontWeight / lineHeight / border / motion / layers`는 공통 값이다. `MixRelayProvider`는 그 정의를 CSS 역할 변수에 연결하고 Core의 필드·메뉴·모달이 직접 소비한다. 기존 `--space-1 / 2 / 3 / 4 / 6` 값은 유지하고, 2·32·40·48·64·80·96·160px 단계를 보완했다. 작은 조작부터 섹션까지 같은 스케일을 쓰되, SVG 좌표·포트 계산·저장된 창/노드 크기는 해당 소유자의 기하 정보로 남긴다.

| 토큰 | 값과 용도 |
| --- | --- |
| `motion` | `instant` 80ms(포트 피드백), `fast` 120ms, `state` 180ms, `spin` 1000ms, `tooltipDelay` 350ms. `--motion-duration-*`로 공급 |
| `--motion-duration-reduced` | `prefers-reduced-motion`에서 테마 계층이 상태 전환 시간을 0.01ms로 바꾼다. transitionend/animationend가 계속 발생하며, 이를 사용하는 컴포넌트는 개별 오버라이드 없이 따른다 |
| `layers` | 포털 단계: dialog 200, floating 300, tooltip 1000. 메뉴·팝오버·대화상자의 실제 포털 스타일에도 적용 |
| `shellLayers` | 앱 셸 단계: header 2, sidebarOverlay 5, chrome 6 |
| `canvasLayers` | 그래프 캔버스 단계: cable·connectionLine 0, node 1, nodeSelected 2, nodeOptionsOpen 3, handle 3, portSelection 5, toolbar 5, handleHover 6, selectionFeedback 8 |
| `shellBreakpoints.overlaySidebar` | 720px. CSS 미디어 쿼리는 사용자 정의 속성을 읽을 수 없어 `StudioScreen.module.css`가 같은 값을 반복 |
| `--control-placeholder` | `--text-muted`. placeholder 글자 |
| `--meter-scale-font-size` | 8px. caption보다 작은 유일한 글자; `--dynamics-scale-font-size`가 참조 |
| `--status-indicator-size` / `--status-indicator-neutral` | 7px 상태 점과 중립 색 |
| `--progress-size` / `--progress-track` / `--progress-indicator` | `ProgressIndicator` 링의 크기·트랙·표시 |
| `--node-icon-size` | 14px 노드 아이콘 |
| `--tooltip-readout-*` | readout 툴팁의 안쪽 여백·경계·배경·그림자·고정폭 글꼴 |

쌓임 단계는 CSS에서 `--layer-*`로 노출하며 그래프 어댑터는 노드 z-order에 `canvasLayers`를 사용한다. 캔버스 내부 쌓임 순서는 그래프 Core가 소유하고 포털 메뉴·모달의 단계는 `layers`로 별도 정의한다.

## 서체

### IBM Plex

Core `theme/fonts`의 IBM 원본 WOFF2 12개를 앱·Storybook에 동봉한다. `Fonts.css`가 Regular 400 / Medium 500과 `html lang`·명시적 언어 영역의 CJK 선택을 소유하고 `tokens.ts`가 Sans/Mono 역할을 연결한다. Latin·키릴 문자는 IBM Plex Sans, 한국어는 KR, 일본어는 JP, 간체 중국어는 SC, 번체 중국어(zh-Hant/zh-TW/zh-HK/zh-MO)는 TC를 사용한다. Latin Sans를 먼저 두고 지역별 CJK 서체를 뒤에 둔다. 수치·시간·단위·미터 값은 IBM Plex Mono이며 일반 문장과 명령 레이블은 Sans다. Storybook의 한국어 설명문은 `lang="ko-KR"`로 선언하며 실행 예시의 전역 언어와 구별한다. 언어 영역에서는 두 family 변수만 같은 토큰에서 재선언하고 크기·행간·색 등 다른 부모 계약을 유지한다. 시스템 글꼴이나 CDN이 실제 제품 서체를 결정하지 않는다.

기존 16/13/12/11/10px 역할과 8px 미터 예외, 행간 1.25/1.4/1.5를 유지한다. 서체 메트릭은 지역마다 달라지므로 Storybook의 11개 언어, 390px 폭, 긴 번역에서 기준선·줄바꿈·클리핑을 함께 확인한다. `Core/Design System/Foundations`의 Typography Locales가 실제 Core 입력과 수치로 검토한다. 20px display는 기존 디자인 시스템 문서용 역할이다.

정적 웹은 `node scripts/sync-website-fonts.mjs`로 Core의 원본·역할·지역 선택을 생성한다. `fonts/sources.json`에 IBM 다운로드 주소와 SHA-256이 있고 원본 OFL을 앱 리소스·라이선스 화면·웹에 동봉한다. [IBM Plex](https://github.com/IBM/plex)의 원본을 수정하거나 서브셋하지 않는다. 선택한 Optical Grid 심벌은 유지한다. 조판된 MixRelay 표본은 확정 워드마크가 아니다.

## 컴포넌트 계약

### 계약의 소유와 문서 연결

모든 독립 계약은 `apps/control-ui/src/core/design-system/componentContracts.ts` registry에서 실제 component metadata를 가진 Playground·API·기본값·상태·사용례를 연결하며 빌드는 그 Core export 소유자까지 확인한다. 설명·API·Playground·상태·실제 조합은 같은 소스를 소비한다. iframe Canvas의 속성 조작은 Playground Story 화면에서 수행하며 정적 ArgTypes를 Controls로 표시하지 않는다. 공개 API를 바꾸면 `componentContracts.ts`의 속성 명세와 해당 상태 예시를 함께 갱신한다.

`pnpm --filter @mixrelay/control-ui docs:api <source> <Name>`은 계약의 API 발췌를 출력하고, `docs:page <Name> [--force|--stdout]`은 계약에서 표준 컴포넌트 MDX 페이지를 생성한다.

Core의 기본 디자인은 테마가 공급하는 역할 토큰으로 결정한다. Feature/Screen은 각 컴포넌트에 토큰 객체나 임의 스타일을 전달하지 않고, 의미·상태·밀도와 동작을 전달한다. `ActionButton`의 `tone / density / emphasis`, `Text`의 `kind / tone`, `Stack / Inline`의 간격 역할이 이 계약이다. 토큰 값 변경은 앱과 Storybook의 같은 소비자에 전달된다.

간격은 자식마다 margin을 추가하는 대신 조합이 소유한다. 필드 묶음은 Stack, 명령 묶음은 Inline, 폼의 최종 명령은 FormActions를 사용한다. 공통 섹션을 사용처의 h2/h3 스타일로 다시 정의하지 않는다. Core 컴포넌트는 호출자에게 `style / className / classNames` 확장점을 제공하지 않는다.

### 사용처에서 허용하는 것과 금지하는 것

- Feature/Screen은 데이터·동작·문구와 Core가 정의한 의미별 선택지를 전달한다. 명령 버튼은 `tone`, `density`, `emphasis` 등 명시된 계약으로 선택한다. React Aria의 전체 props나 `style`, `styles`, `className`, `classNames`, 임의 색·글꼴·모서리·크기를 전달하는 API를 Core에 다시 노출하지 않는다.
- 버튼·입력·선택·체크 조작의 DOM과 시각 상태는 Core가 소유한다. Feature/Screen에서 네이티브 조작 요소를 새로 꾸미지 않는다. 새로운 조작은 Core 컴포넌트와 상태 사용례를 먼저 추가한다.
- 사용처 CSS는 화면 배치와 콘텐츠의 역할 토큰을 사용하며 간격은 `--space-*`로 선택한다. Core/라이브러리 내부 클래스, 전역 선택자·스타일 삽입, 조작 요소를 향한 후손 선택자, `!important`, 시스템 토큰 재정의로 공통 표현을 덮어쓰지 않는다. 토큰을 사용한 오버라이드도 금지 대상이다.
- 화면 고유 좌표·폭·스크롤·계산된 레이아웃은 해당 소유자에 둔다. 로컬 CSS 변수는 `--layout-*`로 구분하고 시각 규격의 대체 값으로 사용하지 않는다. 글자 크기·글꼴은 역할 토큰으로 선택한다. Core 컴포넌트의 외부 배치가 필요하면 사용처가 자체 레이아웃 요소로 감싼다. 예를 들어 `ApplicationStatusBar`는 `className`을 받지 않으며 `StudioScreen`이 자체 레이아웃 요소로 grid 위치를 지정하고, 높이·안쪽 간격·표면은 Core가 소유한다. `WorkspaceHeader`는 사용처의 스타일 확장점을 제공하지 않는다.
- 공통 규격은 모든 소비자에 반영하고 의미별 차이는 Core가 소유한 계약으로 표현한다. 화면 이름을 붙인 일회성 토큰·variant나 검사 예외 목록으로 우회하지 않는다.

### React Aria 동작 계약

Core는 필드·선택·메뉴·대화상자·체크·스위치·슬라이더·툴팁의 동작을 React Aria Components로 구현한다. 반복되는 내부 조작은 `ValueSlider`와 `Tooltip`이 공유한다. Core 밖에는 라이브러리의 스타일·render·전체 props를 노출하지 않는다. 노드 크기·포트 좌표와 드라이버의 버퍼 소유권은 변경하지 않는다.

선택 필드는 이름·선택 값이 함께 읽히는 버튼과 listbox로 구성한다. 소유자가 값을 확인하기 전까지 현재 readback을 유지하고 같은 옵션 재선택도 재시도로 전달한다. 슬라이더는 네이티브 range input의 값·범위·disabled 상태와 단위 설명을 노출한다. 비밀번호 입력은 같은 필드 계약에서 표시·숨기기를 제공하며 11개 지원 언어의 조작 이름을 사용한다. 메뉴·팝오버·대화상자의 실제 포털 스타일에도 `layers` 토큰을 적용한다.

값을 마지막으로 확인한 소유자와 포커스 소유자를 유지한다. 메뉴는 방향키로 하위 목록을 열고 Escape로 닫는다. 배경에 열린 컨텍스트 메뉴는 다른 노드의 한 번 클릭을 가로막지 않으며, 터치 이동·해제·비활성 전환은 long press를 취소한다. 대화상자는 바깥 UI를 접근성 트리에서 차단하고 닫힌 뒤 트리거에 포커스를 돌려준다.

### Core 계약 표

| Core 계약 | 소유하는 규격 | 실제 사용처 |
| --- | --- | --- |
| Stack / Inline | `tight / regular / section / wide` 간격, 세로/가로 조합·정렬·줄바꿈; `Stack align="center"`, `Inline justify="center"`; 포커스 대상 아님 | UpdateAccess 다운로드 조합, RuntimeConnection |
| FormActions | 버튼 행 간격·앞 여백·정렬·좁은 폭 줄바꿈 | GraphCreateDialog, UpdateAccess, NameDialog, RecoveryActions |
| Text | 본문·보조·레이블·캡션 글자와 기본/보조 색; `kind="heading"`은 16px Medium이며 `as` h1/h2와 `id` 지정; 문단 여백은 조합 소유 | UpdateAccess의 설치/최신 버전·설명·상태, RuntimeConnection |
| ContentSection | 같은 제목 규격·콘텐츠 간격·선택적 앞/뒤 구분선, 제목으로 연결된 section 이름 | 업데이트 제품별 항목 |
| InlineNotice | 기본·정보·성공·주의·실패의 표면·글자·경계·안쪽 여백과 선택적 동작; 실패는 alert, 나머지는 status | GraphCreateDialog·NameDialog의 제출 오류, UpdateAccess의 다운로드 오류 |
| ProgressIndicator | 불확정 진행 링과 시각적으로 숨긴 status 레이블; reduced motion에서는 회전하지 않음 | RuntimeConnection |

| 컴포넌트 | 상태와 동작 | 필수 표시 |
| --- | --- | --- |
| ActionButton / StateButton | 일반 명령·확인·위험 동작; 기본·hover·focus·busy·disabled | 짧은 동작 이름, 상태 전환 중 고정된 대상 크기; 위험 동작은 critical 의미 |
| TextField / SelectField / InlineSelect | 편집·선택·읽기 전용·disabled·오류·적용 대기 | 레이블·현재 값·오류; compact 선택도 같은 입력 역할과 규격 |
| ToggleField | on/off·disabled | 레이블과 checked 상태; 색 외 위치/형태 변화 |
| CheckboxField / PathField / TitleField | 확인 체크·읽기 전용 경로와 별도 선택 동작·제목 편집 | 실제 입력 역할과 이름·값·상태; 표시와 입력의 규격은 Core가 소유 |
| IconActionButton / CollectionItem / SidebarToggle | 아이콘 명령·목록 선택/제거·사이드바 열림/닫힘 | 명령 이름·비활성·현재 선택·펼침 상태; 시각 규격을 사용처 CSS에서 덮어쓰지 않음 |
| TransportButton | 기본·hover·focus·active·disabled; active+disabled에서도 상태 유지 | 같은 크기의 아이콘, 동작 이름·툴팁, 실제 pressed 상태. 평상시 재생·일시정지 버튼에는 보이는 경계를 두지 않으며, 키보드 포커스 윤곽은 유지 |
| NodeSignalControls | Off와 Mute를 각각 제어; 노드 드래그와 분리 | 버튼 이름·pressed 상태; 색 외에도 의미 전달 |
| NodeTransportRow | 노드의 값·시간과 transport 조작; 기본·disabled·녹음 중 | 같은 24px 조작 행, 왼쪽 등폭 값·오른쪽 버튼. Feature는 값·상태·명령·설명만 전달 |
| LevelControl / 노드 슬라이더 | 범위·단위·값·키보드 조작; 값은 Feature에서 전달 | 레이블·현재 값·단위; 제어 방향 일관성 |
| StatusBadge / StatusIndicator | neutral·accent·positive·warning·critical | 엔진 상태를 설명하는 텍스트 또는 접근 가능한 이름; StatusIndicator는 7px 점을 소유 |
| Tooltip | `kind="label"` 조작 이름, `kind="readout"` 고정폭 값 표시 | hover·키보드 focus에서 같은 툴팁; 트리거의 이름을 대신하지 않음 |
| Meter | 수신 레벨·무음·높은 레벨; 수평/수직 | 범위 밖 입력은 표시 길이 제한; 색 구간은 clipping 이벤트 판정이 아님 |
| AudioNodeCard / 포트 | 선택·Off·Mute·장애·연결 가능 여부; 묶음 선택은 한 노드 출력만 | 입력/출력·실제 처리 상태; 드래그 중 포인터 옆 CH 범위·개수 한 줄, 놓으면 강조만 유지 |
| 메뉴 / 대화상자 | 열림·focus·disabled·오류·취소 | 안전한 닫기·포커스 복귀; 비활성 이유와 다음 행동 |
| InfoPopover / ShortcutTooltip | 설명·단축키 보조 | 조작 이름은 트리거 자체에 있음; 설명이 핵심 기능을 대신하지 않음 |
| RetryAlert / RecoveryActions | 실패·재시도·대기 | 오류 내용과 실제 복구 행동; 마지막 유효 데이터를 임의로 버리지 않음 |
| Panel / WorkspaceHeader | 섹션 제목·내용·보조 명령 배치 | 의미 있는 제목; 레이아웃 자체는 Tab 대상이 아님 |
| AppInformationButton / ApplicationStatusBar | 앱 정보의 닫힘·열림·확인된 인증·미확인·긴 상태; 한 줄 하단 | 버전 고정·인증 말줄임, 전체 접근성 설명과 툴팁, dialog 관계와 단일 Tab 대상. 행 높이·간격은 Core 앱 상태 토큰 사용 |

이 표는 동작 기준이다. Storybook 상태 예시는 실제 엔진 실행을 증명하지 않는다. 대기 중 요청과 적용된 상태, 런타임 미설치·호환 불가·연결 중·사용 불가를 구별한다. 상세 의미는 제품 문서를 따른다.

### 공통 노드 프레임

| 소유 정의 | 계약 | 실제 소비자 |
| --- | --- | --- |
| `nodeWidths` / `--node-default-width` / `--node-equalizer-width` / `--node-dynamics-width` | 레코더를 포함한 일반 노드 기본 폭 228px, 3밴드 그래픽 EQ 320px, 컴프레서·리미터 420px. EQ·다이내믹스 폭은 그래프 resize 최소폭도 겸함 | Core 그래프와 노드 Storybook |
| Core `audioNodeFrame` / `audioNodeFaceGeometry` | 제목줄 26px·본문 최소 68px·하단 조작/정보 행 24px·경계 1px. 일반 1/2/4채널과 레코더는 접힌 기본 228×120px | `AudioNodeCard`와 `useAudioNodeLayout`의 크기·포트·resize 계산 |
| Core `AudioNodeCard` 구성 | 공개 Core 부품 `AudioNodeIcon`(이미지·글리프 스타일), `AudioNodeEndpointOptions`(`EndpointControl`/`EndpointUpdate`), `AudioNodeMeterBanks`(`AudioNodeMeterBank`; 미터 이름은 번역된 "{name} 채널 N"), `AudioNodeParameterGrid`(`AudioNodeParameterControl` = `ParameterFieldControl` + section/precise)로 조합 | 앱·gain·입출력 장치·레코더·플러그인·내장 효과 노드 |
| Core `AudioNodeCard` / `NodeOptionsSection` | 일반 노드는 Off → Mute → Options, 정상 내장 효과는 Bypass → Options를 배치하고 같은 DOM/Tab 순서 사용. 내장 효과의 저장된 Off/Mute 상태에만 해제 복구 행동을 먼저 제공. Options는 24px 버튼·툴팁·expanded/controls 관계 제공; 펼친 설정은 본문 밖에 유지 | 앱·gain·입출력 장치·레코더·플러그인 노드 |
| Core 정보/조작 행 | 동일 입출력은 `2 CH`, 다르면 `IN 2 / OUT 4` 한 번 표시. gain 값·보조 정보는 같은 하단 행에 배치. 레코더의 시간·Record/Pause/Stop은 `NodeTransportRow` 사용 | 일반 노드와 Feature `RecorderControls` |

사용자가 직접 조절해 저장한 너비는 유지한다. 고채널은 미터·라벨·포트 간격에 필요한 만큼 확장한다. native 플러그인 편집기와 저장된 사용자 크기는 예외로 보존한다. Options 확장으로 포트 좌표를 바꾸지 않는다. 기본 크기를 맞추려고 글자·조작 대상·오류를 축소하거나 숨기지 않는다. 반복 프레임의 숫자는 Core geometry가 소유하고 같은 값을 카드 CSS에 전달하며 사용처에서 다시 계산하지 않는다.

### 컴포넌트별 세부 계약

#### ActionButton과 긴 명령

Core `ActionButton`은 regular 28px·compact 24px를 최소 높이로 유지한다. 부모 폭보다 긴 명령은 같은 글꼴·안쪽 여백으로 줄바꿈하고 높이만 늘린다. `Inline` 다운로드와 `FormActions` 하단 명령은 이 공통 규격을 소비하며, 문구 잘림이나 모달 가로 넘침으로 긴 번역을 처리하지 않는다. Storybook ActionButton `Narrow Long Label`과 실제 UpdateAccess의 390px Pseudo로 확인한다.

#### 아이콘 행동·IconActionButton·Tooltip

익숙한 추가·삭제·열기·재생 같은 기호 행동은 Core 아이콘 버튼/메뉴 트리거와 접근성 이름·hover/키보드 툴팁으로 제공한다. 행동 이름을 아이콘 옆에 반복하지 않는다. 알 수 없는 동작·위험 확인·제출 버튼은 짧은 텍스트를 사용한다. 기호만으로 disabled 사유·실패·적용 상태를 대신하지 않는다.

`IconActionButton`은 실제 native button과 공통 `Tooltip`을 함께 소유한다. 이름은 hover·키보드 focus에서 같은 툴팁으로 표시하며 Tab 대상은 버튼 하나다. PathField·UpdateAccess·ProcessorRecoveryButton·AudioNodeCard의 우회 명령이 이 계약을 소비한다. 사용처는 중복 툴팁이나 native title을 추가하지 않는다.

`Tooltip kind="label"`은 조작 이름, `kind="readout"`은 포트 채널처럼 고정폭 값을 보여주는 툴팁이며 `--tooltip-readout-*` 토큰을 사용한다.

반복되는 아이콘 행동의 접근성 이름과 Tooltip은 인스턴스 제목을 포함한다. Core CollectionItem은 `name: removeLabel`, AudioNodeCard는 `title: Options/Bypass/에디터 행동`, TextField의 비밀번호 버튼은 `필드 이름: 표시/숨기기`를 사용한다. Feature RecorderControls는 표시 이름을 Record/Pause/Resume/Stop·Options·폴더 행동에 전달한다. 전원·음소거·사이드바·모달 닫기도 Core Tooltip과 기존 실제 버튼을 사용하며 Tab 대상은 늘리지 않는다. Storybook Patterns의 Icon Actions가 실제 Core 키 실행·Tab·메뉴 복귀 계약을 검증한다.

#### StatusIndicator와 ProgressIndicator

`StatusIndicator`는 `--status-indicator-size` 7px 점과 `--status-indicator-neutral`을 소유한다. 노드 헤더가 이를 사용하며 툴팁은 왼쪽에 표시한다. `ProgressIndicator`는 `--progress-*` 토큰의 불확정 진행 링과 시각적으로 숨긴 status 레이블을 제공하고, reduced motion에서는 링을 회전하지 않는다. `RuntimeConnection`은 `ProgressIndicator`, `Text kind="heading"`, `Stack align="center"`, `Inline justify="center"`로 구성한다.

#### TextField

`TextField`의 초기값은 `AriaTextField` 상태가 소유하며 Input에 중복 전달하지 않는다. 공개 `value`·`defaultValue`·input 이벤트 계약을 유지하고, 암호 표시 전환은 값·편집 이력을 바꾸지 않는다. LicenseDetails·GraphCreateDialog·NameDialog와 필드·모달 스토리가 같은 Core를 소비한다.

#### InlineNotice banner와 떠 있는 오류

`InlineNotice layout="banner"`는 작업 영역의 가로 오류·경고 표시를 소유한다. 같은 상태 토큰과 공통 banner 안쪽 여백·아래 경계를 사용하며, 라우팅의 저장 경고·배치/그래프 복구·파라미터 실패·플러그인 검색 결과 및 `RetryAlert framed`가 이를 소비한다. 메시지의 시급성이 색과 다르면 `announcement="alert"`로 명시한다. 적용 중 `inert`와 영역의 포커스 추적은 기존 Feature 소유자가 전달하며, 사용처 CSS에서 표시 규격을 다시 정의하지 않는다. `Core/Composition/Form Flow`의 Workspace Banners와 Narrow Workspace Banners는 같은 Core의 일반·긴 번역 상태를 제공한다.

오디오 세션·녹음의 떠 있는 오류도 `InlineNotice tone="critical"`을 사용한다. Feature는 위치·폭·공통 floating layer만 소유하고 표면·경계·글자·안쪽 여백을 중복 정의하지 않는다. 같은 계약은 전체 오디오 일시정지 실패와 라이선스 진입점의 오류에도 적용한다.

#### 복구 행동

RoutingWorkspace는 일반 그래프 revision 변경을 자동 병합한다. 통신 실패·확인되지 않은 적용 결과는 편집을 보존하고 재시도 한 동작을 제공한다. 제한 시간 동안 반복된 실제 충돌만 초안 적용·초안 취소 선택을 제공한다. 배치 조회·저장 실패도 재시도 한 동작을 제공하며 저장된 배치를 고르도록 요구하지 않는다. Core RecoveryActions의 두 선택은 이 지속 충돌처럼 실제 선택이 필요한 소비자에 한정한다.

Core `useRemovedFocusReturn`은 지속되는 작업 영역에서 제거된 복구 조작의 포커스를 호출자가 지정한 기존 입력으로 돌려준다. `TitleField`는 입력 ref를 전달하며 Feature `RoutingWorkspace`는 그래프 복구 영역만 추적하고 그래프 이름을 복귀 대상으로 지정한다. 기존 Tab 대상·외관은 유지한다. `Core/Feedback/Recovery Actions`에서 실제 키 실행·복귀와 좁은 폭·긴 번역을 확인한다.

#### Dialog와 FormActions

대화상자는 Core `Dialog`의 같은 제목·본문·닫기 규격을 사용한다. 폭은 xs 300px·sm 360px·md 420px이며 viewport 여백은 12px이다. 헤더는 기본 높이 32px·13px Medium 제목·상하 2px/좌우 12px 여백, 본문은 12px 여백이다. 긴 제목은 줄바꿈하며 헤더 높이를 확장한다. 이 값은 `tokens.ts`의 dialog 역할과 `ComponentTheme.module.css`가 소유한다.

폼은 입력·설명·오류 다음에 Core `FormActions`를 두고 우측 하단에 취소 → 저장/생성/확인 순서로 정렬한다. 위 간격은 12px, 버튼 사이 간격은 8px이며 좁은 폭에서는 같은 오른쪽 정렬로 줄바꿈한다. 일반 조작 28px·헤더 닫기 24px는 유지한다. 이름·새 그래프·확인 폼은 하단 취소를 사용하고 중복 헤더 닫기를 두지 않는다. 정보·업데이트·라이선스 모달은 헤더 닫기를 사용하며 라이선스의 제출·관리 명령도 같은 FormActions에 연결한다. 라이선스 검증·해제·복구 명령의 행도 공통 `FormActions`의 간격·줄바꿈 규격을 소비한다. 본문 안 복구 행동에만 align=start를 사용한다.

React Aria가 포커스 순환·Escape·닫힘 후 복귀를 담당하고 Core가 busy 상태의 닫힘 차단을 정의한다. NameDialog·ConfirmationDialog·GraphCreateDialog·LicensingDialog·UpdatesDialog·AboutDialog가 이 프레임을 조합한다. Feature는 입력 값과 busy·오류·제출 가능 여부를 담당한다. 입력/오류 묶음은 Core Stack이 간격을 소유하며 사용처 CSS에서 공통 규격을 덮어쓰지 않는다.

#### 노드 프레임·쌓임·포트

캔버스의 일반 노드·선택 노드·옵션을 연 노드는 `canvasLayers`의 1·2·3 쌓임 순서를 사용한다. 열린 옵션은 선택된 다른 노드보다 앞에 있으며, 포털 메뉴와 대화상자는 그보다 위에 표시한다. 옵션을 닫으면 원래 순서로 돌아간다.

포트 묶음 선택은 기본 포트 크기와 간격을 유지한다. 선택 색만 적용하며 외곽 링·확대로 인접 채널이 붙어 보이게 하지 않는다.

Storybook `Core/Audio Nodes/Frame`의 Default·Multichannel·Off·Recording·Failure·Narrow Long Translation은 같은 Core를 비교한다. `App/Workspace`는 이를 사용하는 실제 앱 조합이다.

#### 노드 헤더 동작

내장 효과 Bypass는 공통 헤더의 24px 아이콘 버튼, 툴팁과 aria-pressed로 표현하고 Off와 구별한다. 저장된 Off/Unmute 복구 행동이 성공해서 버튼이 사라지면 Core NodeHeaderActions가 인접한 남은 enabled 헤더 동작으로 포커스를 복귀시킨다. Off 복구 중 버튼이 잠기거나 제거되면 Core useNodeHeaderFocus가 실제 포커스 소유권을 유지하고 인접 명령으로 복귀한다. 사용자가 이미 다른 조작으로 이동하거나 명시적으로 blur한 포커스는 빼앗지 않는다.

노드 헤더 Options와 플러그인 에디터 버튼은 `AudioNodeCard.module.css`에서 공통 `--focus-width / --focus-color`를 소비한다. React Aria의 `data-focus-visible`와 브라우저 `:focus-visible` 모두 같은 중립색 1px 안쪽 윤곽으로 표현한다. 포인터 포커스에 브라우저 기본 파란 윤곽이 남지 않게 한다.

#### Options 펼침과 chevron

세로로 펼치는 노드 Options와 선택 필드의 chevron은 닫힘에서 아래, 열림에서 위를 향한다. 옆으로 여는 하위 메뉴의 오른쪽 방향 표시와 구별한다. 펼침 상태는 `aria-expanded`와 동일하게 반영하고 회전에는 공통 motion 토큰과 reduced-motion 규격을 사용한다.

펼친 Options는 기존 노드 표면·1px 구분선과 상하 `--space-1`(4px)·좌우 `--node-inset`·항목 사이 `--control-gap`을 사용하고, 레벨은 dB 값과 공통 `ValueSlider`를 한 행에 배치한다.

#### 공통 연속값 계산

`Knob`, `ValueSlider`, `NumericValueControl`은 `core/ui/parameterValue.ts`의 범위/로그 계산, step 맞춤, 키보드 step 표, 포인터 capture 드래그 수명 주기를 공유한다. `NumericValueControl`은 커서 이동을 위해 좌우 방향키를 키보드 step 표에서 제외한다. 컴포넌트별 드래그 감도는 아래 각 계약을 따른다.

#### Knob과 조밀한 오디오 파라미터

Core `Knob`과 `ParameterField presentation="knob"`가 반복되는 연속 오디오 파라미터를 소유한다. 세로 입력 행이 과도하게 높아지는 내장 효과에는 노브를 사용하고, 선택형 모드·프리셋은 `SelectField`, 레벨처럼 길이 비교가 필요한 값은 기존 슬라이더를 유지한다. Feature는 값·범위·step·선형/로그 scale·논리 그룹만 전달한다. `AudioNodeCard`는 최대 3열의 반복 배치를 제공하고 1개·2개 그룹은 그 수에 맞춰 열을 배분한다. CSS subgrid로 긴 번역의 라벨 높이를 공유해 노브와 수치 입력의 기준선을 맞춘다.

노브는 48×48 viewBox의 SVG를 24px 표시 지름으로 그리고 실제 조작 영역은 40px로 유지한다. 래스터 이미지·캔버스·CSS 회전 이미지 없이 고DPI와 확대에서도 벡터 선을 유지한다. `--knob-*` 토큰이 크기·선·표면·hover·active를 정의하며 색은 기존 중립 역할을 참조한다. 실제 native range input 하나가 노브의 포커스·역할·현재 값·실제 범위를 소유하고 장식 SVG는 접근성 트리에서 제외한다. 수치 입력은 같은 파라미터의 정확한 값 편집이라는 별도 행동을 소유한다. disabled·단일 중립 포커스·그래프 단축키 경계는 기존 Core 규격을 따른다. 노브 포커스는 24px SVG의 기존 원호 바깥에 아래 중앙이 열린 270도 중립 1px 원호 하나로 표시하고 보이지 않는 40px hit 영역의 큰 사각형을 노출하지 않는다.

위로 드래그하면 증가하며 전체 범위는 160 CSS px 이동이다. Shift 드래그는 1/10 감도로 바꾸되 실제 값의 한 step/px를 넘지 않게 제한한다(주파수 상단에서도 1px 이동당 최대 1Hz). 누적 위치는 연속 실수로 유지하고 전달할 때만 파라미터의 공개 step으로 반올림한다. 로그 주파수도 임의 1000단계로 제한하지 않는다. 방향키는 실제 단위의 한 step, Page Up/Down은 10 step, Home/End는 범위 끝값을 선택한다. 포인터 capture는 해제·취소·disabled 전환·unmount에서 정리하며 window 리스너·타이머를 만들지 않는다. 바이폴라 값은 12시 중심 기준선을 제공하고 Pan/Balance는 L/R 끝값 힌트를 소비한다.

노브의 실제 소비자는 필터·Gate·Expander·Pan·Balance·Delay·Ducking·Reverb·Chorus와 Compressor·Limiter다. Core `Knob/Gain·Frequency·Disabled`, `Numeric Value Control`, Feature `Builtin Effects/Equalizer·Reverb·All Effects`가 공통 구현과 상태를 보여 준다.

#### NumericValueControl

Core `NumericValueControl`은 실제 React Aria NumberField 입력 하나가 직접 입력·방향키·Page Up/Down·Home/End·숫자 드래그·휠을 소유한다. 평소 값+단위 묶음은 내용 너비로 중앙 정렬하고 투명 경계로 표시한다. 노브 아래 숫자는 조밀한 한 줄 높이로 바로 이어지며 별도 row-gap을 더하지 않는다. 24px 벡터 그림의 비어 있는 하단을 제외한 20px compact layout 높이를 공통 소비하여 원호와 값 사이의 시각 간격은 약 3px로 유지한다. 노브 아래 값+단위 글꼴은 Core `--knob-value-font-size`가 caption 10px를 참조하며 라벨과 EQ 숫자표는 기존 크기를 유지한다. 실제 숫자 조작/포커스 대상은 24px 높이를 유지한다. hover·focus 경계는 실제 값+단위 한 줄과 얇은 padding만 감싸며 보이지 않는 24px 조작 영역과 분리한다. 직접 편집할 때는 현재 draft 텍스트 길이를 따라 폭을 늘려 음수·소수·긴 주파수도 가리지 않는다. 범위 최대폭을 미리 예약하지 않는다.

세로 드래그는 실제 공개 step 단위이며 로그 주파수는 연속 비율을 누적한다. Shift는 1/10 감도로 누적하고 전달할 때만 공개 step으로 반올림한다. 휠은 숫자 위에서 한 번당 실제 한 step(Hz 1, dB 0.1, Q 0.001)을 변경하고 Shift 휠은 미세 누적한다. 숫자 입력의 상하 방향키는 항상 실제 한 step이며 좌우는 기본 커서 이동을 유지한다. 컴포넌트의 passive:false 휠 리스너는 줌·페이지 스크롤로 전파하지 않으며 unmount에서 정리한다. `ParameterField`의 노브 아래 값+단위와 EQ 숫자표가 같은 계약을 소비한다.

`NumericValueControl density="compact"`는 표시 값 한 줄의 실제 높이만 배치에 사용한다. 24px 숫자 조작 영역은 그 줄의 중심에 겹쳐 두며 노브 아래에 빈 행을 남기지 않는다. `ParameterField`의 노브 값이 이 계약을 소비한다. `AudioNodeCard`의 compact 설정은 사용자가 노드를 높게 늘려도 남는 세로 공간의 가운데에 배치해 상하 여백을 맞춘다. 네이티브 에디터처럼 늘어나는 콘텐츠는 기존 콘텐츠 공간을 유지한다.

#### GraphicEqualizer와 3밴드 EQ

내장 3밴드 EQ는 `GraphicEqualizer`의 로그 20–20kHz × ±24dB 그래프를 기본면에 항상 표시한다. 세 밴드는 각각 독립 frequency/gain/Q이며 SVG 응답은 실제 DSP와 같은 RBJ bell 세 개의 합이다. 실제 sample rate를 전달하면 각 밴드 frequency를 sampleRate × 0.45 이하로 제한해 계산하고, 알 수 없는 sample rate는 내부 프리뷰 계산에 48kHz를 사용하며 사용자 화면에 계산용 샘플레이트 안내를 표시하지 않는다. 벡터 그래프의 세 핸들은 같은 위치의 native 버튼으로 포커스·밴드 번호/이름·값/단위·선택 상태를 읽는다. 그래프 드래그의 frequency+gain은 `onNodeParametersChange` 한 배치로 전달해 중간 값 손실 없이 한 gesture로 취급한다. 핸들의 좌우 방향키는 1Hz, 상하 방향키는 0.1dB이며 숫자 입력과 동일 값 경로다.

EQ의 1/2/3 밴드 이름과 Hz/dB/Q 숫자 9개는 공통 `NodeOptionsSection`의 기본 접힌 Options 안에 둔다. EQ 전용 chevron이나 별도 노드 카드를 만들지 않는다. 펼침은 노드 포트 좌표를 이동하지 않는다. 값 표의 숫자 드래그·휠·직접 입력으로 각 밴드 Q까지 편집한다. EQ에는 노브를 중복 표시하지 않는다. EQ의 기본 폭과 resize 최소폭은 같은 Core 320px 규격을 소비하며 효과 체인 배치도 같은 폭과 32px 삽입 간격을 사용한다. Q의 UI 표준 기본값은 0.707, 편집 step은 0.001이다.

#### DynamicsMeter와 다이내믹 처리 노드

Core `DynamicsMeter`는 입력·실제 게인 감쇠(GR)·출력의 가로 막대 세 행을 제공한다. 입력과 출력은 L/R 피크를 구별하며 입력 막대의 임계값 선과 수치는 엔진이 적용한 threshold/ceiling에서 가져온다. 감쇠는 makeup 이전 실제 DSP 계수이며 입력과 출력 피크를 빼서 추정하지 않는다. 값 미수신 상태에도 입력·감쇠·출력의 같은 뱅크·막대·눈금·설정 임계값 배치를 유지한다. 측정 부재는 값 자리의 `—`로 표시하고 대기 문구로 미터를 대체하지 않는다. 읽기 전용 접근성 이름에만 측정 부재를 전달하며 실제 측정 0과 구별한다. 모든 미터는 읽기 전용이고 Tab 대상을 추가하지 않는다. `--dynamics-rail-background` 역할은 `--border-default`를 소비해 빈 레일이 라이트·다크 양쪽에서 보이도록 하고, 눈금 글자는 `--dynamics-scale-font-size`(`--meter-scale-font-size`)를 사용한다. Feature는 실제 채널 수와 threshold 계약을 전달하며 개별 노드 스타일로 이를 덮어쓰지 않는다.

`nodeWidths.dynamics`와 `--node-dynamics-width`는 420px 기본·그래프 resize 최소폭을 공유한다. Core `AudioNodeCard`는 다이내믹 처리의 노브를 한 줄에 배치하며 기존의 출력 전용 L/R 미터를 중복 표시하지 않는다. 이전 저장 폭은 오디오 문서를 변경하지 않고 표시 단계에서 정규화한다. 실제 소비자는 라우팅의 컴프레서·리미터와 Core Dynamics Meter 및 Feature All Effects Story다.

#### ParameterField·SelectField 설명과 Options 구역

`SelectField`의 선택 항목별 `description`은 현재 선택의 동작·입출력 관계를 보조 텍스트로 표시하고 React Aria description으로 같은 버튼에 연결한다. 설명에 별도 Tab 대상을 만들지 않는다. `AudioNodeParameterControl.description → ParameterField → SelectField`가 공통 전달 경로이며, 채널 처리 모드가 선택된 실제 합산·복제 관계를 표시하는 데 사용한다. 확인된 값이 바뀔 때 그 항목의 설명도 함께 바꾸며, 명시적인 필드 `description`은 이를 덮어쓴다. 설명과 선택 항목은 Feature의 저장/복원·Undo에서도 함께 유지한다.

`AudioNodeParameterControl.section="options"`는 세부 파라미터를 공통 `NodeOptionsSection`으로 이동한다. 기본 설정은 그대로 표시하고 같은 헤더 chevron이 세부 설정을 펼친다. 리버브는 기본 3노브를 유지하며 알고리즘·공간 크기·선행 지연이 이 계약을 소비한다.

#### 노드 기하 측정

Core `useElementBounds`가 노드 face·header·controls·Options의 ResizeObserver border box 측정을 소유하며 UI 라이브러리의 스타일과 분리한다. width와 height가 모두 같으면 기존 bounds 상태 객체를 유지해 반복 관측이 새 측정 상태나 소비자의 후속 보고를 만들지 않는다. 실제 한 축의 변화와 소수점 기하는 그대로 반영하며 임의 반올림·측정 중단으로 재배치를 숨기지 않는다. border box를 지원하지 않는 관측의 content rect 경로와 detach 시 observer 정리도 같은 계약을 따른다.

#### 장치·드라이버 노드 Options

장치별 설정은 `AudioNodeCard`의 Options에서 편집한다. 별도 Driver 설정 모달이나 전역 설정 버튼을 두지 않는다. DOM·시각·Tab 순서는 이름 → 공통 채널 수 → 샘플레이트 → 비트 심도로 일치시키고 각 항목을 한 번씩 표시한다. 방향별 형식 묶음은 노출하지 않는다. `TextField / SelectField / NativeSampleFormatField`가 공통 필드 규격을 소유하고, Feature는 장치 ID와 Driver의 지원값·읽어 온 값·변경 명령을 연결한다. `NativeSampleFormatField`가 노드와 드라이버 설정의 공통 비트 선택·미확인·불일치·오류 상태를 소유한다. 지원값은 실제 capability의 교집합이며 성공 표시는 readback을 따른다. 같은 장치의 입력·출력 노드는 같은 Driver 설정을 표시한다. 장치 버퍼는 오디오 설정을 따르며 노드에서 편집하지 않는다. 선택값이 하나뿐이고 현재값과 같으면 선택기를 비활성화한다. 설정 변경은 Driver 응답으로 확인하며 실패 시 기존 값을 유지한다. 장치 생성은 그래프의 가상 장치 추가 경로를 사용한다.

#### 그래프 인스턴스의 표시 이름

표시 이름 편집은 저장된 별칭만 입력 값으로 두고, 별칭이 없으면 원래 이름을 placeholder로 표시한다. 원래 이름·빈 값 복원 설명을 본문에 반복하지 않으며 필드의 보이는 label과 접근성 이름은 유지한다. placeholder는 필드 이름과 실제 저장 값을 대신하지 않는다. placeholder 글자는 `--control-placeholder`(`--text-muted`)로 본문과 같은 4.5:1 대비를 유지한다. 실제 값을 지우면 원래 이름을 복원하고 취소하면 변경하지 않는다. 선택에 필요한 장치/플러그인 식별자·채널·형식과 사용 제한·오류는 유지하며, 이름만 반복하는 일반 설명과 동일 버전의 중복 표기는 생략한다.

Core `AudioGraphCanvas`가 이름 텍스트의 더블클릭·바깥 노드의 F2·단일 노드 메뉴를 하나의 `onNodeRenameRequest`로 연결한다. 제목 텍스트에 가짜 button이나 추가 Tab 정지점을 만들지 않는다. 입력·Options·포트의 다른 조작은 편집을 열지 않는다. `AudioGraphNode.displayName`은 제목·접근성 이름·미터/행동 레이블에서 사용하며 `appearance.title`과 runtime label·config·ID는 원래 값이다.

Feature `useNodeDisplayNameEditor`는 Core `NameDialog allowEmpty maxLength={128} placeholder={originalName}`과 `GraphLayout.nodeLabels`를 사용한다. 별칭이 없는 입력은 빈 값이며 placeholder만 원래 이름이다. 빈 이름은 원래 이름 복원, Escape는 취소이며 반환 포커스는 같은 노드 바깥 그룹이다. GraphLayout의 저장/자동 병합은 오디오 제출과 분리한다. NodeDisplayNames OriginalPlaceholder와 NameDialog EmptyReset이 같은 계약을 소비한다.

#### 앱 노드 아이콘

앱 노드는 발견된 **실제 앱 아이콘을 우선** 표시한다. macOS의 앱/NSWorkspace 아이콘과 Linux의 앱 메타데이터·시스템 아이콘 테마에서 얻은 이미지를 `iconDataUrl → appearance.iconUrl → AudioNodeCard`로 전달한다. 없거나 이미지 로드에 실패한 경우에만 공통 APP fallback을 쓴다. 앱 로고를 임의로 다시 그리지 않고, 아이콘이 바뀌어도 선택한 앱과 노드 이름은 유지한다. Core `AudioNodeIcon`이 이미지·글리프 스타일과 `--node-icon-size` 14px를 소유한다.

#### 앱 셸과 하단 상태

앱 셸은 창 높이에 고정하고 바깥 스크롤과 바운스를 막는다. 그래프 목록은 내부 스크롤과 경계 차단을 소유한다. Storybook 카탈로그의 문서 스크롤은 유지하고 `App/Workspace/Long Graph List`로 긴 목록을 확인한다.

`shellBreakpoints.overlaySidebar` 720px 이하에서는 그래프 사이드바가 캔버스 위에 겹쳐 표시된다. 이 폭에서는 닫힌 상태로 시작하고, viewport가 좁아질 때와 선택한 그래프가 실제로 활성화된 뒤 닫힌다. 전환이 실패하면 사이드바와 오류를 그대로 둔다. 사이드바 안에 있던 포커스는 닫힐 때 사이드바 토글로 옮긴다. 토글은 캔버스 높이 중앙의 서랍 손잡이형 탭 하나다. 닫힘에서는 창 왼쪽 끝, 열림에서는 사이드바 경계에 붙어 같은 높이에서 열고 닫는다. 앱 헤더에는 두지 않으며 앱 헤더는 열림·닫힘과 관계없이 사이드바 폭을 유지한다. 닫으면 캔버스만 넓어지고 헤더 항목과 그래프 제목은 움직이지 않는다.

하단 왼쪽은 버전·인증 상태를 표시하는 앱 정보 버튼, 오른쪽은 compact 오디오 조작 하나다. 27px 한 줄에서 버전·인증은 같은 12px 글자와 기준선을 사용한다. 버전은 유지하고 긴 인증 상태만 말줄임하며 전체 버전·상태는 접근성 설명·툴팁에 남긴다. 처리 중 설명으로 행이나 버튼 크기를 늘리지 않는다. 장치별 형식은 해당 장치 Options에 둔다. 장치 I/O 버퍼는 오디오 설정을 따른다. 하단 버전·인증 상태 버튼은 라이선스 상태·활성화·온라인 확인·기기 비활성화만 여는 인증 진입점이다. native About는 제품 버전·제작자·지원 연락처·접힌 약관을 제공하며 인증 명령·오디오 명령·업데이트 명령을 섞지 않는다. 전역 오디오 제어는 하단 오른쪽 일시정지·재개 한 곳에 둔다.

하단의 `--app-status-*`, `--app-info-*`는 Core `tokens.ts`가 정의하고 기존 Provider가 CSS에 공급한다. Core `AppInformationButton`이 글자·기준선·말줄임·호출자가 지정한 dialog 관계를, `ApplicationStatusBar`가 높이·안쪽 간격·표면·좌우 배치를 소유한다. `LicenseAccess`는 버전·번역된 인증 상태·열기 동작을 전달하고, `StudioScreen`은 하단 정보와 오디오 Feature를 조합하며 자체 레이아웃 요소로 화면 grid 위치만 지정한다. `Core/Application/App Information`의 정상·무료·미확인·열림·긴 상태·키 실행과 `App/Workspace`의 실제 소비 조합을 확인한다.

#### 그래프 자동 맞춤

그래프의 자동 맞춤은 최대 100%로 제한하고, 사용자가 저장한 카메라와 직접 확대는 유지한다.

## 조작과 접근성

[조작·포커스·접근성 계약](design/interaction-accessibility.md)을 따른다. Tab 대상, 키보드 행동, 읽혀야 할 이름·역할·상태를 요소별로 정의한다. 포커스 대상이 아닌 미터·상태도 접근성 대상이며, 노드의 포커스 소유자는 하나다.

- React Flow의 노드·연결 키보드 설명과 이동 live region 문구는 `ariaLabelConfig`로 번역한다.
- 케이블 이름은 "{source} CH n → {target} CH m"이다. 한쪽에서 채널 번호가 반복될 때만 버스/포트 이름을 더하고, 이름도 반복되면 순번을 더한다.
- 포트는 "{port}, CH n" 이름의 image로 읽힌다.
- 행동 이름·기본값과 같은 정보를 설명 문장으로 반복하지 않는다. 무료 제한 안내는 해당 행동의 한도만 제공하며 오류·단위·장치/플러그인 식별·녹음 상태·복구 이유는 유지한다. NodeTransportRow는 상태와 설명을 aria-description에도 함께 제공한다.

## 제품 아이덴티티

2026-10-12에 **Plex Optical 워드마크를 동결**했다.
[공식 SVG·사용 규격](../branding/identity/wordmark/README.md)과
[단일 AI 마스터](../branding/wordmark-study/plex/mixrelay-plex-wordmark.ai)를 보관한다.
M/R/l 높이와 심벌 실루엣은 1:1, 기준선은 공통, 심벌–글자 간격은 0.26H다.
y 꼬리와 곡선 overshoot를 보존하고, 실제 배치 크기는 시안에 맞춘다.
라이트는 빨강+Ink, 다크는 같은 빨강+Inverse이며 양 테마의 단색·아웃라인도 제공한다.
작은 UI는 채움형을 사용한다. 워드마크 소유 원본은 `branding/identity/source/wordmark.json`이다.
현행 제품 구현은 아래의 심벌 전용 계약이다. 워드마크 적용 시 Core에서 표현 계약을
확장하고 Storybook·제품 화면에 같은 규격을 반영한다.

2026-10-11에 확정한 Optical Grid 실루엣은 `branding/identity/source/symbol.json`,
색·크기는 Core `tokens.ts`의 `brandColors / brandSizes`가 소유한다.
[브랜드 규격](../branding/identity/README.md)에 보호 여백·최소 크기·채움·아웃라인·반전·음각을 정의했다.

Core `BrandIdentity`는 primary·neutral·inverse, compact·standard·display와 선형 아웃라인을 제공한다.
모든 크기는 정사각형 심벌만 표시한다. primary는 고정 빨강, neutral은 테마의 텍스트 역할을 소비한다.
`--brand-primary / --brand-ink / --brand-inverse`는 일반 상태 색과 구별한다.
아웃라인은 display 크기만 사용하고 작은 헤더·트레이에는 채움형을 사용한다.
`ProductHeading`이 h1·배치·접근성 이름을 소유하며 로고는 Tab 대상을 추가하지 않는다.

실제 소비자는 `StudioScreen` 헤더, `RuntimeConnection` 설치·연결 안내, Storybook
`Core/Identity/Brand`와 `Foundations`, 앱·트레이 생성 자산, 웹 헤더·푸터·favicon이다.
펼친/접힌 헤더 모두 심벌만 표시하고 h1 접근성 이름 MixRelay를 유지한다.
`pnpm branding:build / pnpm desktop:icons`로 같은 원본에서 자산을 생성한다.
브랜드 적용은 앱 노드의 외부 아이콘이나 녹음·오류·실행 상태를 바꾸지 않는다.

## Storybook 문서와 운영

React 코드가 컴포넌트의 기준이고 Storybook은 같은 Provider·토큰·컴포넌트를 직접 사용한다. Storybook의 `Design System`이 디자인 시스템의 문서 진입점이다. `pnpm storybook`으로 열고 `Design System/Overview → Foundations → Components → Patterns → Pages → Review` 순서로 탐색한다. 값과 조작의 소유자는 Core이며, 문서와 실행 예시는 같은 토큰·Provider·컴포넌트를 소비한다. 제품 범위와 진행 상태는 `product.md`에만 기록한다.

| 문서 | 작성·확인 기준 |
| --- | --- |
| Overview | 제품 목적, 표현 방향, 계층별 소유권, 문서 탐색과 변경 순서 |
| Foundations | 토큰 객체에서 직접 생성한 팔레트·의미 역할·테마·컴포넌트 변수, 타이포·서체·간격·밀도·아이콘·모션·브랜드 규칙 |
| Components | 목적·사용 구분·구조·공개 속성/기본값·독립 상태·토큰/규격·조작/접근성·소스/실제 소비자·실행 예시 |
| Patterns | 입력·제출·오류 복구·메뉴·모달·그래프/노드 조작의 순서와 소유 경계 |
| Pages | 시작·연결·작업 화면·장치·라이선스/정보의 과업, anatomy, 정상·빈 상태·대기·실패·잠금, 좁은 폭·긴 번역 |
| Review | 변경 책임, 화면·키보드 리뷰, 자동 검사·fixture·보조 기술·설치본의 검증 범위. 날짜별 리뷰 기록은 [design-system-reviews.md](validation/design-system-reviews.md)에 둔다 |

문서 소스는 `apps/control-ui/src/core/design-system`에 있다. MDX는 기존 CSF 스토리를 연결해 실제 Core·Feature·Screen을 렌더한다. 기존 story ID는 유지한다. 문서 테마와 Canvas의 테마·언어를 각각 확인하며, Canvas는 독립 iframe으로 문서의 스타일과 실행 환경을 분리한다. `build-storybook`은 공개 Core 컴포넌트의 계약·소스·독립 문서·실제 스토리와 문서의 고정 링크를 검사한다.

컴포넌트 문서는 목적·사용 구분·구조·공개 속성·상태·부위별 토큰/규격·조작/접근성·소스/소비자·검증 범위를 같은 순서로 기록한다. default·hover·focus-visible·pressed/selected·busy·disabled·error 중 실제 지원하는 축과 의미 있는 조합을 설명한다. 선택·포커스·실행 상태는 별개다. 문서에 없는 새 상태나 API를 추가하면 해당 계약과 실행 예시를 함께 작성한다. 목적은 짧은 문장, 규격은 표, 상태는 실제 예시로 설명하고 미사여구와 검증되지 않은 평가는 쓰지 않는다.

`Core/Design System/Foundations`에서 전체 팔레트·라이트/다크 역할·타이포와 조작 밀도·확장한 간격 스케일·실제 로고와 앱/트레이 아이콘을 나누어 확인한다. `Design Language`는 상태·명령·입력·제품 조작 아이콘을 제공한다. `Features/Builtin Effects`와 `App/Workspace`는 실제 라우팅 및 앱 조합을 사용하며, 별도 프리뷰 UI나 DSP를 만들지 않는다. 고정 계측 fixture는 설정 revision이 바뀌면 비운다.

`Core/Design System/Component Catalog`는 주요 공통 컴포넌트의 조합을 한 화면에서 비교한다. `Overview`, `Disabled`, `Narrow Long Translation`에서 일반 명령·녹음·Off/Mute·입력·읽기 전용·오류·상태·메뉴·노드·모달을 직접 조작한다. 세부 상태는 각 컴포넌트의 Storybook을 기준으로 한다.

`Core/Design System/System Controls`는 목록의 선택·삭제, 제목·경로·비밀번호·확인 입력과 일반·disabled·busy 상태를 제공한다. 좁은 번역 스토리는 실제 제한 폭을 적용한다. Core의 Bit Depth 예시는 조작한 값을 같은 필드의 readback에 반영한다. 이는 로컬 상태 사용례이며 장치 명령의 완료를 증명하는 미터·상태를 만들어 내지 않는다.

`Core/Composition/Form Flow`는 입력·선택·제출·실패·대기·긴 번역·좁은 폭과 내용 그룹을 실제 Core로 구성한다.

`App/Workspace`는 실제 `App → Feature → StudioScreen` 조합에 검증용 포트를 연결한다. 전체 작업 화면·무료 상태·좁은 폭과 긴 번역을 확인하며 실제 오디오나 파일을 변경하지 않는다. 카탈로그와 앱이 같은 Core를 사용한다.

## 변경과 검증

1. 디자인 요청을 받으면 이 문서와 리뷰·조작 계약을 읽고, 현행 `tokens.ts` → `MixRelayProvider` / `ComponentTheme.module.css` → 해당 Core 컴포넌트 → 실제 사용처와 Storybook을 확인한다. 요청을 담당할 기존 역할·계약과 변경 소유자를 먼저 정한다.
2. 값의 변경은 토큰, 공통 표현은 테마, 컴포넌트의 표현·상태는 해당 Core 계약부터 수정한다. 의미가 같으면 재사용하고 새 의미는 Core 계약으로 추가한다. 문서만 수정하거나 사용처에 새 스타일을 붙이는 것으로 시스템 변경을 대신하지 않는다. 앱과 Storybook은 같은 정의를 소비해야 하며 새 표현·상태는 Storybook에 추가한다.
3. 동작에 맞는 자동 검사와 키보드·좁은 화면·긴 번역·오류/disabled 상태의 화면 검증을 수행한다.
4. 완료 보고에는 변경한 시스템 정의, 이를 소비하는 앱 사용처와 Storybook, 검증 범위를 적는다. 리뷰는 관점별로 중대한 문제만 남긴다. 실제로 확인하지 않은 동작은 미확인으로 기록한다.

### 자동 검사

- `pnpm --filter @mixrelay/control-ui check:boundaries`는 외부 UI 라이브러리의 Core 경계와 색 값 중앙화, 사용처의 스타일 우회 금지를 강제하고 금지/허용 fixture로 검사 자체도 검증한다. 숫자 `z-index`, 숫자 시간(`var()` fallback 포함), 숫자 글자 크기·글꼴 family(`font` 단축 속성과 `var()` fallback 포함), 토큰·스타일시트·`@property`·인라인 스타일에서 정의하지 않은 `var(--x)`를 거부한다. React Aria가 공급하는 `--trigger-width`·`--available-height`는 허용한다.
- 문서 검사기는 `src/core` 어디서든 export한 모든 컴포넌트에 계약을 요구한다(`core/design-system` 문서 도우미 제외). 계약에 적힌 각 소비자가 해당 컴포넌트를 런타임 import(타입 import 제외, export 이름 기준)하는지, 적힌 각 테스트가 그 컴포넌트나 적힌 소비자를 import하는지 확인한다. UI 감사(`audit:ui`)는 Commands·Fields 그룹과 `interactive: true` 계약을 상호작용 컴포넌트로 집계한다.
- `pnpm --filter @mixrelay/control-ui test:a11y`(`src/core/design-system/playgrounds.a11y.test.tsx`)는 모든 Core Playground 스토리를 Storybook preview로 렌더하고 포털을 포함한 document 전체를 axe로 검사해 위반이 있으면 실패한다. 무거운 렌더가 시간에 민감한 동작 테스트와 경쟁하지 않도록 일반 `test`와 분리된 단계로 실행하며 UI `check`에 포함된다. color-contrast와 region 규칙은 실제 브라우저 검증에 맡기며, 예외는 `parameters.a11y.test='off'`로만 지정한다.
- `pnpm check:ui-quality`와 `pnpm --filter @mixrelay/control-ui check`는 스타일·타입·회귀·앱 및 Storybook 빌드를 검사한다. UI `check`는 문서 검사기의 자체 테스트도 실행한다. 계약 API 발췌는 `docs:api <source> <Name>`, 표준 컴포넌트 문서 페이지는 `docs:page <Name> [--force|--stdout]`로 만든다.
- 자동 검사의 통과는 접근성 전체 적합이나 설치된 오디오 동작 검증을 대신하지 않는다.

## 근거

- 문서 구조·운영: Storybook 공식 [문서화 가이드](https://storybook.js.org/blog/how-to-document-your-design-system-best-practices-tools/)·[구조 가이드](https://storybook.js.org/blog/structuring-your-storybook/)·[Docs addon 활용](https://storybook.js.org/blog/4-ways-to-document-your-design-system-with-storybook/).
- 색과 테마 방향: [Braun 기능색](https://www.braun-audio.com/en-KR/stories/design/braun-colour-choices/), [Rams의 색 사용](https://rams-foundation.org/magazin/texte/dieter-rams-und-die-farben/), [HLD 4 사진](https://www.braun-audio.com/en-KR/stories/wp-content/uploads/2022/06/hld-4-hairdryer-braun.jpeg), [OP–XY](https://teenage.engineering/products/op-xy), [2013 Mac Pro](https://www.apple.com/ie/newsroom/2013/10/23Introducing-the-All-New-Mac-Pro-The-Most-Radical-Mac-Ever/), [Carbon 테마 구조](https://www.carbondesignsystem.com/building-blocks/foundations/themes/overview). 소재색과 작은 기능색의 관계는 MixRelay의 해석이며 제품 외형을 복제하지 않는다.
- 토큰과 사용처 규칙: [Carbon 색 역할·테마·컴포넌트 토큰](https://www.carbondesignsystem.com/building-blocks/foundations/color/overview), [Carbon 소개](https://www.carbondesignsystem.com/getting-started/about-carbon). 사용처 규칙은 이 구조를 MixRelay에 적용한 팀의 결정이다.
- 조합 계약: Carbon의 [간격과 Stack](https://www.carbondesignsystem.com/building-blocks/foundations/spacing/overview), [테마와 컴포넌트 토큰](https://github.com/carbon-design-system/carbon/blob/main/packages/styles/docs/sass.md#component-tokens), [Notification 사용 구분](https://www.carbondesignsystem.com/building-blocks/core/components/notification/guidelines). 이 계약과 조밀한 오디오 UI의 값은 MixRelay의 결정이다. Carbon 컴포넌트 전체를 복제하지 않고 확인된 사용처에 필요한 계약을 추가한다.
- 컴포넌트 문서: Carbon의 [컴포넌트 완료 기준](https://carbondesignsystem.com/getting-started/contributing/component-checklist/), 사용 지침·시각 명세·접근성 문서 구성과 역할/상태 관리를 참고한다. 값과 제품 계약은 MixRelay가 결정한다.
- 필드·피드백·조작: [Monome](https://monome.org/)의 제한된 색과 텍스트 위계, [TX–6](https://teenage.engineering/products/tx-6)의 반복 조작과 상태 구별, [Vitsœ](https://www.vitsoe.com/us/about/good-design)의 간결함, [Ableton](https://www.ableton.com/en/manual/live-concepts/)의 오디오 상태 구별을 참고했다. 실제 필요한 필드·피드백·합성 규격을 보강하며 장식용 컴포넌트나 별도 테마 체계를 추가하지 않는다.
- 노브와 EQ: [TX–6 공식 사진](https://teenage.engineering/products/tx-6)의 반복 축, [Ableton EQ Eight 공식 화면](https://www.ableton.com/en/manual/live-audio-effect-reference/#eq-eight)의 주파수/게인 그래프와 수치 관계, [Monome](https://monome.org/)의 절제된 표기를 MixRelay의 토큰과 공통 상태에 적용한다.
