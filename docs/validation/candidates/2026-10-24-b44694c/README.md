# 설치본 검증: b44694c4

제품 소스는 `b44694c409b64ca42649ecfadf0513061fafe2e8`이다.
macOS Universal PKG와 Linux 두 CPU의 DEB, 같은 x64 DEB payload를 감싼 Arch 패키지를
한 후보로 묶는다. 설치본과 시험 코드의 커밋은 리포트에 따로 기록한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 패키지 빌드 | [40061295014](https://github.com/mixrelay/mixrelay/actions/runs/40061295014) |
| macOS PKG | draft `qa-install-b44694c`, Apple 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `9920fb25aee9b80c615f9ac429a8fa53f2704f485e373d7e978ea8bd47442b2a` |
| 동일 후보 13개 환경의 fresh·upgrade·제거 | [40063786943](https://github.com/mixrelay/mixrelay/actions/runs/40063786943), 시험 코드 `6cf50fcad44c4cebe41458e68c9341798ba8eacf` |
| 이전 설치본 | `f7575aa4a8e2f0d97104033023bde12bf7b6d083`, Linux 빌드 `40054217962`, macOS draft `qa-install-f7575aa` |

최종 판정은 위 설치본 실행의 `inventory`와 대상별 리포트에서 확인한다.
`installed-candidate`의 `mix-relay.candidate.json`은 모든 최종 파일의 크기·SHA-256·소스·계약 해시를
고정한다. 대상별 `installed-target-*`에는 두 설치 모드의 원시 증거와 제거 결과가 있다.
필수 시나리오의 실패·미실행·skip이나 정리 실패가 있으면 후보 전체가 실패한다.
이 진단 실행은 공개 배포를 승인하지 않으며 `main`의 출시 검증·게시 조건은 별도로 적용한다.

## 수정과 검증 범위

- Ubuntu ARM의 systemd coldplug 실패는 실제 BOOT·UEFI 장치를 확인하고 원래 mount unit을
  다시 시작해 복구한다. fstab의 필수 mount 검사를 없애지 않는다.
  [VM 검사 40058327538](https://github.com/mixrelay/mixrelay/actions/runs/40058327538)과
  [40060572539](https://github.com/mixrelay/mixrelay/actions/runs/40060572539)가 7개 대상에서 통과했다.
- macOS 데몬의 AVFoundation 사전 조회가 사용자 세션의 마이크 권한을 거절로 오인하던 경로를
  고쳤다. 그래픽 세션이 없는 데몬은 CoreAudio의 실제 IOProc 등록으로 동의를 요청한다.
  실제 native 준비 완료를 받아야 성공하며 허가 대기에는 기존 비동기 작업 영수증을 사용한다.
- Apple stapler의 관측된 연결 끊김과 Homebrew의 DNS 실패만 제한적으로 다시 시도한다.
  유효하지 않은 공증 티켓·도구 오류·재시도 소진은 계속 실패한다.
- 설치 UI는 실제 접근성 트리에서 대상을 찾고, 포인터 클릭과 포커스·오른쪽 화살표로 조작한다.
  동작의 응답이 늦었다고 클릭을 다시 제출하지 않는다. 실제 포커스와 그래프 연결을 확인한다.
  AT-SPI 조회에는 2초, TCG 초기 준비와 macOS 설치 직후 HAL 탐색에는 120초를 허용한다.
  그 밖의 초기 준비 검사는 30초 안에 실제 `ready`를 확인해야 한다.
- macOS UI 그래프를 만들고 화면·트리·엔진 readback을 보존한 다음 편집 창을 최소화한다.
  native 오디오는 정상 백그라운드 실행 상태에서 시험하며 기본 48kHz·128프레임을 유지한다.
  `installed-ui-audio-phase.json`에 창 상태를 기록하고 권한 관찰자는 종료·제거까지 유지한다.
- 전체 시스템 캡처는 무료 권한, 실제 시스템 권한 대화상자와 두 독립 신호 앱을 사용한다.
  캡처 그래프가 준비된 뒤 신호 앱을 시작하고, 같은 그래프의 두 채널에서 신호를 확인한 뒤 녹음한다.
  30초 안에 두 채널이 준비되지 않으면 실패하며 관측값은 `whole-system-ready.json`에 남긴다.
  누락·반복·손상·비유한 PCM과 녹음 파일의 채널·주파수 검사는 유지한다.

macOS 최소 지원 버전은 process tap API에 따른 14.2다. 정확한 14.2 패치에서의 실검증을
주장하지 않으며, 주 버전별 hosted 대표 환경의 관측 패치·CPU를 리포트에 남긴다.
지원 범위와 대표 검증 환경은 [제품 정책](../../../product.md)과
[`support/environments.json`](../../../../support/environments.json)이 기준이다.

## 회귀와 조사 증거

공통 스크립트 519개와 Linux 전용 ELF 검사 2개를 각각 skip 없이 실행했다.
마이크 shim의 실제 Objective-C 경계, Rust 권한·native 준비 판정, VM 복구 셸과 공증 연결 오류의
회귀 검사도 수행했다. Swift UI 도구는 Xcode SDK로 타입 검사를 했다.

수정 전 설치본의 실패는
[40058248883](https://github.com/mixrelay/mixrelay/actions/runs/40058248883),
[40060226275](https://github.com/mixrelay/mixrelay/actions/runs/40060226275),
[40061045205](https://github.com/mixrelay/mixrelay/actions/runs/40061045205)에 보존한다.
후속 [40062520129](https://github.com/mixrelay/mixrelay/actions/runs/40062520129)에서 드러난
macOS 창 숨김 실패, Intel 프레임 누락과 첫 권한 승인 뒤의 무음도 실패로 보존한다.
macOS 14 ARM의 첫 시스템 녹음에서 한 채널이 없었던 파일, Intel 15 업그레이드의 프레임 손실,
AT-SPI 응답 시간 초과를 통과 결과로 바꾸지 않는다. 각 수정의 설치본 판정은 위의 새 전체 실행으로 확인한다.

문서는 `docs` 설정으로 교정했다. 영문 추가 설명의 검출·따옴표 정규화와 원본 보존 검사를 실행했다.
제품 문서의 보존 검사는 상태표·후보 식별자·이전 후보 링크 교체를 3개 오류로 판정해 실패했다.
이 세 변경은 최신 후보로 상태를 갱신하는 작업 범위와 수동 대조했으며, 릴리스 절차 문서의 보존 검사는 통과했다.
후속 갱신에서 이 문서의 보존 검사는 실행 표와 시험 커밋 교체를 2개 오류로 판정했다.
실행 ID·커밋·519개 검사 결과는 원시 결과와 대조했다. 두 보존 검사 실패는 교정 도구의 판정이며
설치본 CI의 통과를 뜻하지 않는다.
