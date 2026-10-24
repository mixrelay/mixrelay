# 설치본 검증: 3c72c55c

제품 소스는 `3c72c55c7dad5755d7dbf630e42db2afee6618dd`, 설치 시험 코드는
`4f107f236dc2b3ffd9b307836f3506021ece652e`다. 두 커밋 사이의 제품 실행 소스는 같으며,
설치 시험·호스트 준비·진단·운영 문서만 바뀌었다. 후속 시험 수정도 제품 실행 파일을 바꾸지 않는다.
Universal PKG 하나, CPU별 DEB와 같은 x64 payload의 Arch 패키지를 사용한다.
네 파일의 크기·SHA-256과 지원 계약 해시는 [후보 JSON](mix-relay.candidate.json)에 고정한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40083449132](https://github.com/mixrelay/mixrelay/actions/runs/40083449132), 모두 통과 |
| macOS PKG | draft `qa-install-3c72c55`, 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `e21989692623ba38a9ba6922c9060e9e9343dd15a38c6efccdb02ed1aab9846f` |
| PKG 크기 | 69,400,310바이트 |
| Apple 공증 제출 | `5623bee9-9a6b-4161-ba4f-e9d0218cc8f5` |
| 13개 환경의 fresh·upgrade·제거 | [40089143239](https://github.com/mixrelay/mixrelay/actions/runs/40089143239), suite `4f107f23`, 최종 실패 |
| Native CI 13개 대상 | [40089948523](https://github.com/mixrelay/mixrelay/actions/runs/40089948523), 소스 `f0427c53`, 모두 통과 |
| Linux VM 부팅 7개 대상 | [40089948524](https://github.com/mixrelay/mixrelay/actions/runs/40089948524), 소스 `f0427c53`, 모두 통과 |
| UI CI | [40089948514](https://github.com/mixrelay/mixrelay/actions/runs/40089948514), 소스 `f0427c53`, 통과 |
| 이전 설치본 | 소스 `f7575aa4a8e2f0d97104033023bde12bf7b6d083`, Linux `40054217962`, macOS draft `qa-install-f7575aa` |

후보 전체의 최종 수락은 위 13개 환경 실행의 `inventory`와 대상별 리포트를 따른다.
이 후보는 macOS의 PCM 실패로 수락할 수 없다. 확인한 실패는 다음과 같다.

| 대상·모드 | 실패 |
| --- | --- |
| macOS 15 ARM 업그레이드 | Destination 목록의 Recorder 미선택, PCM 누락·손상 640프레임 |
| macOS 15 Intel 업그레이드 | 그래프 준비·창 최소화 성공 뒤 PCM 누락 7,832·반복 512·손상 22,432프레임 |
| macOS 26 ARM 새 설치·업그레이드 | 새 설치는 그래프 준비·창 최소화 성공 뒤 PCM 누락·손상 12프레임, 업그레이드는 Destination 목록의 Recorder 미선택 |
| macOS 26 Intel 새 설치·업그레이드 | 두 모드의 그래프 준비·창 최소화 성공 뒤 PCM 실패. 새 설치 누락·손상 1,772프레임, 업그레이드 누락 12,440·반복 2,560·손상 5,212프레임 |
| macOS 27 ARM 업그레이드 | Destination 목록의 Recorder 미선택. 두 모드의 PCM·녹음·시스템 캡처·제거는 통과 |
| Ubuntu 26.04 ARM 새 설치·업그레이드 | 부팅·API·오디오·녹음·제거 통과, UI 실패. 새 설치는 Recorder 메뉴가 선택되지 않았고 업그레이드는 Recorder 준비 확인 실패 뒤 연결 오류 화면이 남음 |

모든 필수 시나리오와 정리가 통과해야 한다.
실패·미실행·skip은 수락할 수 없으며, 개별 진단의 통과를 모아 전체 실행을 대신하지 않는다.
후보는 draft로 보관하며 제품이나 업데이트 카탈로그를 공개하지 않았다.

최종 inventory는 13개 대상을 모두 실행해 7개 통과·6개 실패를 기록했다. skip·미실행 대상은 없다.
`fa09faed`는 숨긴 웹뷰의 상태·미터 전송을 1초로 제한하고 복귀 때 즉시 최신 창을 보내는 제품 수정과,
오디오·녹음 종료 뒤 진단 도구를 실행하는 시험 수정을 포함한다. 제품 실행 파일이 달라졌으므로
이 후보의 기존 설치 파일로 그 수정까지 검증했다고 판단할 수 없다. 같은 새 소스에서 macOS·Linux 설치 파일을 다시 빌드한다.

후속 suite `d26298fe`의 [macOS 15 ARM 진단](https://github.com/mixrelay/mixrelay/actions/runs/40090519269)은
선택 항목도 실제 포커스 확인 뒤 Return으로 확정하고 그래프 실패 뒤에도 오디오 준비를 수행한다.
이 대상의 새 설치·업그레이드는 모든 필수 시나리오와 정리를 통과했다. 다른 12개 대상은 선택하지 않았으므로
최종 inventory는 실패하며, 앞선 전체 실행의 실패를 이 통과로 대체하지 않는다.
suite `62ab6005`의 [오디오 진단](https://github.com/mixrelay/mixrelay/actions/runs/40092127279)은
측정 직후 전체 Float32 녹음과 콜백 진입·종료 시각을 보존하고, PCM 실패 뒤 WebContent 스택을 수집한다.
suite `ec61d7c0`의 [독립 장치 비교](https://github.com/mixrelay/mixrelay/actions/runs/40092326510)는
MixRelay 제거가 끝난 같은 VM에서 고정한 BlackHole 소스와 같은 측정기를 사용한다.
독립 장치는 기본 버퍼를 사용하고 엔진 클라이언트가 없으므로, 그 장치의 통과만으로 제품의 콜백 기한을
VM이 충족한다고 판단할 수 없다. 두 진단은 제품의 실패 결과와 전체 수락 조건을 유지한다.

## 수정과 확인 범위

- Ubuntu ARM coldplug 복구와 기존 오디오·설치 수정은 [이전 후보 기록](../2026-10-24-c525918/README.md)에 둔다.
  소스 `c93aa307`에서 Ubuntu ARM을 포함한 VM 7개의 실제 부팅 검사가 통과했다.
- macOS 27.0의 CPU 3개 hosted VM은 DOM·접근성 트리가 살아 있어도 WebContent의 IOSurface 생성이
  거부돼 화면이 비었다. 제품은 macOS 27.0에서 프로세서가 4개 미만이면 첫 webview를 만들기 전에
  GPU-process DOM 렌더링을 켠다. WebKit의 private feature API 유무와 적용값을 확인하며,
  현재 창 설정을 그대로 사용한다. 같은 제품 브리지를 쓰는 native fixture로 빈 창을 재현하고
  수정 후 실제 창에 내용이 표시되는 것을 직접 확인했다. WebKit 자체 snapshot만으로는 보이는 창의
  렌더링을 확인할 수 없었다. 진단 실행 40086233758의 macOS 27 새 설치는 모든 필수 시나리오와 제거를
  통과했고, 실제 설치본의 창 캡처에서도 두 노드와 연결선이 표시되는 것을 직접 확인했다.
  업그레이드는 컨텍스트 메뉴 조작에 실패했으므로 해당 대상 전체를 통과로 표시하지 않는다.
- macOS 메뉴는 실제 포커스를 확인한 뒤 Return을 한 번 보낸다. Linux의 Add node도
  포커스와 메뉴 펼침을 확인한다. 이전 Ubuntu ARM 실행은 새 설치의 모든 시나리오와 제거를
  통과했지만, 업그레이드는 열리지 않은 Add node 메뉴 뒤에서 출력 항목을 찾다가 실패했다.
  두 모드의 오디오·녹음·제거 통과를 그래프 실패의 대체 근거로 삼지 않는다.
- macOS 노드의 컨텍스트 메뉴는 바깥 노드 그룹의 실제 포커스를 확인한 뒤 Shift+F10으로 연다.
  이전 macOS 15·26·27 업그레이드는 노드 두 개가 생성돼도 우클릭 메뉴가 열리지 않았다.
  버튼과 Destination 팝업도 실제 활성화·포커스를 확인한 뒤 Return을 한 번 보낸다.
  이전 macOS 15 Intel은 Destination 팝업이 열리지 않아 Recorder 항목 조회가 실패했다.
  관찰자의 XCTest 빌드는 통과했으며 설치본 결과는 위 전체 실행에서 확인한다.
- hosted macOS 준비는 폐기할 루트·Data 볼륨의 Spotlight 활동을 `mdutil -d`로 끄고 전후 상태를 기록한다.
  인덱싱만 꺼진 상태는 활동 중지로 판정하지 않는다. 진단 40086233758에서 두 볼륨의 활동 중지를
  확인했지만 macOS 14 업그레이드는 여전히 640프레임 누락·손상으로 실패했다. 같은 실행의 macOS 15·27은
  두 모드의 PCM 검사를 통과했다. macOS 14 실패 직후 호스트 CPU 유휴 비율은 0~0.29%였으며,
  CoreAudio의 사이클 건너뜀도 관찰됐다. 렌더러 시간 측정만으로 전체 콜백 실행 시간이나 최종 원인을 단정하지 않는다.

첫 전체 실행 [40085194395](https://github.com/mixrelay/mixrelay/actions/runs/40085194395)은
Linux 메뉴가 실제로 열려도 트리거의 expanded 플래그를 요구한 시험 오류와 macOS의 PCM·메뉴 실패를
확인한 뒤 수정한 전체 실행으로 교체하며 중단했다. Linux는 실제로 표시된 메뉴 컨테이너를 기다리며,
진단 40086233758의 Ubuntu x64 새 설치·업그레이드·제거는 모두 통과했다. 이 진단 결과를 전체 수락에 합치지 않는다.

후속 전체 실행 40087553288의 macOS 14는 자동 검사를 모두 통과했지만, 업그레이드의 창 캡처에는
녹음기 상태 조회의 `node_not_found` 오류가 표시됐다. 캡처 직후 접근성 트리에서는 오류가 이미 사라졌으므로
이 한 장으로 정상 준비가 완료됐다고 판단하지 않았다. 두 플랫폼의 관찰자는 Record 버튼의 실제 활성화와
녹음 오류 부재를 확인한 뒤 캡처한다. 이 상태가 끝내 확인되지 않으면 그래프 검사를 실패시킨다.
앞선 실행은 미수락 상태로 보존하며, 보강한 전체 실행에서 13개 대상을 다시 검사한다.
녹음기 준비 확인을 추가한 40088213219도 이전 Intel 실패의 Destination 팝업 조작을 보완한 실행으로
교체하며 중단했다. 취소된 대상과 이미 통과한 개별 결과를 최종 전체 실행에 합치지 않는다.

로컬에서는 데스크톱 테스트 232개(ignored 0), 엄격 clippy, 공통 스크립트 556개(skip 0),
실제 WebKit·픽셀·서명 경계 시험 3개와 XCTest 관찰자 빌드가 통과했다.
제품 엔진의 기본 48kHz·128프레임과 PCM 누락·반복·손상·비유한 값 0 조건은 유지한다.
왕복 측정기는 네이티브 버퍼 크기를 별도로 기록하며, 위 실패 세션에서는 512프레임이었다.
신호 사전 재생, 실패 후 재측정이나 PCM 구간 제거는 하지 않는다.

macOS 지원 하한은 process tap API에 따른 14.2다. 실제로 실행한 패치와 CPU는 대상 리포트에 기록한다.
정확히 14.2와 macOS 14 Intel을 실행했다고 주장하지 않으며 별도 필수 대상에도 추가하지 않는다.
운영 절차와 최종 판정은 [릴리스 검증](../../../release-qualification.md)을 따른다.
