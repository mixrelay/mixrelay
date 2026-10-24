# 설치본 진단: a4668a1a

제품과 시험 소스는 `a4668a1afdf33593bf3e74cdcabd01cdc421f063`이다.
네 설치 파일의 소스·크기·SHA-256과 지원 계약은 [후보 JSON](mix-relay.candidate.json)에 고정한다.
이 후보는 파일 잠금 probe의 명시적 unlock과 전체 native 콜백의 진입·종료 계측을 포함한다.
오디오의 48kHz·128프레임과 PCM 누락·반복·손상·비유한 값 0 조건은 유지한다.

| 항목 | 원시 결과 |
| --- | --- |
| Linux 두 CPU·Arch 빌드 | [40098568989](https://github.com/mixrelay/mixrelay/actions/runs/40098568989), 모두 통과 |
| macOS PKG | draft `qa-install-a4668a1`, 공증 Accepted·staple·validate 완료 |
| PKG SHA-256 | `ddab6a032c743214751ee632e5dd9d9f0fa009c9d4b3e34123e55fc3255ef315` |
| PKG 크기 | 69,409,064바이트 |
| Apple 공증 제출 | `031fabc9-ba71-4dcb-b3bc-cb4ba534c552` |
| Native CI 13개 대상 | [40098578695](https://github.com/mixrelay/mixrelay/actions/runs/40098578695), 모두 통과 |
| Linux VM 부팅 7개 대상 | [40098578712](https://github.com/mixrelay/mixrelay/actions/runs/40098578712), 모두 통과 |
| UI CI | [40098578691](https://github.com/mixrelay/mixrelay/actions/runs/40098578691), 통과 |
| 설치본 집중 진단 | [40099816586](https://github.com/mixrelay/mixrelay/actions/runs/40099816586), macOS 15 ARM·26 Intel, 실패 |

집중 진단은 두 대상만 선택했다. 나머지 11개를 검증한 실행이 아니며 전체 지원 행렬을
수락할 수 없다. 마지막 13개 전체 실행의 [fa09faed 결과](../2026-10-24-fa09fae/README.md)도
실패로 유지한다. 이 후보와 제품·업데이트 카탈로그를 공개하지 않았다.

## 제품과 독립 대조의 결과

| 대상·모드 | 누락 프레임 | 반복 프레임 | 손상 프레임 |
| --- | --- | --- | --- |
| macOS 15 ARM 새 설치 | 640 | 0 | 640 |
| macOS 15 ARM 업그레이드 | 0 | 0 | 0 |
| macOS 26 Intel 새 설치 | 8,860 | 1,792 | 20,956 |
| macOS 26 Intel 업그레이드 | 27,764 | 5,072 | 26,784 |
| macOS 15 ARM 독립 BlackHole | 2,048 | 0 | 2,048 |
| macOS 26 Intel 독립 BlackHole | 3,344 | 0 | 2,688 |

두 대상 모두 그래프 조작·녹음·시스템 캡처·설치·제거와 정리는 통과했다.
macOS 15 ARM 업그레이드는 모든 시나리오를 통과했으나 새 설치의 AUDIO-01 실패를 덮지 않는다.

독립 대조는 제품을 완전히 제거한 뒤 고정된 BlackHole 소스와 동일한 512프레임 측정기를
사용했다. 별도의 무음 출력 aggregate 클라이언트는 48kHz·128프레임과 입력 스트림 비활성을
실제 읽기로 확인했다. ARM에서는 1,822회 콜백 중 샘플 시각 단절 3회, Intel에서는 1,961회 중
4회가 있었으며 잘못된 프레임 크기는 0회였다. 두 독립 대조의 정리도 통과했다.

## 콜백 시각이 설명하는 범위

측정기 콜백의 진입·종료 호스트 시각으로 측정 구간을 잡고, 같은 구간의 엔진 클럭 단절 기록을
대조했다. ARM의 입력 콜백 직전 처리 시간은 각각 5.042µs·4.000µs였지만 콜백 밖의 호출 간격은
14.006ms·18.052ms였다. Intel 새 설치에서는 직전 입력 콜백 2.453µs 뒤 40.434ms,
업그레이드에서는 3.717µs 뒤 108.326ms의 간격을 기록했다. 128프레임·48kHz의 주기는 약 2.667ms다.
일부 Intel 출력 콜백의 경과 시간도 이 주기를 넘었다. 경과 시간은 CPU 실행 시간과 구별하며,
모든 지연이 콜백 밖에 있었다고 단정하지 않는다.

이 호스팅 환경의 독립 오디오 경로도 MixRelay 없이 PCM 손실과 클럭 단절을 낸다는 증거다.
제품에서 관찰한 모든 손실의 단일 원인을 증명하거나 제품 실패를 통과로 바꾸지는 않는다.
원래 신호·실패·원시 기록을 보존하며 재생·구간 제거·허용치 변경으로 수락하지 않는다.
