# 설치본 진단: a78e85ef

제품과 시험 소스는 `a78e85ef1785d7c9cdf99192e5e4a20781023e2d`이다.
네 설치 파일의 소스·크기·SHA-256과 지원 계약은 [후보 JSON](mix-relay.candidate.json)에 고정한다.
오디오의 48kHz·128프레임과 PCM 누락·반복·손상·비유한 값 0 조건은 유지한다.

| 항목 | 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40111018879](https://github.com/mixrelay/mixrelay/actions/runs/40111018879), 모두 통과 |
| macOS PKG | draft `qa-install-a78e85e`, 서명·공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `e7b3e65da1356cd5ed828dba9c9f83cb18976317acca2583228e60ec2eccdda4` |
| PKG 크기 | 69,410,301바이트 |
| Apple 공증 제출 | `6fd6c825-6788-46f3-bc58-b1da3b8a128f` |
| Native CI 13개 대상 | [40111011338](https://github.com/mixrelay/mixrelay/actions/runs/40111011338), 모두 통과 |
| Linux VM 부팅 7개 대상 | [40111011328](https://github.com/mixrelay/mixrelay/actions/runs/40111011328), 모두 통과 |
| UI CI | [40111011161](https://github.com/mixrelay/mixrelay/actions/runs/40111011161), 통과 |
| 전체 설치본 검증 | [40112484768](https://github.com/mixrelay/mixrelay/actions/runs/40112484768), 7개 통과·5개 실패·Ubuntu ARM 설치 전 부팅 중단 |

GitHub draft에서 내려받은 PKG와 소스 기록은 로컬 원본 및 GitHub asset 해시·크기와 일치한다.
Linux 패키지는 같은 소스 SHA의 성공한 빌드에서 받았고, 선택기가 네 파일을 모두 확인했다.
업그레이드 기준은 소스 `f7575aa4a8e2f0d97104033023bde12bf7b6d083`의 네 설치 파일이다.
다른 실행의 통과 결과와 합산하지 않는다. [원시 inventory](full-inventory.md)가 이 전체 실행의 최종 판정이다.

## Linux 명령 처리와 관찰

Linux 장치·앱 검색은 동기식 자식 프로세스를 기다렸다. 이를 비동기 명령 처리 안에서 직접
실행하면 다른 제어 작업과 해당 요청의 생존 응답까지 멈췄다. 기존 시간 제한이 있는 검색을
별도 작업 스레드로 옮겼으며, 수락한 검색은 종료 시 완료를 기다려 자식 프로세스를 회수한다.
동일한 단일 스레드 런타임에서 검색과 독립 제어 작업을 함께 실행한 회귀 시험은 변경 전에
응답하지 못해 실패했고 변경 뒤 통과했다. 비동기 읽기 취소와 변경 명령 재전송 금지는 유지한다.

AT-SPI 초점 요청의 응답이 누락되면 관찰기는 캐시를 갱신한 실제 초점 상태를 확인한다.
초점을 확인한 경우에만 Return을 한 번 보내며 초점 요청은 반복하지 않는다. 초점이 없으면
기존 관찰 제한 안에서 실패한다. 이 회귀 시험도 변경 전 실패·변경 후 통과를 확인했다.
녹음 준비·녹음 오류 검사와 설치본 UI 수락 조건은 유지한다.

로컬 데스크톱 시험 237개(제외 0개), 엄격 clippy, 크레이트 계층 검사, E2E 도구 시험 160개,
Linux 관찰기 시험 19개가 통과했다. Arch의 실제 Linux CI에서도 검색 중 제어 작업과 종료 시
작업 회수의 회귀 시험이 통과했다. 실제 설치본 확인은 위 전체 실행의 최종 inventory로 판단한다.

## 이전 실패의 보존

[42088b93 후보](../2026-10-24-42088b9/README.md)의 Ubuntu ARM 새 설치·업그레이드 UI 실패,
macOS 26 ARM·Intel PCM 실패와 ARM 업그레이드의 기준 PKG 설치 실패는 유지한다.
이번 도구는 Installer의 상세 stderr와 `/var/log/install.log`도 수집한다. 앞선 PCM 실패를
허용치 변경·신호 재생·측정 구간 제거로 통과 처리하지 않는다. 제품·업데이트 카탈로그는 공개하지 않았다.

## 전체 실행의 실패와 후속 부팅 확인

macOS 15 ARM 업그레이드는 PCM 누락·손상 각 608프레임, 15 Intel 새 설치는 누락
17,636프레임, 26 Intel 업그레이드는 누락 11,996·반복 2,752·손상 6,272프레임으로
실패했다. 이 세 대상의 나머지 시나리오와 정리는 통과했다.

Debian ARM 업그레이드는 Recorder 준비에 실패했다. 실제 창의 진단에는 로컬 엔진 연결 오류와
`node_not_found`가 표시됐으며 API·오디오·녹음·제거·정리는 통과했다. Arch는 새 설치를
통과했지만 업그레이드 기준 설치본의 Driver가 SIGKILL로 종료돼 후보 설치에 도달하지 못했다.
기존 journal은 종료 신호를 기록했으나 발신자를 확인하지 못했다.

Ubuntu ARM은 EFI 마운트 지연 뒤 원래 마운트 복구를 완료했지만 cloud-init 설정·최종 단계와
SSH가 시작되지 않아 설치 전에 중단됐다. 시험 도구 소스 `8c85aeb1`은 정상 부팅 전환과
cloud-init 후속 단계의 시작을 요청한다. [후속 VM 준비 실행](https://github.com/mixrelay/mixrelay/actions/runs/40114749697)은
7개 환경 모두 통과했다. 이때 Ubuntu ARM은 비상 부팅에 진입하지 않았으므로 해당 실행만으로
복구 분기가 실환경에서 검증됐다고 판단하지 않는다. 설치본 실패 판정도 유지한다.

전체 설치본 실행의 실패 상태는 PR 소스 커밋에도 기록했다. 전체 실행은 같은 상태를 자동으로
갱신하며, 일부 대상만 고른 진단은 전체 지원 계약의 통과 상태를 만들지 않는다.
