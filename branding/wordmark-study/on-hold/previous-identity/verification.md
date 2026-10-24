# 적용·검증 기록 — 2026-10-11

## 적용

선택한 B 심벌의 path를 당시 원본과 문자열로 비교해 동일함을 확인했다.
기하를 `source/symbol.json`으로 승격하고 Core `brandColors / brandSizes`를 추가했다.
study 생성기도 같은 원본과 색을 소비하게 바꾸고 중복 워드마크 원본을 제거했다.

Core `BrandIdentity / ProductHeading`, StudioScreen 헤더, RuntimeConnection,
Storybook Brand·Foundations, 데스크톱 앱·트레이, Control UI favicon,
웹사이트 헤더·푸터·favicon·기존 PNG를 연결했다. 플랫폼에 발견된 외부 앱 아이콘은 그대로 전달한다.

Illustrator에는 다른 작업자의 문서와 별개인 `mixrelay-brand-system.ai`를 생성해
1200×1960 RGB 대지 하나로 저장했다. 네이티브 AI 데이터와 저장 완료 상태,
로고·컬러·아웃라인·반전·음각·여백·제품 사용례를 실제 Illustrator 프리뷰에서 확인했다.
SVG는 재생성 원본, PNG는 문서 열람용 스냅샷, AI는 별도 편집 파일이다.

## 리뷰

- 색 정의 · study 중립색이 제품 팔레트와 달랐다 · 자산별 색이 어긋날 수 있었다 ·
  Core brandColors에 연결해 red-500·neutral-900·graphite-50·neutral-50·graphite-900을 함께 소비한다.
- 아웃라인 · 작은 크기에서는 두 경계선이 가늘어진다 · 심벌 개구부를 읽기 어려워진다 ·
  64px 캔버스 이상으로 제한하고 헤더·트레이는 채움형을 사용한다.
- 헤더 · 좁은 헤더에서 테마 조작과 함께 들어가야 한다 · 글자와 버튼이 겹칠 수 있다 ·
  compact 대문자 높이 10px·간격 8px을 정의하고 접힌 헤더는 접근성 이름을 유지한 심벌만 표시한다.
- 접근성 · 로고 자체에 실행할 행동이 없다 · Tab 정지점을 늘리면 불필요한 이동이 생긴다 ·
  h1 이름 MixRelay와 SVG 이미지 이름을 유지하며 focusable=false, 별도 Tab 대상 없음으로 구현한다.

브라우저에서 라이트·다크 헤더, 360px 긴 번역 화면의 로고·테마 조작 간격을 확인했다.
Tab으로 로고를 건너뛰어 테마 조작에 진입하고 Enter·방향키·Enter로 다크를 적용한 후
같은 트리거로 포커스가 돌아오는 것을 확인했다. 접힌 제품 이름은 회귀 검사로 확인했다.
기존 상태·명령 표현에 새 브랜드 상태 색을 배정하지 않았다.

## 자동 검사

| 검사 | 결과 |
| --- | --- |
| `pnpm desktop:icons` | 앱·플랫폼·트레이 파일 생성 완료 |
| 독립 SVG 20개 | XML·외부 참조 없음·심벌당 닫힌 path 하나·선택 실루엣 동일 |
| `node branding/logo-study/build.cjs` | 제거한 폴더 의존 없이 재생성 |
| `node apps/website/check-site.mjs` | 2페이지의 구조·자산·링크 이름 검사 통과 |
| `pnpm check:ui-quality` | 통과 |
| `pnpm --filter @mixrelay/control-ui check` | 경계·타입·2,015개 테스트 통과, 전체 coverage 기준 실패 |
| 별도 제품 빌드·Storybook 빌드 | 모두 통과 |
| 마지막 브랜드·Studio·RuntimeConnection 회귀 검사 | 3파일 18개 테스트 통과 |

BrandIdentity와 tokens의 문장·분기·함수·줄 커버리지는 모두 100%다.
전체 check의 실패는 이 변경에서 수정하지 않은 `GraphApplicationQueue.ts`,
`GraphLayoutController.ts`, `builtinEffects.ts`에 남은 미실행 분기·문장 때문이다.
전체 문장 99.91%, 분기 99.83%, 함수·줄 100%이며 저장소 기준은 전 항목 100%다.
다른 작업 중인 라우팅 코드는 이 브랜드 작업에서 수정하지 않았다.

실제 설치된 앱·Dock 캐시·시스템 메뉴바·보조 기술 읽기는 이 작업에서 검증하지 않았다.
중복 조사의 원래 조회 범위는 logo-study/research/review.json에 유지한다.
