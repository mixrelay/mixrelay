# Loopback 절차 대조와 PCM 경로 수정

분석 대상은 Loopback 2.5.0의 ARK 13.5.2 arm64와 MixRelay 설치 후보 `a78e85ef`다.
ARK 바이너리 SHA-256은 `6d3593a316192e07e0970a6e110714170ee1d03c1b53327bcb01119ef3473b0d`다.
기존 Ghidra 전체 export를 재사용하고 필요한 함수는 REA Ghidra로 추가 분석했다.
정적 분석과 기존 설치본 원시 자료를 대조했으며 새 로컬 오디오·서비스 접속은 실행하지 않았다.

## 절차별 대조

| 절차 | ARK에서 관찰한 구현 | MixRelay의 실제 경로 |
| --- | --- | --- |
| 출력 수집 | `ECVirtualDevice::writeToStreamLocked:` (`0x1a260`)가 클라이언트별 MixOutput PCM을 공유 큐에 합산 | HAL WriteMix가 OS에서 혼합한 PCM을 render·capture 링에 발행 |
| 시간축 | `ACPUnstableClock` 블록 `0x28644`가 연속 sample·host 진행을 확인하고 프레임을 누적하며 불연속 시 재앵커 | HAL `clock.rs`의 공칭 host clock과 CoreAudio가 제공한 sample 좌표 사용 |
| 큐 | read `0x14694`, write `0x1482c`, erase `0x148d0`; 범위 밖은 무음. erase의 release watermark 뒤에 실제 mix가 이어짐 | 프레임별 epoch stamp로 발행·교체를 확인하며 미발행 프레임은 무음 |
| 엔진 실행 | `ACPStableIOClock` tick `0x35d20`이 미래 경계에 one-shot을 예약하고 별도 큐에서 처리 | 검증 그래프는 CoreAudio IOProc의 direct 렌더러에서 실행 |
| 스레드 정책 | `ACPDispatchQueue` 블록 `0x8fe14`: period·constraint 1ms, computation 0.1ms, preemptible | 해당 검증 경로는 CoreAudio의 스케줄링을 유지. 대체 워커의 RT budget은 이 경로에서 사용하지 않음 |

ARK `ECPlugInDevice::init` (`0x14f8`)은 `clok=raww`를 설정한다.
MixRelay는 이 속성을 생략하므로 Apple SDK가 정한 기본 IIR 필터를 사용한다.
고정 BlackHole 대조 소스도 같은 속성을 생략하므로 이 차이를 PCM 결함으로 확정하지 않았다.
Loopback의 버퍼·오프셋·클럭 주기·RT budget을 MixRelay에 복사하지 않았다.

주요 REA Evidence는 `ev_21b9420f8e2f72c6cd2ba2cb2ad37f44dc4182fda7455c79bbf71cd820f6191f`(초기화),
`ev_e90ca3da87216653406ffe0a4c6f399dce13aef8d0c6adf770ba013442a5cd75`(클럭),
`ev_f90ab47c0734c6c31b06a40bcea2f5b3ff7cf7ca0f2cbf83f5ca92904f9cf5cd`(erase),
`ev_7392c05f4c67325e6a7f66d4c7186b798000f83347b854d1cc2fc30e9736f38b`(read)다.

## 기존 PCM과 콜백 좌표

| 실행·측정 | 프로브 무음의 장치 sample 범위 | 엔진 output 콜백의 expected → actual | 건너뛴 프레임 |
| --- | --- | --- | --- |
| `40191991188` Mac 15 Intel 신규 | 124028–124448, 420프레임 | 124028 → 124576 | 548 = 420 + 128 |
| `40155560097` Mac 15 Intel 신규 | 167676–168348, 672프레임 | 167676 → 168476 | 800 = 672 + 128 |
| `40155560097` Mac 15 Intel 업그레이드 | 114232–115124, 892프레임 | 114232 → 115252 | 1020 = 892 + 128 |
| `40155560097` Mac 26 Intel 업그레이드 | 89736–90604, 868프레임 | 89736 → 90732 | 996 = 868 + 128 |

범위의 끝은 exclusive다. 각 구간 직전 엔진 callback 본문은 약 18–23µs였으며 다음 entry까지
약 10.7–20.7ms가 지났다. 장시간 DSP 실행이 그 간격을 점유했다고 설명할 근거는 없다.
ARM의 대응 관계는 이 표와 달랐으므로 전체 플랫폼의 단일 원인으로 일반화하지 않는다.
앱 녹음도 같은 native IOProc의 누락을 겪을 수 있다. 녹음에서 해당 신호가 보이지 않는다는 사실을
HAL 발행 자체가 없었다는 독립 증거로 사용하지 않는다.

## 소스에서 재현한 결함과 수정

1. 작성 그래프의 읽기 참조가 composition에서 양방향 canonical spec으로 확대됐다.
   장치에서 읽기만 하는 검증 그래프도 빈 mixer를 통해 무음 출력 클라이언트를 열고 이를 direct master로 골랐다.
   실행 그래프들의 읽기·쓰기 참조를 합산해 필요한 클라이언트 방향만 생성한다. Driver canonical 설정은 유지한다.
2. 입력 전용 IOProc은 사용하지 않는 출력 스트림을 켜 두었다. 해당 등록의 출력만 끄고 readback을 확인하며,
   스트림 topology가 바뀌면 다시 준비한다. 다른 클라이언트의 스트림 사용은 바꾸지 않는다.
3. 입력 master도 output timestamp로 렌더링 좌표를 만들었다. master 방향의 sample·host를 함께 사용한다.

합성 시험은 실제 `qualification-stereo.json`을 lowering·composition·native spec·가짜 HAL 준비 경계로
통과시킨다. 입력 owner·IOProc 한 개와 종료 시 회수를 확인하고 원래 Driver 양방향 2채널을 유지한다.
입력 timestamp 회귀 시험과 출력 스트림 설정 실패 시험은 변경 전에 실패했다.
이 소스 수정이 설치본의 PCM 누락을 해결했는지는 아직 확인하지 않았다. 기존 13개 전체 실행의
8개 통과·5개 실패 판정과 PCM 누락·반복·손상·비유한 값 0 조건은 유지한다.

로컬 원시 증거는 `~/Library/Application Support/MixRelay/Diagnostics/installed-e2e-20261025/`의
검증한 `evidence.tar.zst`와 `current/`에 보존한다. REA 추가 함수는 같은 위치의
`loopback-ark-procedures.evidence.json`에 남겼다. `/Volumes/dev`의 임시 증거 폴더와 짧은 검증 체크아웃은 제거했다.
