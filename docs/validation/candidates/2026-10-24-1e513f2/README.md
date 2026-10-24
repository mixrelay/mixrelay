# 설치본 검증: 1e513f21

제품 소스는 `1e513f21c7fb664c1eca338de8fca3a188fc6f0f`, 시험 코드는
`50a780b09b72895d7f3b4aa1150720d14214b9f6`이다.
동일한 Universal PKG, CPU별 DEB와 같은 x64 payload의 Arch 패키지를 설치한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40064756032](https://github.com/mixrelay/mixrelay/actions/runs/40064756032) |
| macOS PKG | draft `qa-install-1e513f2`, 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `b6c2f842aaa9d74abe677d74b39262cdd70bc39fd47ccfb3002f001c5ac6eb84` |
| 13개 환경의 fresh·upgrade·제거 | [40066104242](https://github.com/mixrelay/mixrelay/actions/runs/40066104242) |
| 이전 설치본 | 소스 `f7575aa4a8e2f0d97104033023bde12bf7b6d083`, Linux `40054217962`, macOS draft `qa-install-f7575aa` |

최종 판정은 이 실행의 `inventory`와 대상별 리포트를 따른다. 실패·미실행·skip 또는 정리 실패가
있는 필수 시나리오는 후보 전체를 실패시킨다. 개별 대상이나 후속 진단의 통과를 전체 수락으로 합치지 않는다.
`installed-candidate`는 파일별 크기·해시·제품 소스·지원 계약을 고정하며, `installed-target-*`는
두 설치 모드의 원시 증거와 관측한 실제 OS 패치·CPU를 보존한다. 후보는 공개 배포하지 않았다.

## 변경과 확인 범위

- HAL은 CoreAudio 호스트에 지연 0을 보고한다. 앱의 측정 지연과 변경 알림은 유지하며,
  알림용 값이 호스트의 입력 시각을 움직이지 않게 한다. 수정 전 호스트가 1을 반환하던 회귀를
  재현했고, 수정 후 HAL 검사 113개와 Clippy가 통과했다. 실제 PCM 판정은 위 설치본 실행에 둔다.
- Ubuntu ARM coldplug 복구와 macOS 데몬의 실제 CoreAudio 동의는 이전 후보의 수정을 포함한다.
  Linux UI는 편집 앱의 식별을 유지하고 이름이 맞는 대상에 역할·표시 상태를 조회한다.
  조회 실패만 다시 확인하며 물리 클릭은 반복하지 않는다. TCG의 조회·그래프 반영은 120초,
  UI 전체 흐름은 15분 안에 실제 결과를 확인해야 한다. native AT-SPI 조회 한도는 2초다.
- 녹음 명령의 적용 전 거절은 같은 그래프·revision·녹음 상태·경로를 다시 확인한 뒤에만
  제출한다. 불확실한 결과·다른 거절·외부 변경·대기 한도 소진은 실패한다.
  관측값은 `recorder-admission.json`과 `whole-system-recorder-admission.json`에 남긴다.
- macOS 첫 시스템 오디오 동의는 같은 권한창을 한 번만 승인하고, 실제 창 목록에서 닫힘을 확인한다.
  닫힘을 확인하기 전에는 신호 앱을 시작하지 않는다. 두 독립 소스의 실제 신호를 확인한 뒤 녹음하며,
  동의·신호 준비·생산자 콜백·PCM 파일을 각각 증거로 남긴다. 업그레이드는 유지된 동의와 실제 오디오를 검사한다.
- UI 화면·접근성 트리·그래프 연결을 보존한 뒤 편집 창을 최소화한다. 기본 48kHz·128프레임과
  PCM 누락·반복·손상·비유한 값 0 조건, 녹음의 채널·주파수 검사는 유지한다.

공통 스크립트 525개는 실패·skip 없이 통과했다. Swift 도구는 Xcode SDK로 타입 검사를 했고,
Linux 조회 변경은 실제 GTK WebKit에서 노드 생성·연결을 수행해 확인했다.
소스·도구 검사를 설치본 전체 수락으로 표시하지 않는다.

이전 [b44694c4 후보](../2026-10-24-b44694c/README.md)의 실패는 그대로 보존한다.
macOS 최소 지원 버전은 process tap API에 따른 14.2이며, 정확한 14.2에서의 실검증을 주장하지 않는다.
지원 정책은 [제품 문서](../../../product.md)와 [지원 계약](../../../../support/environments.json)을 따른다.

문서는 `docs` 설정으로 교정했다. 제품 문서의 보존 검사는 상태표·후보 식별자·링크 교체를 3개 오류로,
검증 현황 문서는 새 날짜 절을 추가한 것을 1개 오류로 판정해 실패했다. 각 변경을 최신 후보 갱신과
검증 기록 추가 범위에 대조했으며, 이를 기계적 검사 통과로 표시하지 않는다.
