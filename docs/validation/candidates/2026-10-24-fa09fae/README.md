# 설치본 검증: fa09faed

제품 소스는 `fa09faedaef98e8fc4b908d663ea6b5b5a6c2f7b`, 설치 시험 코드는
`c86b996cfc6b6160157f7a1bdc2916c02c07db82`다. 두 커밋 사이에는 Linux UI 관찰자와
검증 기록만 바뀌었다. macOS와 Linux는 같은 제품 소스를 사용하며, 네 설치 파일의 크기·SHA-256과
지원 계약 해시는 [후보 JSON](mix-relay.candidate.json)에 고정한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40094401742](https://github.com/mixrelay/mixrelay/actions/runs/40094401742), 모두 통과 |
| macOS PKG | draft `qa-install-fa09fae`, 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `8b04364f2e3e917b5a3070c2fdcb84d304c29185af612420c1a671a730ea16d5` |
| PKG 크기 | 69,408,014바이트 |
| Apple 공증 제출 | `a63c99f8-df22-42b3-b37f-e65b0b907c00` |
| 13개 환경의 fresh·upgrade·제거 | [40095387842](https://github.com/mixrelay/mixrelay/actions/runs/40095387842), 진행 중 |
| Native CI 13개 대상 | [40094407865](https://github.com/mixrelay/mixrelay/actions/runs/40094407865), 제품 소스 `fa09faed`, 진행 중 |
| Linux VM 부팅 7개 대상 | [40094407862](https://github.com/mixrelay/mixrelay/actions/runs/40094407862), 제품 소스 `fa09faed`, 진행 중 |
| UI CI | [40094407888](https://github.com/mixrelay/mixrelay/actions/runs/40094407888), 제품 소스 `fa09faed`, 통과 |
| 이전 설치본 | 소스 `f7575aa4a8e2f0d97104033023bde12bf7b6d083`, Linux `40054217962`, macOS draft `qa-install-f7575aa` |

전체 수락은 위 설치본 실행의 최종 inventory에서 13개 대상의 모든 필수 시나리오와 정리가
통과했는지로 판단한다. 실패·미실행·skip은 수락할 수 없다. 개별 진단의 통과를 합치지 않으며,
후보는 draft로 보관한다. 제품과 업데이트 카탈로그는 공개하지 않았다.

## 수정과 확인 범위

이전 제품 소스 `3c72c55c`의 전체 실행은 13개 중 7개 통과·6개 실패였다.
Ubuntu ARM 부팅은 통과했지만 메뉴 조작·Recorder 준비 확인과 macOS PCM 누락 등이 남았다.
[이전 후보 기록](../2026-10-24-3c72c55/README.md)에 실패와 원시 결과를 보존한다.

새 제품은 숨긴 편집 창의 상태·미터 전송을 1초에 한 번으로 제한하고, 창이 다시 보이면
즉시 최신 상태와 누적 최고 미터값을 전송한다. 보이는 창의 20Hz 전송과 엔진·공유 리더의
계측 주기는 유지한다. 상태 전달을 완전히 끊지 않으므로 숨긴 창에서도 연결 감시를 계속한다.
가상 시계 시험으로 숨긴 동안의 병합, 복귀 즉시 전송과 기존 10초 credit 종료 조건을 확인했다.
실제 설치본에서의 동작과 PCM 결과는 위 전체 실행으로 확인한다.

시험 도구는 오디오·녹음 종료와 파일 마무리 뒤에 CPU·WebContent 진단을 실행한다.
이전 진단에서 PCM 측정은 끝났어도 녹음이 계속되는 동안 무거운 샘플 수집이 실행됐다.
진단보다 녹음 종료가 먼저라는 순서를 경계 시험으로 검증했으며, 순서를 뒤집은 결함 주입은 실패했다.
원래 PCM 실패는 그대로 반환한다. 이 관찰만으로 모든 PCM 누락의 원인을 단정하지 않는다.

Linux 관찰자는 활성 상태와 실제 포커스를 확인한 뒤 leaf control에 Return을 한 번 보낸다.
노드 컨텍스트 메뉴는 바깥 canvas panel에 포커스를 둔 뒤 ContextMenu 키로 연다.
메뉴·연결·Recorder 준비 확인은 유지한다. 18개 격리 Python 시험과 Node wrapper가 통과했으며,
Ubuntu ARM의 실제 설치본 결과는 위 실행에서 확인한다.

로컬 데스크톱 시험 234개(ignored 0), 엄격 clippy, 공통 스크립트 560개(skip 0),
전체 UI 검사(UI 2,236개·접근성 243개)와 UI 품질 검사가 통과했다.
릴리스 빌드 전 실제 앱·PKG 서명 시험도 통과했다. 엔진의 48kHz·128프레임,
측정 신호 길이와 PCM 누락·반복·손상·비유한 값 0 조건은 바꾸지 않았다.
신호 사전 재생·실패 후 재측정·PCM 구간 제거는 하지 않는다.
