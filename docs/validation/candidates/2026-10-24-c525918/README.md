# 설치본 검증: c5259184

제품 소스는 `c52591847b587fb555feec8b55282a41da4617cc`, 시험 코드는
`bcdd4a781b16fa8b1413393e372e694beca1bb89`다. 이후 변경은 설치 시험·러너 준비·진단과 CI 구성에
한정하며 제품 실행 파일을 바꾸지 않는다. macOS 서명 시험은 macOS 전용 CI 단계에서 실제로 실행한다.
동일한 Universal PKG, CPU별 DEB와 같은 x64 실행 파일을 담은 Arch 패키지를 설치한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40068941574](https://github.com/mixrelay/mixrelay/actions/runs/40068941574) |
| macOS PKG | draft `qa-install-c525918`, 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `462fefec1d579a5d9affbbd6794d28d6c67bced0a6f6ac7b268395bad4e29b4a` |
| PKG 크기 | 69,380,291바이트 |
| Apple 공증 제출 | `1c83510c-ef9e-4694-bc63-2a3f8ea6cae1` |
| 13개 환경의 fresh·upgrade·제거 | [40072198505](https://github.com/mixrelay/mixrelay/actions/runs/40072198505) |
| Native CI 13개 대상 | [40070065316](https://github.com/mixrelay/mixrelay/actions/runs/40070065316), 소스 `dd7166b9`, 모두 통과 |
| Linux VM 부팅 7개 대상 | [40070065304](https://github.com/mixrelay/mixrelay/actions/runs/40070065304), 소스 `dd7166b9`, 모두 통과 |
| UI CI | [40070065293](https://github.com/mixrelay/mixrelay/actions/runs/40070065293), 소스 `dd7166b9`, 통과 |
| 이전 설치본 | 소스 `f7575aa4a8e2f0d97104033023bde12bf7b6d083`, Linux `40054217962`, macOS draft `qa-install-f7575aa` |

최종 판정은 위 13개 환경 실행의 `inventory`와 대상별 리포트를 따른다.
실패·미실행·skip 또는 정리 실패가 있는 필수 시나리오는 후보 전체를 실패시킨다.
개별 대상이나 후속 진단의 통과를 전체 수락으로 합치지 않는다. 후보는 공개 배포하지 않았다.

## 원인과 수정

- Ubuntu ARM의 느린 coldplug로 mount 작업이 먼저 만료되면 실제 BOOT·UEFI 장치를 확인한 뒤
  이미지의 원래 mount unit을 다시 시작한다. fstab·검증 조건은 바꾸지 않는다.
  [7개 VM 부팅 검사](https://github.com/mixrelay/mixrelay/actions/runs/40067495800)는 모두 통과했다.
  이 실행의 Ubuntu ARM은 TCG에서 정상 부팅했으며 emergency 복구 분기의 실제 발동을 뜻하지 않는다.
- HAL은 CoreAudio의 `ProcessOutput`에서 렌더링한 클라이언트를 식별한다.
  `WriteMix`에서는 OS가 합친 PCM만 한 번 복사한다. 엔진 자체 출력 때문에 외부 앱이
  렌더링한 것으로 판단하거나, 외부 앱을 놓쳐 입력 지연이 바뀌는 문제를 막는다.
  호스트 입력 지연은 0, 앱에 보고하는 측정 지연과 변경 알림은 유지한다.
- 초기 그래프 조회 중 백그라운드 연결이 끊기면 복구 가능한 조회 실패를 503으로 전달한다.
  편집기의 기존 초기 조회 복구를 사용하며, 보안·프로토콜 오류와 변경 명령의 불확실한 실패는 유지한다.
  실제 소켓 단절 회귀, 데스크톱 테스트 230개와 기존 UI 조회 복구 테스트 31개가 통과했다.
- macOS의 첫 시스템 오디오 동의는 native AXPress로 한 번 승인하고 창이 닫혔는지 확인한다.
  소유한 신호 앱은 장치 구성 변경 알림으로 엔진이 멈춘 경우 같은 장치 UID를 확인한 뒤 재개한다.
  두 독립 소스의 실제 신호와 녹음 PCM을 검사하며, 무음을 이유로 신호를 다시 시작하지 않는다.
- 설치 UI는 실제 빈 그래프와 노드 생성을 확인한 뒤 연결한다. 업그레이드 상태 보존을 검사한 후
  UI fixture를 준비할 때는 편집기를 다시 열어 저장한 빈 상태를 읽게 한다.
  macOS의 증거 수집은 편집 창 안에서 조회해 Apple 메뉴의 Recent Items를 열지 않는다.
- Driver·녹음 변경의 재제출은 서버가 적용 전에 거절했음을 확인한 경우로 한정한다.
  같은 revision·그래프·녹음 상태를 다시 확인하며 불확실한 결과는 반복하지 않는다.

네 파일의 크기·SHA-256과 지원 계약 해시는 [후보 JSON](mix-relay.candidate.json)에 고정한다.
기본 엔진 48kHz·128프레임, PCM 누락·반복·손상·비유한 값 0 조건과 녹음의 채널·주파수 검사를 유지한다.
콜백 시각 진단은 I/O 종료 뒤에 출력하며 PCM 판정이나 실시간 경로를 바꾸지 않는다.

## 설치 시험에서 확인한 후속 실패

[첫 전체 실행 40069992849](https://github.com/mixrelay/mixrelay/actions/runs/40069992849)의 실패 원시를 보존한다.
Debian·Ubuntu·Kali x64와 Arch는 두 설치 모드와 제거를 통과했다. macOS 15 ARM의 새 설치는
840프레임, macOS 14 ARM의 업그레이드는 128프레임이 누락됐다. 같은 시점 CoreAudio 로그에
마감시간·safety violation 경고가 있고 엔진 처리 시간은 예산 안이다. 이 관측만으로 원인을 확정하지 않는다.
새 실행은 지연 알림 횟수와 콜백 시각을 함께 기록하고 macOS 여섯 대상을 순차 실행한다.
128프레임 설정과 PCM 누락·반복·손상 0 조건은 유지한다.

[지연 알림 진단 40071958367](https://github.com/mixrelay/mixrelay/actions/runs/40071958367)에서
macOS 14 ARM은 두 설치 모드를 통과했다. macOS 15 ARM의 새 설치는 880프레임 누락이 재현됐고,
입력 지연 변경 알림은 0회였다. 같은 대상의 업그레이드는 PCM 누락·반복·손상 0이었다.
이 진단에서도 새 설치 실패를 유지하며 전체 수락에 성공 결과만 골라 합치지 않는다.

macOS 26 Intel은 제품 설치 전에 native 아이콘 조회가 15초 안에 돌아오지 않았고,
시스템 iconservicesagent의 연속 충돌 17회·Abort trap 6을 확인했다.
[GitHub 러너 이미지 문제 14751](https://github.com/actions/runner-images/issues/14751)와 같은 증상이다.
시험 도구는 이 조건을 모두 확인한 폐기형 hosted VM에서만 해당 에이전트를 해제한다.
해제 상태와 native 아이콘 조회의 복구를 다시 확인하며 실패하면 시험도 실패한다.
이때 시스템 fallback 아이콘을 사용한다. TCC 승인과 실제 오디오 검사는 그대로 수행한다.

macOS 15 Intel에서는 PKG 서명·Gatekeeper 확인 뒤 Apple stapler의 연결 단절(-1005)이
세 번 모두 발생했다. 제품 시험 전 실패로 보존하며 검증을 생략하지 않는다.

[이전 Ubuntu ARM 실행](https://github.com/mixrelay/mixrelay/actions/runs/40066104242)은 부팅과 두 설치 모드의
설치·실행·저장·녹음·제거를 통과했으나 이전 UI 시험과 업그레이드 PCM에서 실패했다.
Linux 시험은 같은 4초 신호를 쓰면서 재생 프로세스를 3초 뒤 중단해 TCG의 늦은 시작을 잘랐다.
이제 캡처의 첫 PCM과 유한 재생의 실제 drain을 기다린다. 데이터·채널·레벨 판정은 유지한다.

## 진단과 전체 수락의 구분

HAL 수정이 포함된 이전 소스 `79d0f67b`의
[macOS 27 ARM 진단](https://github.com/mixrelay/mixrelay/actions/runs/40068585053)은
새 설치·업그레이드의 필수 시나리오와 제거를 통과했다. 양쪽 PCM 누락·반복·손상·비유한 값은 0이었다.
화면 캡처 파일에는 빈 앱 본문이 찍혀 있어 이 이미지로 렌더링의 적합성을 판정하지 않는다.
실제 클릭·접근성 트리·저장된 그래프와 오디오 결과를 시각 확인과 구별한다.

[Kali x64 진단](https://github.com/mixrelay/mixrelay/actions/runs/40068112836)도 두 설치 모드를 통과했다.
두 실행은 일부 대상을 선택한 진단이므로 누락 대상을 실패시키는 전체 inventory는 통과하지 않는다.
이 결과를 현재 후보의 전체 통과로 승계하지 않는다.

이전 [1e513f21 후보](../2026-10-24-1e513f2/README.md)의 실패와 원시는 보존한다.
macOS 최소 지원 버전은 process tap API에 따른 14.2다. 정확한 14.2에서 실검증했다고 주장하지 않는다.
실행이 관측한 실제 패치·CPU·파일 해시를 보존하며, 지원 정책은
[제품 문서](../../../product.md)와 [지원 계약](../../../../support/environments.json)을 따른다.

문서는 `docs` 설정으로 교정했고 변경 산문의 문장부호 정규화는 변경 없이 끝났다.
초기 후보 기록 갱신의 보존 검사는 제품 문서 3개, 검증 현황 2개, 플랫폼 인벤토리 2개 오류로 실패했다.
후보 식별자·링크·상태표 갱신이 원인이며, 확인한 빌드와 실행에 대조해 변경 범위를 검토했다.
이 결과를 기계적 보존 검사 통과로 표시하지 않는다.
