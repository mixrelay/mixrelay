# 디자인 시스템 검수 기록

Storybook `Design System/Review`의 날짜별 기록을 보관한다. 각 기록의 개수·coverage 수치는 그 시점의 값이며 현재 상태를 뜻하지 않는다. 현재 결과는 `pnpm --filter @mixrelay/control-ui check` 출력이 기준이고, 제품의 진행 상태는 [product.md](../product.md)에 둔다.

## 2026-10-12 Carbon 기준 검수

Carbon의 [컴포넌트 검수표](https://www.carbondesignsystem.com/getting-started/contributing/component-checklist)를 기준으로 토큰·실제 코드 계약·상태·조작·문서의 대응을 확인했습니다. 색·밀도·형태는 MixRelay 계약이며 Carbon 인증이나 전체 실환경 검증을 의미하지 않습니다.

| 검사 | 결과와 범위 |
| --- | --- |
| 문서 | 기준 문서 6개·독립 컴포넌트 문서 61개의 브라우저 렌더 확인 |
| 빌드 계약 | 계약 61개·공개 Core UI/audio-graph export 57개의 API·타입·기본값, 소비자·테스트 경로, 독립 문서·상태 스토리·고정 링크 대응 확인 |
| 화면·조작 | 스토리 217개의 실제 렌더; 기본 198개 Light/Dark, 추가 상태 예시, 문서 테마·검색·Enter 선택, 390px Pseudo 복구 화면. 아이콘 명령 Tab·Enter·툴팁, 연속 revision 자동 병합, 재시도 후 편집·포커스 보존 확인 |
| 자동 검사 | UI 품질·Core 경계·타입·앱/Storybook 빌드 통과; 160개 파일의 동작 검사 2,042개 통과 |
| 전체 UI 게이트 | statement 5,936/5,936 · branch 4,988/4,988 · function 1,737/1,737 · line 5,096/5,096: 모두 100% |
| 미검증 범위 | 모든 Canvas 입력 조합, 실제 스크린리더, 설치본·실제 엔진·장치·파일 |

Storybook 문서 전용 UI는 제품 코드 coverage에서 제외하고 실제 렌더와 빌드 계약으로 검사합니다.
제품의 현행 소스·설치 검증은 `docs/validation/current.md`를 따릅니다.

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| Foundations / 계약 표 | Numeric·필드·제목의 크기와 굵기 일부가 실제 코드와 다름 | 구현 기준을 잘못 전달 | 기본·compact·실제 Knob 부모의 값을 구별하고 실제 Core Canvas 연결 |
| Fonts / DocsLayout | 부모에서 계산된 글꼴 변수가 언어 영역을 따라 바뀌지 않음 | CJK 문서·예시의 지역 글꼴 불일치 | 명시적 lang과 두 family 역할의 영역별 재연결 |
| IconActionButton | 외부 툴팁이 실제 버튼 focus에 연결되지 않음 | 키보드로 명령 설명을 읽기 어려움 | Core에서 native button·툴팁을 함께 소유; 중복 제거 |
| TextField | defaultValue를 Input에 전달해 Context value와 충돌 | 모달·암호 필드의 초기값 누락과 제어 방식 경고 | AriaTextField가 초기값을 소유하고 표시 전환·편집·재렌더의 값 유지 검증 |
| Components / Pages | disabled·busy·오류 조합과 실제 적용 실패 예시 부족 | 상태의 소유자·복구 범위를 확인하기 어려움 | Contract States와 readback·미지원·녹음·제출 대기/복구 예시 추가 |
| 문서 빌드 검사 | API·기본값·공개 export 누락을 탐지하지 못함 | 문서와 코드가 조용히 어긋남 | AST 기반 API·타입·기본값·export 대조 |
| GraphApplicationQueue / GraphLayoutController | 두 번째 revision 충돌에서 초안 소실·수동 선택 가능 | 평범한 동기화가 사용자 과업을 중단 | 자동 재조회·병합·제한 시간 재시도, 초안·외부 변경 보존 |
| RoutingWorkspace | 통신·배치 실패에도 두 선택 제공, 조회 실패가 대기로 가려짐 | 불필요한 선택과 불명확한 상태 | 일반 실패는 재시도 하나, 지속 충돌만 두 선택; loading status·failure alert 구분 |

## 2026-10-12 Storybook 공식 문서 기준 적용

세 공식 가이드의 출처와 적용 범위는 <a href="/?path=/docs/design-system-contributing--docs" target="_top">Contributing</a>에 기록했습니다. 첫 적용 당시 기준·운영 문서는 8개, 독립 컴포넌트 문서는 61개, 스토리는 230개였습니다.

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| Overview / 운영 문서 | 시작 방법·변경 순서·기여 절차가 분산됨 | 새 작업자가 구현과 문서의 소유 계층을 찾기 어려움 | GetStarted·Contributing에 실행·선택·변경·호환성 기준 정리 |
| ActionButton·TextField·StatusBadge | 개별 속성을 조작하며 계약을 읽는 경로 부족 | 조합 예시만으로 기본값과 API를 파악하기 어려움 | 실제 Core 컴포넌트의 Playground·ArgTypes·Source 연결 |
| TextField Playground | 입력마다 args를 갱신하면 채널 응답이 빠른 입력을 되돌림 | 입력 예제가 실제 필드 동작을 왜곡 | 로컬 React 상태로 입력을 소유하고 blur에서 args 반영; Controls 변경도 동기화 |
| 문서 빌드 검사 | Playground 이름만으로 실제 Core 사용을 확인할 수 없음 | 다른 컴포넌트가 공개 계약 예시로 연결될 수 있음 | CSF component 메타데이터의 import·소유 경로를 AST로 대조 |

추가한 13개 스토리의 Light/Dark 렌더와 새로 만들거나 변경한 문서 5개의 390px 레이아웃을 확인했습니다. 버튼의 Tab·Enter·Space 실행, 필드의 빠른 입력·blur·Controls 연동, 실제 JSX Source를 확인했습니다. 첫 적용은 위 세 컴포넌트의 전용 Playground를 대상으로 했습니다. 전체 확장 결과는 아래 기록을 따릅니다. 이 검사는 모든 play 입력 조합이나 시각 회귀 기준 이미지 검사를 뜻하지 않습니다.

## 2026-10-12 전체 Playground·페이지 문서 검수

전체 계약 61개에 실제 Core export의 Playground·API·기본값·개별 상태·사용 조합을 연결했습니다. 현재 문서는 69개, 스토리는 445개입니다. 61개 모두 실제 CSF 전체 코드와 필요한 입력 상태 helper를 문서에서 펼쳐 볼 수 있습니다. ReactNode·callback·포트·계측 store는 정확한 타입과 Source로 설명하며 JSON Controls로 대체하지 않습니다. 공개 props가 없는 ProductHeading·ThemePicker는 Context·조합 계약을 명시합니다.

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| 컴포넌트 58개 | 기존 조합 예시만 있고 독립 Playground·속성 표가 없음 | 지원 입력·구현 기본값·상태를 같은 기준으로 비교하기 어려움 | 실제 Core component 메타데이터·렌더·typed args·ArgTypes·Source와 전용 상태 연결 |
| Playground 상태 처리 | React와 preview 훅을 혼합하면 재렌더 실패; blur 확정 값과 이전 값의 경합 | 입력·선택 예제가 실제 동작을 왜곡 | preview 훅으로 로컬 값 소유, 외부 args의 실제 변경만 동기화, 최신 변경 값만 blur에서 공유 |
| Pages | 그래프 생성·삭제·파일 capability·업데이트 상태의 예제와 소유 목록 부족 | 페이지의 실패·대기·복구와 구현 관계를 확인하기 어려움 | App·Screen·Feature 소유 10개 목록과 실제 상태 링크, 신규 페이지 예제 26개 연결 |
| DocsLayout | 긴 inline 코드가 한 줄로 고정됨 | 390px Pages 본문이 가로로 넘침 | 공통 문서 소유 계층에서 inline 코드 줄바꿈, Source pre의 공백·코드 보존 |
| Storybook Docs 목차 | 한국어 제목 hash를 smooth-scroll이 인코딩된 ID로 조회 | 링크를 눌러도 해당 제목으로 이동하지 않음 | 문서 소유 DOM에서 hash를 해석해 제목으로 이동·포커스; 제목 hash 링크 보존 |
| 빌드 검사 | 선택적인 Playground·메타데이터만 확인 | 실제 렌더·API 연결·페이지 누락을 놓침 | 전체 Playground 필수, import 소유자·JSX 소비·명시 기본값·독립 API 연결·App/Screen 목록·상태 링크 검사 |

61개 Playground와 새 개별 상태 131종의 Light/Dark 렌더, 독립 컴포넌트 문서 61개와 Pages의 390px 본문을 확인했습니다. 숫자 빠른 입력·blur·Controls 양방향, Select 선택 보존·Controls 반영, 토글 Space, TextField Editing을 확인했습니다. 숫자·Select·체크박스·Dialog의 회귀 play 4개와 새 Feature 페이지 24개 play가 통과했습니다. App catalog 실패·대기에서 기존 작업 유지와 생성 잠금을 확인했습니다.

UI 품질·경계·타입·2,042개 동작 검사·앱/Storybook 빌드·문서 계약 검사와 UI coverage 100%를 유지합니다. 이 기록은 전체 입력 조합·기준 이미지 시각 회귀·실제 스크린리더·설치본·장치·파일 검증을 의미하지 않습니다.

## 2026-10-12 그래프 인스턴스 표시 이름

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| AudioGraphCanvas / RoutingWorkspace | 같은 앱을 가리키는 인스턴스의 이름이 같음 | 용도를 구별하기 어려움 | Core의 이름 더블클릭·바깥 노드 F2·단일 노드 메뉴를 Feature 표시 이름 편집으로 연결 |
| NameDialog | 생성용 빈 값 금지·40자 계약만 지원 | 기본 이름 복원·긴 별칭을 표현할 수 없음 | allowEmpty·maxLength·description을 공개 계약에 추가; 기존 기본값 보존 |
| GraphLayout / 프로토콜 | 인스턴스 별칭 저장 필드 없음 | 재조회·파일 복원 후 별칭 유지 불가 | UI ID별 nodeLabels 저장·검증·자동 병합; 실제 앱 이름·selector·오디오 revision 보존 |

두 Firefox 인스턴스의 이름 변경·저장, 빈 이름 복원, Escape 취소와 같은 노드로의 포커스 복귀를 실제 RoutingWorkspace의 Storybook play 3개에서 확인했습니다. 표시 이름 저장 호출과 오디오 그래프 제출 0회를 각각 검사합니다. 390px Dark/Pseudo에서 모달 가로 넘침 없이 긴 이름 입력·Tab·Enter 저장과 반환 포커스를 확인했습니다. 메뉴의 단일 선택·다중 선택 구분과 명령 위임은 Core 회귀 검사로 확인했습니다.

문서 69개·스토리 451개입니다. UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 162개 파일의 동작 검사 2,056개가 통과했습니다. UI coverage는 statement 6,011/6,011 · branch 5,063/5,063 · function 1,758/1,758 · line 5,163/5,163으로 모두 100%입니다. 프로토콜 호환 검사 1개, 엔진 표시 메타데이터 검사 9개, 그래프 파일 저장·열기·재시작 검사 1개가 통과했습니다. 이 기록은 설치본에 변경을 반영하거나 창별 오디오 캡처를 검증한 결과가 아닙니다.

## 2026-10-12 전체 모달 밀도·폼 하단 통일

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| Dialog / 테마 | 제목 16px·큰 헤더/본문 여백·폭이 모든 모달에 적용됨 | 짧은 저장 폼도 과하게 큼 | 공통 dialog 토큰: xs/sm/md 300/360/420px, 기본 헤더 32px·제목 13px, 본문 12px. 긴 제목 줄바꿈 |
| NameDialog / LicenseDetails | 생성·표시 이름·라이선스 버튼의 정렬이 다름 | 폼의 확정 위치를 예측하기 어려움 | 공통 FormActions end, 위 12px·버튼 간격 8px. 이름은 하단 취소 → 확정, 오류는 하단 명령 앞에 배치 |
| ConfirmationDialog | 내부 inline flex 규격으로 확인 행을 소유함 | 공통 폼 간격 변경을 따라가지 않음 | FormActions와 strong 확인 버튼 사용, 취소 우선 포커스와 busy 계약 유지 |

이름·생성·삭제·업데이트·라이선스·제품 정보 6종을 Light/Dark로 확인했습니다. 390px Dark/Pseudo에서 위 유형과 긴 제목, 무료 라이선스 활성화 폼의 가로 넘침·오른쪽 버튼 정렬을 확인했습니다. 긴 제목은 잘리지 않고 헤더 높이를 확장하며 라이선스 명령은 오른쪽에 맞춰 줄바꿈합니다. NameDialog FooterKeyboard play의 초기 필드 → 취소 → 확정, Shift+Tab·Enter 취소·Escape가 통과했습니다. 실제 GraphCreateDialog creating 상태의 Escape 차단도 유지됩니다.

문서 69개·스토리 454개입니다. UI 품질·경계·타입·앱/Storybook 빌드·61개 문서 계약 검사, 162개 파일의 검사 2,056개가 통과했습니다. UI coverage는 statement 6,013/6,013 · branch 5,063/5,063 · function 1,758/1,758 · line 5,165/5,165으로 모두 100%입니다. 설치본 반영·실제 스크린리더 검증과 구별합니다.

## 2026-10-12 기호·기본값·중복 정보 정리

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| AudioGraphCanvas / StudioScreen | + 옆에 추가 행동 문구를 반복함 | 반복 도구가 넓어지고 화면에 문구가 쌓임 | +만 표시, Core 메뉴/아이콘 버튼의 접근성 이름·툴팁·키 실행·메뉴 관계 유지 |
| NameDialog / 표시 이름 편집 | 원래 이름을 입력 값과 설명으로 중복 표시 | 기본값과 저장된 별칭을 구별하기 어려움 | Core placeholder 계약 추가; 별칭 없는 값은 빈 입력·원래 이름 placeholder. 필드 이름 유지 |
| 효과 메뉴 / 업데이트 | 일반 효과 설명과 같은 설치/최신 버전이 반복됨 | 선택과 현재 상태를 읽는 데 불필요한 줄이 늘어남 | 컴프레서·리미터는 이름만, 같은 버전은 한 번 표시. 장치/플러그인 식별·형식·제한·오류 정보 유지 |

OriginalPlaceholder play에서 빈 값·Firefox placeholder·설명 없음, +만 표시하는 버튼의 툴팁·Enter 메뉴 열기·Escape 포커스 복귀를 확인했습니다. 실제 App의 새 그래프 아이콘은 접근성 이름·단축키를 유지하며 Enter로 생성 창을 열고 취소 후 돌아왔습니다. 프로세서 메뉴와 업데이트 Current의 실제 렌더, 390px Dark/Pseudo 모달의 가로 넘침 없음도 확인했습니다. 실제 스크린리더 검증과 구별합니다.

문서 69개·스토리 455개입니다. UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 162개 파일의 검사 2,056개가 통과했습니다. UI coverage는 statement 6,015/6,015 · branch 5,065/5,065 · function 1,758/1,758 · line 5,167/5,167으로 모두 100%입니다. 개발 설치본에는 아직 반영하지 않았습니다.

**위치 · 관찰 · 영향 · 수정**을 한 항목으로 씁니다. 예: `Routing/Workspace → Options · 긴 번역에서 값이 잘림 · 현재 장치 형식을 읽기 어려움 · Core 필드의 줄바꿈 계약 수정`.

람스의 기능·질서, 아이브의 위계·재질·일관성, Teenage Engineering의 밀도·표기·반복 조작 관점으로 검토합니다. 제품에 없는 장식이나 하드웨어 형태를 복제하지 않습니다. 관점의 원문과 적용 범위는 `docs/design/ui-reviewers.md`, 조작 계약은 `docs/design/interaction-accessibility.md`를 따릅니다.

## 2026-10-12 앱 전체 정보·아이콘 조작 검수

`audit:ui`로 제품 TSX 86개와 조작 소스 위치 107개·소유 파일 49개를 대조했습니다. 목록은 native 입력·버튼·동적 button/div 별칭·Core 조합을 포함하며 화면에 생성되는 버튼 개수나 접근성 합격 판정이 아닙니다.

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| Core CollectionItem·AudioNodeCard·TextField, Feature RecorderControls | 반복 삭제·설정·에디터·녹음·비밀번호 행동의 이름에 대상이 빠짐 | 여러 인스턴스·필드에서 조작 대상을 구별하기 어려움 | 그래프/노드/필드 이름과 행동을 조합. 전원·음소거·사이드바·닫기·비밀번호에 공통 Tooltip, 중복 native title 제거 |
| Core useNodeHeaderFocus | React Aria가 pending 버튼에 보내는 blur와 실제 포커스 이탈을 동일하게 처리 | 복구 버튼 제거 후 body로 포커스 유실 | 실제 activeElement와 포커스 관계로 소유권 유지. 인접 명령 복귀와 명시적 blur·다른 소유자 존중을 함께 회귀 검사 |
| Feature 한도·인증·Driver 안내, Core i18n | 전체 무료 한도·Free 상태·일반 노드 설명 반복, 오래된 설치 문구, Pseudo 키 누락 | 관련 제한·상태를 읽기 어렵고 긴 번역 검수가 영어 fallback으로 누락됨 | 동작별 한도, 무료 요약 한 번, 실제 상태/오류 유지. 가상 장치 부재만 짧게 설명. Pseudo 전체 키 생성·보간 토큰 보존 검사 |

Core 아이콘 회귀 검사 19개와 실제 브라우저 KeyboardOwners·RecoveryKeyboard play에서 Enter/Space 호출 횟수·순/역 Tab·폴더·비밀번호·테마 메뉴 Escape 복귀를 확인했습니다. 메뉴의 계산된 이름은 React Aria의 트리거 `aria-labelledby` 관계를 포함해 검사합니다. Feature의 인스턴스별 녹음 명령·한도 사유와 복구 포커스는 실제 소유자 회귀 검사가 다룹니다.

App·연결·그래프 생성/목록/파일·인증·제품 정보·업데이트·표시 이름의 브라우저 렌더와 버튼 이름을 대조했습니다. 390px Dark/Pseudo에서 주요 과업과 긴 삭제·업데이트 문구를 확인했으며 확인한 화면에 이름 없는 버튼이나 페이지 가로 넘침은 없었습니다. 모든 Storybook play 입력 조합·실제 스크린리더·설치본 검증을 의미하지 않습니다. 채널 연결의 키보드 대체 과업은 제품 명세의 별도 범위입니다.

UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 163개 파일의 검사 2,082개가 통과했습니다. coverage는 statement 6,030/6,030 · branch 5,090/5,090 · function 1,762/1,762 · line 5,181/5,181로 모두 100%입니다. 문서 69개·스토리 459개입니다. 재사용 검사는 프로젝트 `audit:ui` 명령으로 유지하며 임시 이관 스크립트는 제거했습니다.

이 UI 검수 시점에는 단일 제품 기준을 명세에 기록했으며 패키지 통합은 후속 작업이었습니다. 후속 구현과 검증은 아래 기록을 따릅니다.

## 2026-10-12 단일 제품·업데이트·긴 명령

| 위치 | 관찰 | 영향 | 수정 |
| --- | --- | --- | --- |
| UpdateAccess / UpdatesPort / native updates | 앱·Driver별 카탈로그와 버전 표시 | 한 제품을 따로 설치·업데이트하게 됨 | MixRelay 하나의 설치 버전·게시 버전·전체 설치 링크. 미게시·실패 상태 유지 |
| RuntimeConnection / 11개 언어 | 별도 Driver 설치·업데이트 안내 | 복구 행동이 통합 설치와 어긋남 | 같은 MixRelay 설치·업데이트 행동에 연결 |
| Core ActionButton / UpdateAccess | 390px Pseudo의 다운로드 명령이 한 줄로 모달을 넘침 | 문구와 조작이 가로 스크롤에 가려짐 | regular 28px·compact 24px 최소 높이 유지, 부모 폭 안 줄바꿈과 높이 확장. 공통 Narrow Long Label 예제 추가 |

실제 UpdateAccess의 390px Light/English·Dark/Pseudo와 한국어 호환 불일치 복구 화면을 확인했습니다. 업데이트 아이콘 Enter 열기, 모달 Tab 순환, Escape 닫기·아이콘 포커스 복귀를 확인했습니다. 수정 후 긴 다운로드 문구가 줄바꿈하고 모달 가로 넘침이 사라졌습니다. Core의 같은 규격을 다운로드 Inline과 폼 하단 FormActions가 소비합니다.

UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 163개 파일·2,081개 검사가 통과했습니다. statement 6,026/6,026 · branch 5,084/5,084 · function 1,762/1,762 · line 5,177/5,177로 모두 100%입니다. 중복 제품 경로를 제거했고 coverage 기준은 유지했습니다. macOS의 실제 전체 PKG 빌드·서명·확장 검사는 완료했으며, 공증·호스트 설치·기존 설치본 실제 이관과 Linux 배포판 설치 검증은 `docs/product.md`의 별도 범위입니다.
