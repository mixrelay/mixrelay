# MixRelay identity

2026-10-12에 **Optical 워드마크를 동결**했다. 2026-10-11에 확정한 Optical Grid
심벌과 조합하며, 심벌 형태와 Core 색 체계는 유지한다.
현행 제품의 심벌 전용 구현과 동결 에셋의 준비 완료를 구별한다.

- [동결 워드마크 SVG · 사용 규격](identity/wordmark/README.md): 라이트·다크 컬러, 단색·반전, 실제 아웃라인 14종.
- [단일 워드마크 AI 마스터](wordmark-study/plex/mixrelay-plex-wordmark.ai): 승인한 Optical 세 대지.
- [동결 에셋 검토판](identity/wordmark/preview.png): 실제 SVG 렌더링과 축소 표본.

![MixRelay 로고 체계](identity/preview.png)

- [심벌 키트 ZIP](identity/mixrelay-symbol-kit.zip) · [키트 안내](identity/kit/README.md): SVG·투명 PNG·앱·트레이 아이콘.
- [브랜드 규격](identity/README.md): 색·형태·여백·최소 크기·사용 규칙.
- [Illustrator 문서](identity/mixrelay-symbol-kit.ai): 편집 가능한 컬러·아웃라인·반전·기하·사용례.
- [SVG 가이드](identity/mixrelay-brand-system.svg): 원본에서 재생성하는 벡터 문서.
- [워드마크 설계 근거](wordmark-study/typography-brief.md): Plex Optical의 글자 보정과 조합 비례. 초기 세 안과 DIN 추천은 철회했다.
- [보류한 레터링 A·B](wordmark-study/lettering-v2/README.md): 같은 글자 구조에서 R·e·l·a·y를 달리한 새 시안, 기존 형태·Sonolog·축소 비교.

형태는 `identity/source/symbol.json`과 동결 글자 원본 `identity/source/wordmark.json`,
색과 UI 크기는 `apps/control-ui/src/core/theme/tokens.ts`의 `brandColors / brandSizes`가 소유한다.
`build.mjs`가 같은 정의에서 SVG와 가이드를 생성한다.
`icon.png`는 앱용 1024px 파생 자산, `tray-white.svg`는 외부 참조 없는 벡터 템플릿이다.

`pnpm branding:kit`으로 아이콘과 휴대용 ZIP 키트를 생성한다.
`pnpm branding:build`로 SVG와 앱 PNG·웹 PNG를 생성한다.
`pnpm desktop:icons`는 앱·Dock·설치·플랫폼 아이콘과 트레이 32/64/512px를 이어서 생성한다.
Tauri 개발·빌드 훅도 같은 명령을 소비한다. 직접 Cargo 빌드를 위해 생성 자산을 보관한다.
트레이는 64px PNG를 컴파일 시 내장하며 macOS가 template 색을 결정한다.

Core `BrandIdentity / ProductHeading`, 앱 헤더, 설치·연결 안내, Storybook,
데스크톱 앱·트레이, 웹 헤더·푸터·favicon이 이 시스템을 소비한다.
사용 규격은 [디자인 시스템](../docs/design-system.md)에 연결했다.
다른 앱 노드의 실제 아이콘은 해당 앱과 플랫폼이 소유한다.

[logo-study](logo-study/README.md)는 선택 과정과 중복 조사 증거를 보관한다.
제품 자산의 기준은 `identity/`이며 조사 폴더에서 별도 기하·색을 결정하지 않는다.
그래픽 심벌 선택·제품 소스 적용은 완료했다. 조사에서 확인한 범위와 미확인 범위는 기록에 구별해 남겼다.
