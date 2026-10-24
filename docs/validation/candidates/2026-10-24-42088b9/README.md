# 설치본 진단: 42088b93

제품과 시험 소스는 `42088b93c5e05119b52b939cf484ae8168ee49d6`이다.
네 설치 파일의 소스·크기·SHA-256과 지원 계약은 [후보 JSON](mix-relay.candidate.json)에 고정한다.
읽기 요청의 실행부 생존 응답 누락을 수정한 후보이며, 오디오 처리·버퍼·PCM 수락 조건은 유지한다.

- Linux 두 CPU의 DEB와 Arch 빌드: [40102936031](https://github.com/mixrelay/mixrelay/actions/runs/40102936031), 모두 통과.
- macOS PKG: draft `qa-install-42088b9`, 코드 서명·공증 Accepted·staple·validate 완료.
- Apple 공증 제출: `15881456-f1c5-4466-9aff-6d55ec0676d9`.
- UI CI: [40102929097](https://github.com/mixrelay/mixrelay/actions/runs/40102929097), 통과.
- Native CI 13개 대상: [40102929155](https://github.com/mixrelay/mixrelay/actions/runs/40102929155), 모두 통과.
- Linux VM 부팅 7개 대상: [40102929262](https://github.com/mixrelay/mixrelay/actions/runs/40102929262), 모두 통과.
- Ubuntu ARM 설치본 진단: [40104329016](https://github.com/mixrelay/mixrelay/actions/runs/40104329016), 새 설치·업그레이드 UI 실패, 정리 통과([원시 inventory](ubuntu-arm-focused-inventory.md)).
- 13개 대상 전체 설치본 검증: [40107306863](https://github.com/mixrelay/mixrelay/actions/runs/40107306863), 진행 중.

## 읽기 요청의 생존 확인

편집 앱은 요청의 응답이 3초 동안 없으면 연결 실패로 판단한다. 실행부는 변경 요청을 처리하는
동안 1초마다 대기 응답을 보냈으나 읽기 요청에는 이 응답이 없었다. 엔진 읽기가 오래 걸려도
살아 있는 실행부를 확인할 수 있도록 같은 대기 응답을 읽기에도 적용했다. 편집 앱 종료·연결
해제·실행부 종료 시 읽기 취소와, 수락한 변경의 완료·중복 실행 금지 규칙은 유지한다.

실제 Unix 소켓을 가상 시계로 구동한 회귀 시험은 수정 전에 시간 초과로 실패했고 수정 뒤
통과했다. 데스크톱 시험 235개는 제외 0개로 통과했으며 엄격 clippy와 크레이트 계층 검사도
통과했다. Ubuntu ARM의 실제 설치본은 여전히 실패했으므로 이 수정으로 해결됐다고 판단하지 않는다.
실패 시 편집 앱 로그는 명령 이름과 고정된 전송 오류 분류를 기록하며 인자·원시 오류는 기록하지 않는다.

## 판정 범위

Ubuntu ARM 새 설치는 Recorder 메뉴에 초점을 요청하다 AT-SPI 응답 시간초과로 실패했다.
업그레이드는 노드 연결 뒤 Recorder가 준비되지 않았으며, 실제 화면에는
`Recording error: node_not_found`가 표시됐다. 두 모드의 API 그래프 저장·복원, 오디오,
녹음, 제거와 정리는 통과했다. 편집 앱 로그에는 두 모드 모두 `list_audio_devices`와
`control_apply_graph` 등의 `response_timeout`이 남았다. 읽기 생존 응답 외의 원인을 조사한다.

진행 중인 전체 실행에서는 macOS 26 ARM 새 설치의 PCM 누락 684·반복 256·손상 428프레임을
기록했다. 업그레이드는 기준 PKG의 설치 스크립트 실패로 후속 시나리오가 차단됐다.
PCM 조건과 설치 실패 판정을 유지하며, 다음 실행은 Installer의 상세 오류도 수집한다.
macOS 26 Intel도 새 설치에서 PCM 누락·반복 각 256프레임으로 실패했다. 업그레이드는 통과했다.

이번 집중 실행은 Ubuntu ARM의 새 설치·업그레이드를 확인한다. 나머지 12개 대상은 이 실행의
검증 범위에 없으며, 집중 통과를 전체 수락으로 합산하지 않는다. 마지막 13개 전체 실행의
[fa09faed 결과](../2026-10-24-fa09fae/README.md)는 8개 통과·5개 실패로 유지한다.
macOS PCM 실패와 [독립 BlackHole 대조](../2026-10-24-a4668a1/README.md)의 실패도 유지한다.
제품·업데이트 카탈로그를 공개하지 않았다.
