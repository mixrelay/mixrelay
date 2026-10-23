# 후보 0.1.0 (`8582d15`) 설치본 결과

필수 환경 전체 수락은 0/15다. [전체 실행](https://github.com/mixrelay/mixrelay/actions/runs/39945522752)의
13개 작업이 끝났고 12개 환경에서 실제 설치 파일과 설치된 실행 파일의 일치를 확인했다.
Ubuntu ARM은 VM 준비가 20분 제한을 넘어 앱 설치 전에 종료됐다.
[Ubuntu ARM 후속 실행](https://github.com/mixrelay/mixrelay/actions/runs/39949458383)은 같은 DEB에
VM 준비 제한을 35분으로 늘린 도구를 적용한다. 후속 결과는 전체 실행의 통과로 합치지 않는다.

Ubuntu x64·Arch x64·Debian ARM은 그래프 저장/재열기, native stereo PCM, 종료된 RF64 녹음 검사를
모두 통과했다. Linux 준비 상태 오류와 설치본 UI 미구현이 남아 이 세 환경의 전체 수락도 실패다.
아래 표에서 `차단`은 선행 실패 또는 미구현으로 해당 검사를 완료하지 못했다는 뜻이고,
`미실행`은 실제 시험 결과가 없다는 뜻이다. 설치 성공은 환경 전체의 출시 수락을 뜻하지 않는다.

| 실제 환경 | CPU | 설치·파일 일치 | API 준비 | 그래프 저장/재열기 | native PCM | RF64 녹음 | 정리 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| macOS 14.8.9 | ARM | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| macOS 15.7.9 | ARM | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| macOS 15.7.9 | Intel | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| macOS 26.6.2 | ARM | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| macOS 26.6.1 | Intel | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| macOS 27.0.1 | ARM | 통과 | 통과 | 차단 | 실패 | 차단 | 실패 |
| Debian 13.7 | x64 | 통과 | 실패 | 실패 | 차단 | 차단 | 실패 |
| Debian 13.7 | ARM | 통과 | 실패 | 통과 | 통과 | 통과 | 통과 |
| Ubuntu 26.04.1 LTS | x64 | 통과 | 실패 | 통과 | 통과 | 통과 | 통과 |
| Ubuntu 26.04 ARM | ARM | 미실행: VM 준비 제한 | 미실행 | 미실행 | 미실행 | 미실행 | 설치본 미실행 |
| Kali rolling 2026.1 | x64 | 통과 | 실패 | 차단 | 차단 | 차단 | 통과 |
| Kali rolling 2026.1 | ARM | 통과 | 실패 | 통과 | 실패 | 실패 | 실패 |
| Arch rolling | x64 | 통과 | 실패 | 통과 | 통과 | 통과 | 통과 |
| macOS 14.2 | ARM | 미실행: hosted 버전 선택 불가 | 미실행 | 미실행 | 미실행 | 미실행 | 미실행 |
| macOS 14.2 | Intel | 미실행: hosted runner 없음 | 미실행 | 미실행 | 미실행 | 미실행 | 미실행 |

설치본 UI 조작 `GRAPH-01`은 모든 설치 환경에서 미구현으로 차단됐다.
macOS 무료 전체 시스템 캡처 `AUDIO-11`도 미구현으로 차단됐다.
UI·권한 대화상자·업그레이드·제거·플러그인·전체 39개 과거 시나리오를 검증했다는 뜻은 아니다.
여기서 실행한 필수 기준선은 [지원 계약](../../../../support/environments.json)의 7개 시나리오다.

## 실패 원인과 검증 한계

- macOS 여섯 환경: native 그래프 활성화에서 CoreAudio 장치 초기화 `Err(Timeout)`로 실패했다.
  엔진 표본은 `AudioDeviceCreateIOProcID`의 서버 응답 대기를 기록했다. 근본 원인을 확정한 것은 아니다.
  PCM 측정과 녹음은 진행하지 못했다. 정리는 그래프 변경 완료를 확인하지 못해 실패로 유지했다.
  macOS 26 Intel에서는 제품의 정상 Quit 응답도 시간 초과했다.
- Linux 설치 환경: 엔진의 `driverState: not_installed`로 준비 검사가 실패했다. Kali x64를 제외한
  다섯 환경에서는 Driver API가 Ready여서 엔진 보고와 불일치했다.
  `APP-01` 실패를 유지한 채 실제 Driver 버전·상태와 엔진 protocol·power·native backend를
  대조한 뒤 독립 오디오 검사를 진행했다. `readiness-disagreement.json`이 두 응답을 보존한다.
- Debian x64: Driver HTTP 요청 실패로 그래프 적용에 실패했다. 뒤이은 녹음 요청과 정리에는
  `graph_exchange_busy`가 발생했다. 오디오·녹음은 선행 실패로 차단됐으며 측정 실패로 집계하지 않는다.
- Kali x64: Driver가 `Driver control queue is full`을 반환해 독립 검사를 시작하지 못했다.
- Kali ARM: 그래프 저장/재열기는 통과했다. PCM peak는 기대값 `[0.125, 0.25]`의 두 배인
  `[0.25, 0.5]`로 실패했다. 녹음 시작은 진행됐지만 종료·검증과 정리는 `graph_exchange_busy`로 실패했다.
  [보존 PCM 재분석](signal-level-review.json)의 해시는 원시 artifact와 같다.
- Ubuntu ARM: 전체 실행에서는 앱 설치 전 VM 패키지 준비 제한에 걸렸다. 제품 실패와 구분한다.
- Linux x64는 KVM, ARM은 TCG Cortex-A72 VM에서 실행했다. ARM 결과는 기능 검사이며
  물리 하드웨어의 실시간 지연·부하·오디오 장치 수락을 보장하지 않는다.
- Kali 공식 이미지 파일명은 `2026.2`이지만 게스트의 실제 OS 보고는 `2026.1`이다.
  Arch와 Kali는 rolling 표기만으로 환경을 특정하지 않고 커널·의존성·패키지 목록 해시를 기록한다.

## 후보와 원시 근거

제품 소스는 `8582d158455c56006e4288146dcc646100c34bb5`, 전체 실행의 테스트 도구는
`96c1bc1840c46e214f20b668222394c44ec7d8eb`다. Ubuntu ARM 후속 도구는
`e5cbbd21870f9eee7edc2e23b793ad5f63d97c76`이며 제품 파일은 재빌드하지 않았다.

- [후보 JSON](mix-relay.candidate.json): 같은 Universal PKG, CPU별 DEB 2개, Arch 패키지의 크기·SHA-256·소스·계약 해시.
- [공증 결과](macos-build.json): PKG notarized·stapled·validated. `qa-install-8582d15`는 draft release다.
- [Linux 빌드](https://github.com/mixrelay/mixrelay/actions/runs/39939576577): native x64·ARM 빌드.
  [Arch payload 대조](arch-payload-proof.json): 세 실행 파일은 x64 DEB와 바이트 단위로 같다.
- [전체 실행 인벤토리](inventory.md): 15개 필수 환경의 실제 버전·커널·의존성·시나리오 결과.
  같은 원시 보고서를 후속 도구로 다시 렌더링한 표다. 원시 색인의 `inventory.md` 해시는 CI 원본의 해시다.
- [관찰 원문](observations.json): 전체 실행의 12개 설치본 보고서와 7개 VM 실행 기록. publication receipt가 아니다.
- [원시 파일 색인](raw-evidence-index.json): 전체 실행 artifact의 200개 파일 경로·크기·SHA-256.
  설치 파일, PCM·RF64, API 응답, 앱 로그·엔진 표본과 VM 준비 로그는 Actions의
  `installed-e2e-evidence` artifact에 90일 보존한다. 저장소에는 바이너리를 중복 저장하지 않는다.

필수 누락·실패·차단·정리 실패를 gate가 거부했다. 업데이트 카탈로그를 게시하거나 정식 릴리스를 만들지 않았다.
이 후보의 배포 판단과 남은 제품 작업은 [제품 명세](../../../product.md)가 기준이다.
