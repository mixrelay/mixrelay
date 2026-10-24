# 아이덴티티 검증 기록

## Optical 워드마크 동결 · 2026-10-12

사용자 승인한 Optical 한 안을 AI 마스터와 SVG 14종으로 보관했다.
[에셋과 사용 규격](wordmark/README.md), [실제 SVG 검토판](wordmark/preview.png),
[구조·해시 검증 결과](wordmark/verification.json)를 확인한다.

- 여덟 글자의 path가 승인한 B의 정규화 윤곽과 정확히 일치한다.
- M/R/l cap·심벌 높이 76, 공통 기준선, 간격 19.76 = 0.26H를 확인했다.
- 라이트·다크 컬러, 단색·반전, 글자 단독과 실제 아웃라인 14종 모두 SVG 렌더링에 성공했다.
- 검토판을 직접 읽고 잘림·테마별 위치 이동·윤곽 누락을 확인했다.
  채움 H=16/24/32px와 아웃라인 H≥64px 표본을 확인했다.
- SVG는 path만으로 로고를 표현하며 텍스트·글꼴·외부 참조·바탕 면에 의존하지 않는다.
- 모든 색은 현재 Core brandColors와 일치한다. AI·원본 폰트 해시를 보존했다.
- 재생성 전후 14개 SVG 해시가 일치한다. AI를 덮어쓰지 않는다.

이번 작업은 에셋·규격 동결이다. Core/UI 구현 변경과 실제 설치본 검증은 수행하지 않았다.
아래는 2026-10-11의 심벌 전용 제품 적용 기록이며 당시 검사 결과다.

## 확정 그래픽 심벌 적용 검증 v1.1

2026-10-11. 사용자 결정: Optical Grid 그래픽 심벌만 확정, 워드마크 전체 보류.

## 변경과 사용처

Core `BrandIdentity`의 워드마크 원본 import·글자 path·가로 조합 계산·symbolOnly 분기를
제거했다. 모든 크기가 같은 정사각형 심벌이다. `ProductHeading`은 펼침 상태에 따른
로고 변경 없이 h1 접근성 이름 MixRelay를 제공한다. Core 색·16/24/64px 토큰은 유지한다.
StudioScreen, RuntimeConnection, Identity·Foundations Storybook이 이 계약을 소비한다.
웹 두 페이지의 헤더·푸터도 심벌로 변경하고 홈 링크 이름을 유지했다.
앱과 웹 favicon, Dock·패키지·제거 앱 아이콘, Driver 트레이는 같은 원본에서 생성한다.

이전 배포 워드마크와 가로 로고·이전 브랜드 AI/미리보기는
`../wordmark-study/on-hold/previous-identity/`에 보관했다. 후보 레터링도 보류 표시했다.
공식 identity와 키트에서 워드마크는 제외했다. 심벌 source SHA-256은
`5db20c03858a504a88c27edf56e361431605685c54b37f1ced9e0255c41f5cf3`으로 변경 전과 같다.

`mixrelay-symbol-kit.ai`는 Illustrator에서 새로 저장한 그래픽 전용 문서다.
대지와 원본 SVG 배치를 확인하고 hasUnsavedChanges=false를 확인했다.
다른 작업자의 rawsector 문서는 수정하지 않았다.

## 자동 검사

- `pnpm desktop:icons`: 성공. SVG→1024 PNG→플랫폼 앱 아이콘, 트레이 32/64/512 생성.
- `pnpm check:ui-quality`: 422 파일 검사 통과.
- `pnpm --filter @mixrelay/control-ui check`: 경계 검사·타입 검사 통과, 159 파일/2015 테스트 통과.
  전체 명령은 기존 다른 변경의 statement 99.91%·branch 99.83% 커버리지 때문에 실패했다.
  미커버 범위는 GraphApplicationQueue(497/559/589/590), GraphLayoutController(137/215),
  builtinEffects(259/264)이며 이번 그래픽 변경에서 수정하지 않은 파일이다.
  함수·줄 커버리지는 100%다. 검사 실패를 전체 통과로 보고하지 않는다.
- 중단된 후속 빌드는 별도 실행: `pnpm --filter @mixrelay/control-ui build`,
  `pnpm --filter @mixrelay/control-ui build-storybook` 모두 통과.
- `node apps/website/check-site.mjs`: 두 페이지의 구조·이름·로컬 자산 검사 통과.
- 변경 범위 `git diff --check`: 통과.
- 심벌 PNG 59종의 실제 치수·alpha 채널, 반전의 투명 바탕·음각의 투명 구멍을 확인했다.
  87개 파일의 ZIP에 AI·기하 원본·실제 팔레트가 들어 있고 워드마크/가로 조합 자산이 없는지 확인했다.
  빌드된 앱 JS에서도 이전 워드마크 path가 없다.

## 실제 브라우저 확인

Storybook의 Core Product Header와 Screens Studio Long Translation에서 확인했다.
16·24px SVG가 정사각형·한 path이며 워드마크가 없다. 360×780 화면의 pseudo 긴 번역,
라이트·다크, 펼친/접힌 헤더에서 심벌 잘림과 페이지 가로 넘침이 없다.
양쪽 상태 모두 h1 이름 MixRelay를 유지한다.
Tab은 로고를 건너뛰고 테마 버튼에 도달한다. Enter로 메뉴를 열고 위 화살표·Enter로
라이트 전환을 실행했다. 사이드바 Enter 토글 후 역방향 Tab도 테마 버튼으로 돌아온다.
로고에 포커스가 가지 않는다. 보조 기술의 실제 음성 읽기는 별도 미확인이다.

## 리뷰

- 앱 헤더 · 미확정 레터링이 공식 표기처럼 표시됨 · 승인 범위 혼동 · Core를 심벌 전용으로 수정.
- 공식 키트 · 이전 가로 로고와 승인 심벌이 같은 폴더에 존재 · 배포 시 혼용 가능 · 이전 파일을 보류 폴더로 이동, 심벌 전용 manifest·ZIP 구성.
- 좁은 긴 번역 · 360px 라이트/다크에서 심벌과 테마 조작이 분리되고 가로 넘침 없음 · 이름과 조작 유지 · 추가 수정 없음.

현재 적용은 소스·생성 자산·브라우저·프런트엔드 빌드 기준이다.
설치된 앱 교체, 실제 Dock 캐시·메뉴바 표시·운영체제별 설치 패키지 실행은 이번 검증 범위가 아니다.
