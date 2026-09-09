# 기존 요구 완성도 재점검 — 2026-09-07

## 작업 경계

사용자가 작업 순서를 확정했다. **현재 합의된 과정의 세 OS 구현·검증을 먼저
마무리하고, 미정 사항은 그 뒤 결정한다.** 아래 범위 검토 목록은 지금 실행할
작업이 아닌 보류 목록이다. 기존 기능을 임의로 축소하거나 OS별 누락을 허용하지
않으며, 미정 목록이 있다는 이유로 확정 작업을 대기시키지 않는다.

목표는 새로운 제품 기능 확대가 아니라 이미 요청된 라우팅·장치·플러그인·OS
동등성의 누락을 구현하고 실제 사용 경계를 검증하는 것이다. 기존 구현을 폐기하거나
지원 형식을 조용히 줄이지 않는다. 테스트 개수는 기능 완성도 비율이 아니다.

매트릭스의 **35개 그룹 / 열린 출시 게이트 31개**는 미구현 기능 31개라는 뜻이
아니다. 그룹에는 실제 구현 누락, 일부 OS의 구현 누락, 실제 장치 검증 부족이
섞여 있다. 감사 시 표기는 all-OS implemented 13개, partial 포함 18개, missing
포함 3개, 범위 검토 1개였지만 이는 표의 집계이며 구현의 정확성을 증명하지 않는다.
예를 들어 plugin_discovery의 implemented 표기에도 native 실패를 빈 성공 목록으로
바꾸는 코드가 있었다. 행 이름 대신 아래의 실제 동작 계약을 확인한다.

## 구현 누락 — 처리 순서

| 순서 | 기존 요구와 누락 | 현재 소스 경계 | 종료에 필요한 증거 |
|---|---|---|---|
| 1 | 실패를 빈 목록·정상 상태로 표시하지 않기 | processor-host discovery, routing-workspace/model.ts의 가상 노드 positive 고정 | 빈 결과와 실패 구분, 이전 정상 값 보존, 명시적 복구, 늦은 응답 차단 |
| 2 | 편집한 설정이 재시작·전환에서 보존되기 | engine-service/graph_catalog.rs, processor-host/rack_formats.rs | editor opaque state 보존, 저장 실패 처리, format별 실제 recall |
| 3 | OS와 무관한 가상 장치 이름·채널 설정 | engine-service/windows.rs의 2 render/1 capture 고정 LAB backend | 제품 드라이버 name/width 설정·원복·재생성, 실제 설치 장치 PCM |
| 4 | Ableton/SPAT 등과 실제 sample-rate/clock 상호운용 | engine-service/{main,linux,windows}.rs session profile | format 협상·rollback, SRC/drift, 독립 clock 왕복·장시간 수치 |
| 5 | 앱 이름·아이콘과 재실행 시 선택 의도 유지 | audio-io/{windows,linux}.rs의 icon None·PID selector | 새 PID/stream 재연결, 다른 앱 혼입 없음, 실제 아이콘/fallback |

이번 수정은 1번 중 플러그인 검색의 누락을 대상으로 한다. 나머지 항목을 이번에
완료한 것으로 표시하지 않는다. Windows LAB 드라이버 테스트 통과를 제품 드라이버
구현 완료로, sample-rate mismatch 거부를 SRC 구현으로 바꾸어 부르지 않는다.

## 구현과 실제 검증을 분리할 항목

- 입력 게인, 실시간 조절과 history coalescing, 그래프별 위치·viewport 저장은
  코드와 회귀 테스트가 있다. 모든 OS의 native slider→PCM/no-reopen과 실제
  종료·재시작 검증이 별도로 남는다.
- 다중 포트 선택, 케이블 수렴/spread, mini bar·tooltip, Options 고정 anchor는
  실제 32/64 채널 UI에서 검증한다. React Flow mock 통과로 대체하지 않는다.
- 메뉴 내부 스크롤, 단축키·텍스트 입력 예외, DPI별 배치는 실제 앱에서 검증한다.
- Ubuntu ARM64 결과를 Debian/Kali/Arch 또는 Linux x86_64에 복제하지 않는다.
  Windows/macOS도 기존 테스트의 스냅샷과 현재 변경 범위를 구분한다.

## 자동 확장하지 않을 범위

- MIDI/transport는 매트릭스상 scope_review이며 현재 대화에서 확정된 신규 요구가
  아니다. 이를 임의로 구현하거나 기존 요구 누락 개수로 계산하지 않는다.
- graph export/import·cross-OS device remapping은 저장/복원 계약과 구분할
  제품 범위 검토 사항이다. 현재 매트릭스에는 남기되 확정 요구라고 단정하지 않는다.
- Linux 에디터 지원과 XWayland 없는 순수 Wayland 에디터 구현은 같은 조건이
  아니다. 지원 런타임·의존성을 명시해야 하며 사용자 합의 없이 Linux 지원을
  제외하거나 별도 윈도우 시스템 구현을 확정 범위로 확대하지 않는다.
- 이미 노출한 플러그인 형식의 기능 부족은 지원 계약으로 추적한다. 형식을 삭제하여
  게이트를 닫지 않는다.

범위가 불확실하다는 이유로 이번에 출시 게이트를 자동 종료하지 않았다. 구현 누락과
검증 부족이 모두 해소되어야 해당 계약을 완료로 표시한다.

## 감사 이후 반영된 하위 계약

위 표는 감사 시점의 누락 목록이다. 이후 검색 실패/재시도는
[discovery diagnostics](2026-09-07-native-discovery-diagnostics.md), stale meter는
[meter freshness](2026-09-07-meter-freshness.md), Windows/Linux 표시 메타데이터와
공통 selector별 아이콘 연결은 [application metadata](2026-09-07-application-metadata.md)에
구현·검증 근거를 기록했다. 이 결과를 앱 재실행 추적이나 모든 native UI 완료로
확대하지 않는다. 현재 잔여의 기준은 기능 매트릭스다.
