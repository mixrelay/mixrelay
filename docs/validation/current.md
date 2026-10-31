# 검증 현황

## 2026-11-01 macOS 지원 하한

v0.1.0의 최소 지원 버전은 macOS 15.0이다. GitHub의 2026-11-02 macOS 14 hosted runner 종료로
해당 주 버전의 출시 검증을 계속할 수 없어 지원 대상에서 제외한다. process tap API의 도입 버전은 14.2다.
현재 계약은 macOS 15·26의 arm64/x64와 27의 arm64, Linux 7개 환경으로 총 12개 설치본 대상을 요구한다.
Intel 지원은 Apple 지원 모델의 macOS 15.0–26으로 유지한다. 아래 macOS 14 결과와 이전 대상 수는
당시 후보·계약의 이력이며 현재 지원 범위나 새 후보의 검증 결과로 승계하지 않는다.

제품 동작·범위·남은 작업은 [product.md](../product.md)가 기준이다. MixRelay는 아직 출시하지 않았다.
이 문서는 소스 검사와 로컬·GitHub Actions 설치본의 검증 범위를 구분한다.
OS·CPU별 지원 범위와 같은 사용자 시나리오의 확인·부분·미확인 결과는
[OS별 지원·검증 인벤토리](platform-matrix.md)에서 비교한다. 각 결과의 실행 날짜·빌드는 아래 기록을 따른다.

## 2026-10-27 같은 네 설치 파일로 13개 수락 완료

제품·검사 소스 `c198dd16b4f6750f74a48d5d4e8a414f7534899f`의
[전체 설치본 실행 40480070783](https://github.com/mixrelay/mixrelay/actions/runs/40480070783)은
단일 실행(attempt 1)에서 13개 대상의 새 설치·업그레이드와 정리를 모두 통과했다.
같은 최종 설치 파일 4개의 해시를 고정했으며 필수 시나리오 결과 116개 통과·skip 0이다.
다운로드한 전체 증거의 SHA-256, 설치 파일 4개와 참조한 원시 증거 507개의 해시,
13개 보고서의 소스·계약·실행 번호·attempt를 다시 대조했고 로컬 수락 판정도 통과했다.

수락은 실제 HAL 쓰기·읽기와 엔진 녹음의 PCM 보존을 검사한다.
OS가 공급하지 않은 구간은 원래 진단에 남기며 제품 입력으로 합산하지 않는다.
macOS 6개 대상의 새 설치·업그레이드에서 기본 녹음과 무음·유음 양 순서의
녹음 구간 36건을 확인했다. 게시된 유음의 누락·중복과 대응 무음 구간의 비영점 PCM은 모두 0이다.

이전에 실패한 macOS 15 Intel은 두 설치 모드의 각 입력 순서에서 유음 144,000프레임을
보존했고 대응 무음 144,000프레임의 비영점 PCM은 0이었다.
전체 시스템 녹음은 모드별 144,384프레임이며 양쪽 톤이 유지됐다.
실제로 로드된 HAL은 trace v5이고 기본 측정·양 입력 순서의 trace에서 admission 거부는 0이다.
이전 trace v4의 거부 1건에 사용된 인자와 OS 콜백 공백의 원인은 여전히 단정할 수 없다.
이전 실행의 실패 판정은 유지한다.

[소스 CI 40478160331](https://github.com/mixrelay/mixrelay/actions/runs/40478160331)은
13개 OS·CPU 조합을 통과했으며 UI 검사와 VM 준비도 통과했다.
로컬 native 회귀 1,196개·스크립트 650개를 통과했고 새 ignore는 추가하지 않았다.
기존 benchmark·명시 실환경·자식 프로세스 전용 fixture 7개는 일반 native 실행의 제외 대상으로 별도 기록했다.
캡처 전송·ABI·요청 검증을 분리한 뒤 Desktop 244개를 다시 통과했고
전체 workspace의 엄격 Clippy와 opt-in live fixture의 실행 없는 컴파일도 확인했다.
실시간 처리에 할당·잠금·버퍼 확장을 추가하지 않았다.

## 2026-10-27 host I/O 요청과 SRC 게시량 분리

제품 `b30fb23`·검사 `63b1dbe`의 전체 실행 `40464144389`는 12개 통과,
macOS 15 Intel 1개 실패다. Ubuntu ARM을 포함한 Linux 7개는 모두 통과했다.
소스 CI `40464125462`는 13개 조합을 전부 통과했다. 필수 시나리오를 생략한 결과는 없다.

15 Intel 새 설치에서는 HAL extent admission 거부 1건이 있었다. 실제 게시된 유음
139,356프레임과 이를 받은 녹음의 추가 손실은 0이다. 기존 trace v4는 null 버퍼와
큰 요청을 구분하지 않으므로 해당 요청의 실제 인자는 단정할 수 없다.
코드에서는 설정 가능한 버퍼 상한 2,048프레임을 host I/O 상한으로도 쓰고 있었다.
후속 코드는 보존량 안의 큰 요청을 기존 scratch 크기로 나누며, 실제 native HAL 회귀에서
8,192프레임의 무음·유음·정수 변환·복수 클라이언트 믹스를 확인한다.
HAL trace v5는 거부 크기·포인터 유무·방향과 null/보존량 초과 건수를 분리한다.

같은 대상의 업그레이드에서는 전체 시스템 녹음의 중앙 구간이 무음이었다.
앱 펌프는 SRC reset에서 버린 초기 캡처 이력까지 native credit으로 공개할 수 있었다.
공유 PCM 전에 246,272프레임을 공개하는 경로를 유한 회귀로 재현했다.
후속 코드는 timed PCM commit과 credit을 같은 생산자 claim으로 공개하고 정지는 위치를 보존한다.
엔진은 `SourceRegistry::pcm_progress`의 실제 그래프 게시량으로 진행한다.
44.1→48kHz의 1,025프레임을 native 단위로 재환산하면 마지막 1프레임을 잃는 대조 회귀도 확인했다.
이 변경의 전체 소스 검사와 새 설치 파일 4개를 통한 13개 수락은 후속 검증 대상이다.

## 2026-10-27 설치본 PCM 통과와 보고서 파일명 수정

제품 `b30fb23`의 전체 실행 `40454423877`에서 Linux 7개 작업은 통과했다.
macOS 6개 대상의 새 설치·업그레이드 모드 보고서 12개도 필수 시나리오를 모두 통과했다.
HAL 입력 보존, 기본 녹음과 무음·유음 양 순서 녹음에서 실제 입력의 추가 손실은 0이다.
26 Intel 새 설치의 이전 녹음에서 누락됐던 첫 유음 1,064프레임도 이번에는 보존됐다.
소스 검사 `40452750846`은 13개 OS·CPU 조합을 모두 통과했다.

설치본 실행의 최종 결론은 실패다. 새 `AUDIO-12`의 하위 경로 증거를 합치는 코드가
대상 폴더 없이 복사했고, 최종 qualification 보고서를 만들지 못했다.
수정한 검사 실행부는 원본 상대 경로 전체를 인코딩해 게이트가 요구하는 단일 파일명으로 보존한다.
두 설치 모드의 하위 경로·해시 대조와 증거 변조 거부를 확인했다.
이전 실행의 결론은 유지하며 같은 설치 파일 4개로 새 전체 실행을 해야 수락할 수 있다.

## 2026-10-27 제품 입력 경계와 초기 credit

사용자 지시에 따라 macOS의 제품 수락은 실제 HAL에 전달된 PCM의 보존을 검사한다.
OS가 소스 콜백을 호출하지 않은 구간은 입력으로 꾸며 넣지 않으며 원래 왕복 결과에 남긴다.
HAL 쓰기·읽기와 엔진 녹음은 별도로 대조한다. 무음·유음 양 순서는 macOS의 필수
`AUDIO-12`이며 설정으로 끌 수 없다. 필수 시나리오 skip은 허용하지 않는다.

보관한 `40438316558`의 Intel 양 대상·새 설치·업그레이드 원시 자료에서 HAL의 입력 보존
계약은 모두 맞았다. 소스 비교 녹음도 실제 게시한 프레임은 모두 보존했다. 다만 더 강한
기본 녹음 검사에서 26 Intel 새 설치의 유음 첫 1,064프레임이 없는 것을 확인했다.
기존의 길이·피크 검사로는 발견하지 못한 범위다. 원래 실행 결과는 변경하지 않는다.

대기하던 그래프가 source의 첫 누적 credit을 현재 시작 기준으로 잡고 버리는 경로를
유한 회귀로 재현했다. 수정 전에는 이미 준비된 8,192프레임을 전부 놓쳤다.
후속 소스는 source를 기다리는 상태와 이미 실행 중인 source에 새로 붙는 상태를 구분한다.
실제 공유 버퍼·그래프 획득 회귀에서 무음·유음 8,192프레임을 보존하고 RT 할당 0을 확인했다.
이 소스 결함과 설치본의 1,064프레임 누락의 대응 확인, 새 설치본의 전체 수락은 남아 있다.

## 2026-10-27 최종 네 설치 파일의 11개 통과와 Intel 잔여 실패

제품 소스 `559721e`·측정 소스 `f6d1f67`의
[전체 설치본 실행 40438316558](https://github.com/mixrelay/mixrelay/actions/runs/40438316558)은
같은 네 설치 파일로 13개 중 11개 통과, macOS 15 Intel·26 Intel 실패다.
Ubuntu ARM과 macOS 14·15·26·27 ARM은 통과했다. 다른 실행의 통과를 합산하지 않으며
전체 설치본 수락은 미완료다.

15 Intel 새 설치의 `AUDIO-01`은 missing 1,224·corrupt 612프레임이다. 출력 콜백과
같은 소스의 HAL 쓰기에는 각각 같은 612프레임 구멍이 있다. 무음→유음의 종료 녹음은
새 설치 996·업그레이드 876프레임이 빠졌으며 해당 소스 출력의 누락과 일치한다.
26 Intel은 새 설치·업그레이드의 필수 시나리오와 정리가 통과했지만, 추가 소스 비교의
무음 구간 콜백 좌표가 각각 724·664프레임 빠져 실패했다. 관찰한 무음 payload 오류는 0이다.
무음의 시간축 검사를 완화하거나 이 대상을 통과로 바꾸지 않았다.

같은 설치 파일의 [집중 진단 40443993285](https://github.com/mixrelay/mixrelay/actions/runs/40443993285)은
15 Intel 새 설치의 PCM 실패를 5.336276초 System Trace 안에서 재현했다.
소스 오디오 스레드가 CoreAudio의 깨움을 기다린 22.1ms 구간과 이후 timestamp 불연속을
관찰했다. CPU 기록은 완전하지만 스레드 상태 export는 시간 초과로 부분 저장됐다.
추적 부하·인터럽트 원인을 분리하지 못했으므로 VM만의 문제나 제품 수정 완료로 판정하지 않는다.
무음·유음 양 순서가 이 진단에서 통과했어도 전체 실행의 실패를 대신하지 않는다.
절차별 시각·coverage와 한계는 [native 경계 분석](2026-10-26-native-boundary-analysis.md)에 기록한다.

측정 도구는 Darwin의 실제 trace 시작 알림을 확인한 뒤 PCM을 실행한다.
알림·프로세스 회수·기록 export를 분리했으며 합친 소스의 스크립트 시험 637개는 skip 없이 통과했다.
추가 제품 수정이나 새 설치 파일의 성공을 뜻하지 않는다.

## 2026-10-27 출력 콜백 누락과 Driver 조회 부하

`abf09f6`의 [전체 설치본 실행 40375187739](https://github.com/mixrelay/mixrelay/actions/runs/40375187739)은
13개 중 10개 통과, macOS 14 ARM·15 Intel·26 Intel의 `AUDIO-01` 실패다.
Ubuntu ARM은 새 설치·업그레이드와 정리가 통과했다. macOS 세 실패 대상도 무음·유음 양 순서
비교와 정리가 통과했다. 전체 수락은 실패 상태로 유지한다.

원본 출력 콜백·HAL 쓰기·HAL 읽기·입력 PCM을 분리한
[절차별 분석](2026-10-26-native-boundary-analysis.md#2026-10-27-출력-콜백과-driver-조회-수명)은
소스 output에 없는 구간과 실제 공급한 PCM의 손상을 구별한다. Driver 조회마다 동기 HTTP
runtime을 만들고 회수하는 경로는 client 수명 동안 재사용하도록 수정했다. 실제 Unix 연결 회귀는
같은 11번의 요청에서 수정 전 연결 11개, 수정 후 1개를 관찰했다.
이 부하 수정이 설치본 PCM 실패를 해소했는지는 새 후보에서 확인해야 한다.

탭 회수 시험은 두 번째 실패 결과를 선택한 뒤에만 성공을 허용한다. 호출 진입만 기다리던
경쟁을 고쳤으며 기존 비차단·재시도·admission 회수 assertion을 유지한다. 해당 크레이트의
65개 시험은 ignore 없이 통과했다.

## 2026-10-27 보존된 PCM의 획득 범위와 backlog 용량

`a88eada`의 [집중 실행 40329569555](https://github.com/mixrelay/mixrelay/actions/runs/40329569555)은
macOS 15 ARM 통과, 15 Intel·26 Intel 실패다. 선택한 세 대상의 정리는 모두 통과했다.
소스 CI 13개·VM 준비 7개·UI 검사는 통과했다. 이 집중 실행은 13개 전체 수락이 아니다.

26 Intel 새 설치의 뒤 순서에서 native output 누락은 1,108프레임이었으나 종료 녹음의 유음
144,000프레임 중 143,104프레임이 누락됐다. 녹음기는 오류 없이 정상 종료했으므로 이 손실을
소스 콜백 지연이나 녹음 큐 overrun만으로 설명할 수 없다. 15 Intel 업그레이드의 녹음 누락
4,032·1,600프레임은 해당 순서의 소스 output 누락과 일치한다.

지연된 그래프가 누적 native credit을 처리할 때 획득 시작점을 한 블록 분량으로만 계산했다.
버퍼에 남은 과거 PCM을 지나쳐 아직 게시하지 않은 미래 범위를 읽고 있었다. 합성 회귀는
유음 32,768프레임 중 1,280프레임만 전달하며 실패했다. 후속 소스는 남은 credit 전체를
획득 어댑터로 넘기고 실제 보존 용량 안에서 읽는다. 절대 native 주기까지 기다려 준비된
프레임이 없을 때의 250µs 폴링도 줄인다.

사용자 요청에 따라 공통 backlog 정책은 최대 지원 192kHz에서도 최소 4초로 늘렸다.
큐의 저장량·그래프 따라잡기 범위를 함께 바꾸고 재생 지연 목표는 callback burst로 유지한다.
macOS 전체 workspace의 lib·bin 시험과 실패 수정 후 영향 범위 재검사를 완료했다.
전체 소스의 strict all-targets clippy·fmt·미사용 의존·크레이트 계층 검사도 통과했다.
AU 35개, CLAP 69개, VST3 111개와 실제 child crash·hang 격리와 오류 관찰을 포함한 processor-host 196개가 통과했다.
격리된 Ubuntu ARM 컨테이너에서 Linux workspace lib·bin 1,837개가 통과했다.
기존 ignore 5개는 직렬 release 성능 측정이며 필수 설치본 시나리오와 구분한다.
LV2는 실제 Lilv fixture를 포함해 54개가 ignore 없이 통과했고, 별도 manifest의 strict
all-targets clippy도 통과했다. PipeWire C callback의 ASan·UBSan과 공통 PCM C fixture도 통과했다.
컨테이너에는 호스트 소스·오디오 장치를 연결하지 않았으며 설치된 제품은 변경하지 않았다.
AU·LV2 별도 manifest 시험과 Linux C callback fixture는 OS별 필수 source gate에 연결했다.

`44f6594`의 macOS 소스 CI 6개와 Ubuntu ARM을 포함한 VM 부팅 7개는 통과했다.
같은 소스의 Linux CI 7개는 장치 ID 로그의 없는 `Display` 구현을 요구해 컴파일에서 실패했다.
기존 `as_str()`를 사용하는 후속 소스로 위 로컬 Linux 검사를 완료했다. 최종 HEAD의
13개 소스 CI와 같은 설치본의 전체 수락은 별도로 확인해야 한다.

저장량을 늘린 뒤 기존 macOS bridge 회귀가 지연 목표 상한 8,192프레임을 넘는 결과로 실패했다.
`maximum_target_frames`를 저장 용량에서 분리한 뒤 같은 기대값으로 통과했다.
긴 checkout에서 소켓을 만들지 못한 Desktop·Driver 시험은 짧은 checkout 내부 IPC fixture를
사용하며 기존 연결·권한·회수 assertion을 유지한다. 새 설치본의 전체 수락은 아직 미완료다.
같은 네 설치 파일로 13개 대상, 필수 시나리오 skip 0과 정리 성공을 확인해야 한다.

## 2026-10-26 전체 설치본의 잔여 실패와 엔진 실행 주기

`f27da5c`의 [전체 실행 40315961318](https://github.com/mixrelay/mixrelay/actions/runs/40315961318)은
같은 네 설치 파일로 13개 중 10개 통과, macOS 15 ARM·15 Intel·26 Intel 실패다.
Ubuntu ARM을 포함한 Linux 7개와 macOS 14·26·27 ARM이 통과했다. 정리는 13개 모두 통과했고
필수 시나리오 skip은 0이었다. macOS 26·27 ARM의 새 설치·업그레이드에서 양 순서의
종료 녹음은 각 유음 144,000프레임의 누락·중복·변조가 0이었다.

15 ARM 업그레이드는 유음 녹음이 모두 일치했으나 외부 무음 구간의 콜백 좌표가
1,824프레임 빠져 실패했다. 문제 콜백의 실행은 13.5µs, 호출 간격은 약 40.8ms였으며
CoreAudio overload가 함께 기록됐다. 15 Intel에는 소스 콜백 누락, 26 Intel에는 녹음기
packet queue overrun도 있었다. 각 호스트 지연의 정확한 원인을 확정한 것은 아니다.

엔진은 128프레임 주기를 선언했으나 연속 playhead의 작은 변화마다 그래프를 실행했다.
15 ARM 로그에는 10초에 공유 source 실행 24,105회가 있다. Loopback의 절대 tick 절차와
대조해 완료된 native 주기에서만 작업을 풀도록 분리했다. 실제 공유 큐 회귀는 같은
2,048프레임의 처리 횟수가 수정 전 1,032회, 수정 후 16회이며 PCM 비트·순서 보존과 할당 0을
확인한다. 늦게 깨어났을 때 보존된 PCM은 계속 읽는다. 버퍼·오차·실시간 정책 예산은 유지한다.

실패로 종료된 녹음도 마무리된 파일을 분석하며 원래 실패 상태·오류를 독립적으로 보존한다.
유음이 완전하더라도 녹음기 실패는 수락하지 않는다. 관련 Rust·네이티브 1,028개와 스크립트
626개, all-targets clippy·fmt·크레이트 계층 검사가 통과했다. 후속 설치본의 전체 수락은 남아 있다.
구간별 근거와 분석 한계는 [native 경계 분석](2026-10-26-native-boundary-analysis.md)을 따른다.

## 2026-10-26 부분 블록의 획득 좌표와 측정 종료

`8d94344` 후보의 [집중 실행 40307670264](https://github.com/mixrelay/mixrelay/actions/runs/40307670264)는
macOS 26 ARM·27 ARM이 모두 실패했다. 새 설치·업그레이드의 외부 무음·유음 양 순서 8개는
모두 통과했고, 보존된 engine 로그에 `hal_shared_pcm` / `source_pump` 종료는 없었다.
종료 녹음의 유음 누락은 26 ARM 새 설치 0·26프레임, 업그레이드 0·0프레임,
27 ARM 새 설치 69·284프레임, 업그레이드 0·6프레임이었다. 정리는 네 경로 모두 통과했다.
이 결과는 13개 전체 수락이 아니다.

실제 공유 큐·변환기·source registry·획득 좌표를 연결한 합성 회귀에서 부분 게시와 지연된
worker의 burst가 유음 32,768프레임 중 59프레임을 누락시켰다. 안전 frontier의 후퇴로
공통 획득 좌표를 되돌리는 분기를 제거하면 이 회귀는 통과했으나 늦은 앱 입력의 기존 시험은
실패했다. 후속 소스는 최소 frontier를 제공한 입력의 신원·epoch 변경과 같은 입력의 reserve 변동을
구별한다. 측정 probe는 요청한 구간이 끝난 뒤 control stop이 늦어도 추가 PCM을 저장하지 않는다.
구간 안의 overflow·손상 실패 조건은 유지한다. 관련 Rust·네이티브 1,027개와 스크립트 625개가
통과했고, 새 회귀의 실시간 경로 할당은 0이었다. all-targets clippy·fmt·크레이트 계층 검사도
통과했다. 새 후보의 설치본 수락은 아직 남아 있다.

## 2026-10-26 공유 source의 관찰·PCM·clock 발행 경계

`a47b164`의 공증 후보는 source CI 13개와 VM 준비 7개·UI 검사를 통과했으나
[설치본 집중 실행 40292248302](https://github.com/mixrelay/mixrelay/actions/runs/40292248302)의
macOS 26 ARM·27 ARM은 모두 실패했다. 외부 무음·유음 대조 8개 중 7개는 통과했고
무음 payload 오류는 0이었지만, 네 종료 녹음 모두 식별 가능한 유음 누락이 있었다.
26 ARM 새 설치의 뒤 순서는 소스 callback·게시 누락도 따로 기록했다.
구간·판정·분석 한계는 [후속 native 경계 분석](2026-10-26-native-boundary-analysis.md)을 따른다.

현재 후속 소스는 정상적인 소비자 snapshot 경합과 실제 grant 만료를 구별한다.
생산자 epoch 승인 뒤의 재관찰 의존성을 없애고, PCM을 확정한 쓰기와 그 clock credit을
같은 producer claim으로 묶었다. 새 epoch의 native metadata는 일관되게 초기화하며
재무장 실패 때 기존 진단값을 보존한다. 포맷 승인 중에는 master 신원도 보존한다.

관련 Rust·네이티브 검사 966개와 스크립트 625개가 통과했다. 실제 공유 mapping에서
소비자 갱신 중 converter 준비, 부분 입력의 실제 PCM 공급량과 credit, 실패한 쓰기,
중지·새 epoch와 native rate를 확인했다. 이전의 잘못된 post-reset 관찰을 재주입하면
회귀가 `Stale` 오류로 실패하는 것도 확인했다. 새 설치본과 13개 전체 수락은 아직 미완료다.

## 2026-10-26 가상 입력의 PCM 전송·수명 분리

최종 전체 설치본 판정은 아직 미통과다. [실행 40251947994](https://github.com/mixrelay/mixrelay/actions/runs/40251947994)는
13개 중 11개 통과·macOS 26 ARM/27 ARM 2개 실패였고, 제거는 13개 모두 통과했다.
후속 `7bfc05df` 집중 실행도 macOS 26 ARM이 실패했다. macOS 27 ARM의 job 성공은
종료 녹음에서 발견된 유음 누락을 검사하지 못한 결과이므로 수락 근거로 쓰지 않는다.
[원본 PCM·종료 녹음 분석](2026-10-26-native-boundary-analysis.md)에 구간·해시·제한을 기록했다.

현재 소스는 `VirtualEndpointTransport` 입력의 CoreAudio IOProc을 없애고 HAL client별
공유 VM을 엔진의 독립 source로 전달한다. HAL 외부 독자와 엔진은 각각 native frame 좌표로
읽는다. 전체 clock publication·membership·retirement가 맞지 않으면 창을 진행하지 않는다.
물리 native 입력·출력 경로는 별도로 유지한다. 구조와 소유자는 [architecture.md](../architecture.md),
참고한 함수와 적용 한계는 [Loopback 대조](../research/loopback-engine-comparison.md)에 둔다.

관련 단위 검사 957개가 통과했다. 신호·무음 양 순서, ring wrap, 독립 부분 독자,
endpoint clock 전환, client 추가 중 보류, 16비트 ring epoch 소진 뒤 grant 교체,
실제 SRC/source-registry 전달의 PCM 비트 일치·실시간 할당 0을 포함한다.
기존 serial benchmark·자식 프로세스 fixture·설치 장치 soak의 ignore는 이 집계와 구분한다.
설치본의 필수 시나리오를 skip하도록 변경하지 않았다.

관련 5개 패키지의 all-targets clippy, fmt, crate 계층·지원 계약 검사와 `pnpm check:native`가
통과했다. 네이티브 검사는 C ABI sanitizer fixture와 ARM·Intel HAL 번들 factory/COM을 포함하며
장치를 초기화하거나 설치하지 않았다. 오프라인 release 합산은 512프레임 × 2,000회에서
2→2채널 client 1·2·8개의 총 시간이 각각 5,640·18,003·77,367µs였고,
2→7채널 client 2개는 20,742µs, 64→64채널 client 2개는 320,688µs였다.
이전 측정과 시스템 부하를 통제한 비교가 아니므로 성능 향상 수치로 쓰지 않는다.

VM 실행 도구는 자식 pipe의 close 뒤 증거 파일의 flush까지 기다리고 파일 쓰기 오류도 실패로
전달한다. 스크립트 검사 625개는 skip 없이 통과했다. 이 소스의 새 설치·업그레이드,
coreaudiod sandbox → XPC grant 전달과 13개 환경의 같은 후보 수락은 아직 확인하지 않았다.

## 2026-10-24 Ubuntu ARM 부팅 복구와 동일 후보 설치본 검증

최신 후보는 소스 `c5259184`의 공증 PKG·CPU별 DEB·동일 x64 실행 파일을 담은 Arch 패키지다.
[후보 기록](candidates/2026-10-24-c525918/README.md)에서 빌드·시험 소스와 13개 환경의 전체 설치본 실행을 연결한다.
필수 시나리오의 실패·미실행·skip과 제거 실패는 전체 수락을 막는다. 최종 결과는 해당 실행의 inventory를 따른다.

소스 `dd7166b9`의 Ubuntu ARM 포함 7개 VM 부팅 검사, Native CI 13개 대상과 UI CI는 통과했다.
현재 후보는 HAL의 실제 렌더링 클라이언트 식별, 초기 그래프 조회의 연결 복구와 설치 UI 조회 수정을 포함한다.
첫 시스템 오디오 권한창의 닫힘을 확인한 뒤 두 독립 소스를 시작하고 실제 PCM을 검사한다.
적용 전 거절된 Driver·녹음 명령은 revision과 관련 상태를 다시 확인하며, 불확실한 변경은 반복하지 않는다.
당시 계약은 process tap API 도입 패치를 최소 지원으로 정하고, 해당 패치의 정확한 hosted 이미지를 요구하지 않았다. 현재 지원 하한은 위 2026-11-01 결정을 따른다.
아래 2026-10-23의 15개 대상·0/15 결과는 이전 계약과 후보의 실패 이력이다.

## 2026-10-24 간격·크기 규격과 진단 창의 문제·지원 리포트

간격은 Carbon과 같은 2px 기반 스케일, 높이는 24·28·32px 세 단계로 정리했다. 영역·행·섹션 구분선의 레이아웃 역할을 Core 토큰으로 두고, CSS의 원시 간격 길이는 `check:boundaries`, 역할 토큰의 원시 길이는 `tokens.test.ts`가 막는다. 노드 제목줄은 28px, 접힌 기본 노드는 228×120px 그대로다.

검색 리포트 항목은 Core `DisclosureList`의 28px 접힘 행으로 바꿨다. 작업 영역 위 오류 바는 없애고 그래프 적용·파라미터·장치 목록·저장·배치 실패를 진단 창의 현재 문제와 하단 상태 점·개수·polite 알림으로 옮겼다. 진단 창의 진단 리포트 저장은 기존 `dump.json`에 문제 코드와 형식·단계별 검색 실패 개수만 더하며, 실행부가 형식과 한도를 검증한다.

Biome, 경계 검사, 타입 검사, UI 테스트 2177개(커버리지 100%), 접근성 테스트 238개, Storybook 빌드·문서 검사, 데스크톱 크레이트 fmt·clippy·테스트 225개가 통과했다. Storybook에서 진단 창(일반·390px 긴 번역), 작업 영역 문제, 상태 점, 펼침 행을 직접 확인했다. 설치본의 저장 대화상자·파일 쓰기와 스크린리더 음성은 이번에 검증하지 않았다.

## 2026-10-24 플러그인 검색 리포트와 하단 진단

플러그인 검색의 상시 빨간 경고를 제거했다. 우측 하단 진단 버튼은 엔진·Driver 상태와 현재 검색 리포트를 열며,
검색 오류 개수는 중복을 제외한 검색 진단 수다. 프로세서 메뉴의 검색 리포트와 같은 세션을 사용한다.
검색 재시도는 그래프를 적용하거나 오디오를 일시정지하지 않는다. 엔진이 확인한 처리 중단은 해당 노드의
critical 상태·설명·재시도로, 편집기 통신 오류는 해당 노드 내부 안내·편집기 재시도로 표시한다.

Core `AudioNodeCard`의 compact 상태 문구와 DOM 본문의 높이 측정을 보완했고 RoutingWorkspace가 이를 소비한다.
Core `ApplicationStatusBar`의 기존 controls 슬롯에 StudioScreen이 오디오 조작과 진단 진입점을 조합한다.
새 색·컴포넌트별 외관 오버라이드는 추가하지 않았다. 검색 리포트는 Core Dialog·ContentSection·Text·ActionButton을 사용한다.

실제 App 조합의 한국어 다크 화면에서 상단 경고 부재·하단 진단·리포트 열기·Esc·트리거 포커스 복귀를 확인했다.
Storybook의 실제 390×844 프레임에서는 라이트·긴 번역의 하단 배치·진단 제목·본문 줄바꿈·기술 정보 펼침을 확인했다.
Core의 조밀한 실패 문구와 Feature의 단일 실패 노드·편집기 오류 노드도 직접 보았다.
화면 데이터는 fixture이며 설치된 macOS 앱 교체·실제 플러그인 장애·native 오디오·스크린리더 읽기는 이번에 검증하지 않았다.

문서는 docs 프로필로 교정했다. 변경 산문 문장부호 정규화는 변경 없이 끝났고, 새 영어 주석 검출 결과에 findings는 없었다.
문서 보존 검사는 product·design-system·Patterns에서 통과했다. Pages와 조작·접근성 문서는 표 변경으로 실패했으며,
진단 진입점·포커스 계약을 반영하라는 요청에 따른 표 수정임을 diff로 별도 확인했다. 이 결과를 기계적 통과로 표시하지 않는다.

## 2026-10-23 동일 배포 후보의 GitHub Actions 설치본 E2E

환경·CPU·의존성 하한·이미지·배포 파일·필수 시나리오는 `support/environments.json`이 정의한다.
소스 CI, GitHub-hosted 설치본 시험, 후보별 인벤토리와 업데이트 게시 gate가 이 계약을 소비한다.
[운영 절차](../release-qualification.md)와 [후보별 환경·시나리오 결과](candidates/2026-10-23-8582d15/README.md)를 따른다.
필수 대상 15개 중 macOS 6개와 Linux 전체 OS VM 7개를 GitHub Actions에서 실행한다.
당시 계약의 최저 패치 양쪽 CPU는 hosted runner로 정확한 버전을 지정할 수 없어 필수 미검증 대상으로 남았다. 현재 계약에는 이 대상을 포함하지 않는다.
전용 기기·self-hosted runner·관리자 runner 토큰은 사용하지 않는다. GitHub 조회 시 저장소는 이미 PUBLIC이었다.

후보는 `0.1.0`, 제품 소스는 `8582d158455c56006e4288146dcc646100c34bb5`다.
[Linux 빌드 39939576577](https://github.com/mixrelay/mixrelay/actions/runs/39939576577)에서 각 CPU의 DEB를
한 번씩 만들었고, Arch 패키지의 세 실행 파일이 x64 DEB와 바이트 단위로 일치하는지 확인했다.
깨끗한 같은 소스에서 `pnpm signing:check` 뒤 Universal PKG를 빌드했다.
공증 제출 `acc11bf5-2746-4f88-a23d-5631c07a8dee`는 Accepted이며 staple·validate가 통과했다.
네 최종 파일의 SHA-256·크기와 계약 해시는 [후보 JSON](candidates/2026-10-23-8582d15/mix-relay.candidate.json)에 보존한다.
PKG는 `qa-install-8582d15` draft release에 있으며 제품을 공개 배포하지 않았다.

[설치본 실행 39945522752](https://github.com/mixrelay/mixrelay/actions/runs/39945522752)는 테스트 소스
`96c1bc1840c46e214f20b668222394c44ec7d8eb`로 최종 파일을 다시 빌드하지 않고 설치했다.
설치된 핵심 실행 파일의 해시, 실제 OS 패치·CPU·커널·오디오 의존성, 그래프 저장/재열기,
native stereo PCM과 종료된 RF64 녹음을 검사한다. 모든 결과와 원시는 `installed-e2e-evidence` Actions
artifact에 90일 보존한다. Linux는 공식 이미지의 URL·체크섬을 고정한 전체 systemd OS VM이며,
x64 KVM과 ARM TCG의 차이를 기록한다. TCG 결과는 하드웨어 지연 수락이 아니다.

전체 실행은 12개 환경의 설치·실행 파일 일치를 확인했다. Ubuntu ARM은 VM 준비 제한으로 미실행이었다.
[후속 실행 39949458383](https://github.com/mixrelay/mixrelay/actions/runs/39949458383)은 도구
`e5cbbd21870f9eee7edc2e23b793ad5f63d97c76`과 같은 후보를 사용했다. VM 준비 뒤 실제 DEB 설치를
시도했으나 의존 패키지 설정 중 설치 명령의 20분 제한으로 실패했고 앱·오디오 검사는 차단됐다.
13개 실행 대상 모두 설치 검사를 시도했으며 필수 환경 전체 수락은 0/15다. 후속 결과를 전체 실행의
통과로 합치지 않는다. Ubuntu x64·Arch x64·Debian ARM의 그래프 저장/재열기·native PCM·RF64는
부분 통과이며, 준비 상태 오류와 UI 차단을 포함한 환경 전체 수락은 실패다.

macOS 여섯 환경은 설치·API 준비 검사를 통과했지만 CoreAudio 초기화 시간 초과로 오디오 수락에 실패했다.
설치 검사를 통과한 Linux 여섯 환경은 엔진의 `not_installed`로 준비 검사가 실패했다. 일부 환경은 Driver API의 Ready와도 불일치했다.
독립 native 검사는 Driver 버전·Ready와 엔진 protocol·power·backend가 맞을 때만 계속 실행하며,
그 통과가 준비 검사 실패를 지우지 않는다. 환경별 부분 통과와 실패·blocked·미실행은 후보 인벤토리에서 구분한다.
설치본 UI 자동화와 macOS 무료 전체 시스템 캡처 수락은 미구현 blocked 항목이다.
이 후보는 출시 gate를 통과하지 못했다. 과거 설치본이나 VM 준비 검사 통과를 이 후보의 수락으로 승계하지 않는다.
VM 준비 이력 `39933536135`·`39935430023`도 설치본 수락과 구분한다.

로컬 스크립트 검사 419개, 지원 계약 검사와 workflow actionlint가 통과했다.
백그라운드 실행부의 상속된 stderr 때문에 SSH가 끝나지 않던 문제는 파일 FD로 분리했고,
수정 전 중단·수정 후 완료하는 실제 프로세스 회귀로 확인했다. native 프로브 하나의 종료 실패가
다른 프로브 정리를 생략하던 경계도 두 프로세스 회귀와 결함 재주입으로 확인했다.
프로브 정리 수정은 전체 실행 이후·Ubuntu ARM 후속 실행 이전에 적용했다. 후보 파일은 바꾸지 않았다.

## 2026-10-23 macOS 전체 시스템 캡처 무료 정책

macOS 전체 시스템 캡처는 무료 입력 노드 하나로 계산한다. 기능 한도 검사에서 macOS의 전체 캡처 유료 차단을 제거했고 Linux의 기존 유료 정책은 유지했다. 엔진 library의 관련 정책 테스트 13개가 통과했다. 무료 controller가 전체 캡처를 허용해야 한다는 회귀는 기존 차단 코드에서 1개 실행·1개 실패한 뒤 수정 후 통과했다. 입력·출력·매핑·플러그인·녹음 한도와 무료 전환의 기존 정책 검사도 함께 통과했다.

이는 macOS에서 실행한 소스 정책 검사다. Linux 분기 실행, 설치본·실제 캡처·E2E는 이번에 확인하지 않았다. macOS 캡처 resolver가 `AllExcept`를 거부하는 구현 공백과 현재 메뉴의 전체 캡처 진입점 부재는 남아 있다. 무료 제한 검사 통과를 오디오 경로 구현 완료로 표시하지 않는다.

## 2026-10-19 macOS 14 업데이트·제거 확인

새 VM `mixrelay-sonoma-clean`(macOS 14.8.7)에서 `/Applications/MixRelay` 폴더 배치의 공증 PKG로
업데이트·제거를 확인했다. 업데이트는 종료 0, payload 86개 SHA-256 일치, 장치 설정 유지였다. 설치 경로의 helper를
직접 실행한 제거는 바로 거부됐고 파일은 바뀌지 않았다. 제거 앱과 같은 root:wheel 0755 사본으로 실행한 제거는
소유 경로 8개와 `MixRelay IO`를 지웠다. 사용자가 둔 파일이 있으면 제품 폴더를 남겼고, 없으면 폴더까지 지웠다.
`drivers.json`은 두 경우 모두 남았다. 설치 로그의 `Distribution failed architecture check`는 Distribution에
`hostArchitectures`가 없어 Installer가 Intel로 재실행된 것이어서 `arm64,x86_64`를 선언했다. GUI 인증·XPC
완료 응답은 headless라 확인하지 않았다.

## 2026-10-19 macOS 14 설치 확인

Tart VM(macOS 14.8.7, arm64, headless)에서 공증 PKG를 설치했다. 첫 PKG는 새 macOS에
`/Library/Audio/Plug-Ins/HAL`이 없어 postinstall에서 실패했다(`open protected directory ... No such file`).
설치기가 없는 단계를 root:wheel 0755로 만들도록 고친 PKG(`777dcd5`, 공증 `8f991807-…`)는 설치에 성공했다.
설치 뒤 HAL 디렉터리 root:wheel 0755, Driver·엔진 서비스 실행, GUI 사용자 앱 실행, CoreAudio의 `MixRelay IO`
입출력 2채널 48 kHz, 설치 파일 86개 SHA-256의 PKG 일치를 확인했다. headless라 UI·권한 대화상자·
실제 캡처·녹음은 확인하지 않았다. 설치 로그의 Installer 임시 payload 앱 조회 `NSCocoaErrorDomain 260`은
설치 결과(종료 코드 0)에 영향이 없다. VM `mixrelay-sonoma`는 재사용을 위해 보존한다.

## 2026-10-19 첫 공증 릴리스 빌드

`pnpm signing:check` 뒤 `pnpm release:macos`가 App Store Connect API 키 참조로 최종 PKG를
공증했다(제출 `19119fba-e92c-448c-9b77-b59f4b6f3bbf`, `Accepted`, Apple 로그 경고 0). stapling과
`stapler validate`가 통과했고 `spctl --assess --type install`은 `accepted`,
`source=Notarized Developer ID`로 판정했다. 빌드 중 인증창은 뜨지 않았다. 이 PKG의 설치본 확인은 별도다.

## 2026-10-18 제품 코드 unwrap·expect 제거와 설치본 재확인

`fdaf814`에서 제품 Rust 코드의 `unwrap()`·`expect()`를 없애고 workspace clippy에
`unwrap_used`·`expect_used`를 deny로 켰다(테스트는 `clippy.toml`이 허용). macOS에서 workspace
clippy(`--all-targets`, `--all-features` 포함), fmt, crate 계층, workspace lib/bin 시험, `check:native`,
`test:scripts`, 제어 UI 검사, 왕복 프로브 자체 시험이 통과했다. Linux 전용 변경은 PR #2의 CI
(`c8755cb`)에서 Debian·Ubuntu·Kali(x86·ARM)·Arch(x86)의 clippy·시험과 macOS 15·UI 검사가 모두 통과했다.
이 과정에서 Linux CI 이미지가 `clippy.toml`·`rustfmt.toml`을 빠뜨려 프로젝트 한도 없이 검사하던 것과
macOS runner의 Intel target·socket 경로 길이, Linux runner 디스크 부족을 바로잡았다.

같은 소스로 서명 빌드한 `MixRelay-Installer-0.1.0.pkg`(Developer ID Installer, trusted timestamp)를
사용자가 설치했다. 첫 시도는 관리자 인증 단계 전에 "설치 취소"로 끝났고 다시 설치해 성공했다.
설치된 앱·엔진·Driver·HAL 네 실행 파일의 SHA-256이 패키지 내용과 일치했다.
원시는 `/private/tmp/mr-installed-20261018/`에 있다.

- 앱→가상 출력(`capture-virtual/`)과 앱→녹음 입력(`capture-recorder/`)의 설치본 캡처 수락이 통과했다.
  마지막 meter 창의 source·목적지 양쪽 채널이 약 0.05, sample fault·fault block은 0이며
  원래 빈 그래프로 복원했다.
- 지연 보고 수락(`latency-loopback-input/`)은 2채널 입력 장치로 Loopback Audio를 입력 장치로만
  사용했다(내장 마이크는 1채널이라 시험 경로가 거부됐고 그래프는 바뀌지 않았다). 보고값은
  경로 전 0, 경로 중 916프레임으로 세 번 연속 안정, 경로 제거 후 0이다. 왕복 실측(`--roundtrip`)은
  마이크 권한이 있는 사용자 터미널이 필요해 이번에 하지 않았다.

## 2026-10-17 입력 시각 보존과 CoreAudio 안정성

[Loopback 안정성 경계 대조](../research/loopback-engine-comparison.md)의 후속 소스다.
PCM 획득 시각을 native ring·부분 읽기·SRC·공유 application clock v5·graph까지 유지하고,
독립 입력을 공통 획득 cursor로 읽는다. timestamp snapshot이나 FIFO residence를
개별 PCM 구간의 시각으로 대신 사용하지 않는다.

현재 설치 엔진은 `981f8b8` 빌드다. Loopback → 엔진 → 가상 장치의 실제 왕복은
20/20 임펄스 정렬·지연 변동 0.9프레임·보고 오차 2.09프레임으로 통과했다.
설치 앱의 캡처→가상 출력도 정상 PCM·sample fault 0으로 통과했다. 기본 가상 장치 왕복은
144,000프레임 비트 일치·누락/중복/손상/추가 지연 0이다. 앞선 `fdd2859` 설치본에서
확인한 녹음 입력과 다중 독립 입력의 합성 시험은 이 설치본의 같은 항목 수락으로 합치지 않는다.
`cf6d868`·`fdd2859`는 설치기 재실행의 권한 책임과 출력에 도달하지 않는 입력의 정렬 참여를 고쳤다.
후속 `4252373`은 최초 그래프 시작의 PCM 경계·실제 SRC slab 피드백 목표·획득 좌표 기반
입력 지연 측정을 보강했다. `981f8b8`은 단일 입력의 시각 읽기·PCM epoch·발행 묶음을 보강했다.
최종 workspace lib/bin은 37개 실행 파일 기준 2,306개 통과·0개 실패·8개 ignore이며,
fmt·32개 crate 계층·workspace all-targets clippy `-D warnings`도 통과했다.
아래 이력의 이전 설치본 통과와 구분한다.

- 44.1/48kHz, +300/−200ppm, 64/512-frame native callback과 별도 출력 pacing을
  실제 native callback/SRC/compiled graph 경로로 구동했다. 20초 합성 PCM의
  초기 0.25초 이후 197개 이벤트가 1 graph frame 이내로 정렬됐고 graph RT 할당은 0이다.
  정렬을 꺼 결함을 주입하면 같은 회귀가 183-frame 차이로 실패한다.
- 시각이 없는 입력만 무음으로 두고 정상 입력을 계속 전달한다. 이전의 모든 입력 준비
  대기 조건을 재주입하면 정상 경로도 `[0, 0]`으로 남아 시험이 실패한다.
  두 결함 주입은 시험 후 복원했다.
- aggregate input stream·ASBD·callback configuration 준비, output-only IOProc의
  input usage 설정·readback, 서비스 재시작 시 등록·알림 재설치, UID 재확인,
  삭제 객체와 transient 오류의 구별·callback/listener drain을 보강했다.
  direct duplex graph는 입력을 유지하고, 입력을 추가하면 output owner를 다시 준비한다.
- workspace lib/bin 2,295개 통과·8개 ignore. 이후 추가한 rate-refresh 시각 회귀와
  stats의 구형 레이트/새 generation 혼합 회귀는 각각 수정 전 실패를 확인했다.
  format 준비 상태를 native running과 구별해 전환 중 PCM·clock credit만 보류하며,
  새 형식의 PCM을 이전 SRC에 전달하지 않는다. process-tap 전체 56개가 통과했다.
  최종 변경으로 데스크톱 binary 시험 209개도 다시 통과했다.
- Rust fmt, 32개 crate 계층, workspace all-targets clippy `-D warnings`, UI 정적 검사,
  왕복 프로브 자체 시험은 통과했다. `pnpm check:native`도 통과했으며, ARM·Intel
  Universal HAL의 링크·서명 검증·factory/COM probe를 확인했다. native/graph/회수 경계는
  합성 HAL과 주입된 장애로 검사한다. 전용 키체인 잠금 해제와 실제 앱·PKG 서명 시험도 통과했다.
- 위 변경을 담은 Universal 앱·서비스·HAL과 서명된
  첫 `MixRelay-Installer-0.1.0.pkg`를
  2026-10-17 11:49 KST에 빌드했다. 패키지의 Apple Developer ID Installer 서명과
  trusted timestamp를 확인했다. SHA-256은
  `481b7e177e1fa6c83d300377df47dedbc52e732ec2cb85b4f210efb31ca25338`이다.
  사용자 설치 후 11:50:13 KST 설치 transaction 완료·TargetedHost HAL 재적재와
  Driver/엔진 Ready를 확인했다. 설치된 앱·엔진·Driver·HAL 네 실행 파일의 SHA-256은
  빌드 산출물과 모두 일치한다.
- 새 설치본 기본 패스스루는 144,000프레임 전체 비트 일치, 추가 지연·누락·중복·손상·
  시각 단절 0이다. Loopback → 엔진 → MixRelay 가상 장치는 추가 30초 유지 없이
  통상적인 지연 보고 안정화 대기 뒤 20/20 임펄스 정렬, 완전한 녹음, 시각 단절 0으로 통과했다.
  평균 2,266.97프레임(47.229ms), Loopback 자체 미보고 지연 769프레임,
  MixRelay 보고 1,497프레임으로 전체 경로 보고 오차는 0.97프레임이다.
  기존 설치본보다 지연 자체가 줄었다는 결과는 아니다. 시험 후 원래 빈 그래프와 보고 0을 복원했다.
- 설치된 실행부의 native 앱 캡처는 117개 callback·59,904개 경과 프레임과 유효 시각을 확인했다.
  그러나 실제 앱 캡처 → SRC → 공유 IPC → graph 수락 시험은 입력 meter가 0으로 남아 미통과다.
  native callback 진행을 전체 PCM 전달 성공으로 취급하지 않는다. 시험 그래프와 신호 앱은 회수했으며
  후속 원인과 재검증은 아래에 구분한다. 원시는 `/tmp/mixrelay-installed-stability-20261017/`에 보존한다.
- 후속 분리 시험에서 실제 native 캡처/SRC의 44.1→48→44.1kHz 전환과 설치본의
  앱 캡처 → 녹음 노드 입력은 통과했다. 가상 출력과 함께 정렬되는 경우로 범위를 좁혔다.
  늦게 준비된 입력의 안전한 PCM 경계가 이전보다 과거로 이동할 때, 기존 공통 cursor가
  그 PCM을 계속 지나치는 회귀를 native callback·실제 공유 큐·compiled graph로 재현했다.
  경계의 역행 시 공통 좌표를 재획득하며 개별 입력의 forward-only 읽기 커서는 유지한다.
  정상 입력의 최초 전달·합류 후 양쪽 전달·RT 할당 0을 확인했고, 수정 조건을 제거하면
  정상 입력은 약 0.1인데 앱 입력은 0으로 남아 같은 회귀가 실패한다. 원래 소스로 복원했다.
  source registry의 scheduling clock을 주입해 이 회귀는 실제 경과 시간에 의존하지 않는다.
  후속 최종 workspace lib/bin 2,299개·스크립트 362개가 통과했으며 workspace와
  opt-in desktop fixture의 엄격 clippy도 통과했다. 실제 SRC 공유 큐의 획득 좌표가
  native host-time 원점에 남는 것까지 확인했다. 재현 도구는
  [macos-installed-capture-acceptance.mjs](../../scripts/lab/macos-installed-capture-acceptance.mjs)의
  `--help`로 사용법을 확인한다. 가상 출력과 녹음 입력을 나눠 측정하며 원래 그래프·자식 프로세스를
  복원한다. 응답 유실·측정 실패·SIGINT·동시 편집 때의 회수와 외부 변경 보존도 검사한다.
  이 후속 수정의 설치본 결과는 다음과 같다.
- 후속 엔진 `df523ee`를 ARM·Intel로 빌드해 서명 검증한 뒤, 이미 검증한 앱·Driver·HAL과
  후속 `MixRelay-Installer-0.1.0.pkg`로
  묶었다(12:48 KST). Universal 엔진 SHA-256은
  `3e6a29d50dbbf99d0a51d5510534cc37385ba64d7940bc20178ac140c9944221`,
  패키지 SHA-256은 `1cfbe0845e874e30f4311800dc9fe3666a698576b2ec4503b9d42f3e4cdd3d5b`다.
  패키지의 Developer ID Installer 서명과 trusted timestamp를 확인했다. 사용자 설치 완료 후
  설치된 앱·엔진·Driver·HAL 네 파일의 SHA-256이 모두 빌드 산출물과 일치함을 확인했다.
- 설치 직후 앱→가상 출력과 앱→녹음 입력은 모두 무음이었다. 같은 신호 앱의 별도 native tap은
  실제 PCM peak 0.05·RMS 약 0.03536으로 정상이며, 설치 앱과 동시에 실행한 native tap도 통과했다.
  TCC 로그에서 runtime PID 33231의 책임 앱이 `com.apple.installer`였고
  `AudioCapture` 요청의 `authValue=0, authReason=2`를 확인했다. 설치기가 실행 파일을 직접
  자식으로 띄워 Installer의 권한 책임을 물려준 경로다. callback 진행만으로 권한과 PCM을
  정상으로 판단한 것이 잘못이었다.
- 같은 설치 파일을 바꾸지 않고 정상 Launch Services 경로로 runtime을 재시작했다.
  TCC의 책임 앱이 `io.mixrelay.desktop`으로 바뀐 뒤 **앱→가상 출력과 앱→녹음 입력 모두 통과**했다.
  각 경로의 마지막 10개 meter 창에서 source/목적지 양쪽 채널이 약 0.05,
  비정상 source/input/output sample과 fault block은 0이다. 각 시험은 원래 빈 그래프를 복원하고
  자신의 신호 앱을 종료했다. 원시는 `capture-virtual-launchservices/`,
  `capture-recorder-launchservices/`, `capture-tcc-after-install.log`,
  `capture-tcc-launchservices.log`다. 설치기의 재실행도 공개 `NSWorkspace` API를 사용하도록 소스 수정했다.
- 12:48 설치본의 물리 입력 경유 지연 수락은 두 번 모두 안정된 보고값을 얻지 못해 시간 초과됐다.
  이전 설치본의 20/20 통과 결과로 대체하지 않는다. `engine-fixed/`와 `engine-fixed-repeat/`에
  실패와 그래프 복원을 보존했다. 연결되지 않은 입력과 listener가 없는 내부 capture snapshot까지
  공통 정렬에 참여하는 소스 결함을 별도로 재현했다. 실제 output/recorder에 이르는 경로만
  정렬에 참여시키며, 나머지 입력의 FIFO 소비와 미터는 유지한다. 실제 native callback·SRC·graph로
  정상 출력 약 0.25와 별도 미터 약 0.5·RT 할당 0을 확인했다. 모든 입력을 다시 참여시키면 같은
  회귀가 출력 0으로 실패한다. 결함 주입은 복원했다. 이 수정의 설치본 지연 재확인은 남아 있다.
- 후속 Universal 엔진과 설치기·제거 앱의 서명·양쪽 아키텍처 검증을 마쳤다. 빌드한 설치기로
  실제 runtime Quit → runtime 재실행 → editor 재실행을 수행한 뒤, 설치된 앱의 캡처→녹음 입력이
  약 0.05의 양쪽 채널과 fault 0으로 통과했다(`capture-installer-restore-recorder/`).
  이는 새 설치기의 공개 앱 실행 경로에 대한 실행 근거이며, 아직 교체하지 않은 새 엔진의 수락 결과가 아니다.
- 13:28 KST에 후속 설치 파일을
  저장소 `target` 안에 만들었다. 엔진 SHA-256은
  `30520a1589dda4094e3f3d479cc4a3842e7027ef588893f6ac9684824000293b`,
  설치기 SHA-256은 `5e793b8e5a455a3b2e949a6ee8fd0c703451c1c2fd4a710b600e549fa496ce32`,
  패키지 SHA-256은 `404ae587183ecf20383df3bfb134741fb00b9b51c4a794cf4bebd26602890e1f`다.
  Developer ID Installer `38UUAD7TUP`와 trusted timestamp 04:28:47 UTC를 확인했다.
  실제 앱·Driver·HAL은 이미 검증한 동일 파일을 사용한다. 사용자 설치 완료 후 설치된 네 실행
  파일이 빌드 산출물과 일치하며 Driver Ready·빈 그래프를 확인했다.
- 이 설치 직후 별도 수동 재실행 없이 앱→가상 출력과 앱→녹음 입력 모두 통과했다.
  각 마지막 10개 meter 창의 source·목적지 양쪽 채널은 약 0.05이며 sample fault는 0이다.
  `capture-final-virtual/`과 `capture-final-recorder/`에 보존했고 원래 빈 그래프를 각각 복원했다.
  기본 패스스루도 144,000프레임 비트 일치·누락/중복/손상/시각 단절 0·추가 지연 0으로 통과했다.
- `engine-final/`의 입력 지연 보고는 1,866프레임으로 안정돼 이전의 보고 timeout은 해소됐다.
  그러나 20개 임펄스 도착 지연이 약 2,496→2,268프레임으로 줄어, 같은 지연에 정렬된 것이
  2개뿐이었다. 녹음 완전성·장치 시각 연속성은 통과했지만 왕복 수락은 **미통과**다.
  시험 후 빈 그래프와 보고 0을 복원했다. 준비 중 쌓인 PCM을 최초 그래프 실행으로 가져오는
  경계와, 실제 128-frame SRC slab을 256-frame scratch 크기로 계산한 피드백 목표를 후속 수정했다.
- `4252373`의 합성 native callback·실제 SRC·compiled graph 회귀는 준비 중 64ms의 PCM이
  최초 실행에 섞이지 않고, 2초 후부터 60개 임펄스의 지연 변동이 2프레임 이내임을 확인했다.
  입력 callback이 graph보다 앞/뒤에 오는 두 순서를 모두 통과했고 graph RT 할당은 0이다.
  최초 실행 초기화를 빼면 준비 PCM 유출, SRC 목표만 이전대로 두면 약 93프레임의 지연 이동으로
  실패했다. 가변 callback 지연 보고도 기존 48프레임 오차 한도로 네 경로 모두 통과했다.
  graph 소비 시 실제 PCM 획득 좌표를 읽어 SRC·큐 시간을 포함한 age를 프레임 가중해 측정하며,
  부분 읽기·시각 부재·미래 시각도 회귀로 검사했다. 수정 엔진의 설치본 재확인은 아직 남아 있다.
- `4252373`의 ARM·Intel 엔진 빌드와 Developer ID 서명·두 아키텍처 검증을 마쳤다.
  13:57 KST에 [새 설치 파일](/Volumes/dev/mixrelay/target/macos-capture-verification/MixRelay-Installer-0.1.0.pkg)로
  이전 검증 패키지를 원자적으로 교체했다. 엔진 SHA-256은
  `c951277838cd87dca7c1ba6a819b43e11b8bb9e9084395bc781c6ed26cedfe96`,
  패키지 SHA-256은 `90408686acb7d5da1f8b47ae112c0753a1c5e6b631512315b2097feb3a6b2aa2`다.
  Developer ID Installer `38UUAD7TUP`와 trusted timestamp 04:57:39 UTC를 확인했다.
  앱·Driver·HAL·설치기/제거 앱은 앞서 검증한 동일 서명 파일이다. source 변경에 대한 Universal
  HAL의 양쪽 링크·서명 검증·실제 factory/COM probe도 별도 산출물로 통과했다(설치하지 않음).
  사용자 설치 후 Installer의 성공 화면과 설치 엔진의 동일 SHA-256·Driver Ready를 확인했다.
  `engine-startup-final/`의 왕복 시험은 20개 임펄스가 모두 도착하고 녹음 완전성·시각 연속성은
  통과했지만, 지연이 1,573.20→1,529.71프레임으로 이동해 3개만 고정 지연에 정렬됐다.
  보고는 884프레임으로 안정됐으나 왕복 수락은 **미통과**다. 시험 뒤 빈 그래프와 보고 0을 복원했다.
  이 13:57 설치본의 앱 캡처 양쪽 경로는 다시 측정하지 않았다.
- 후속 소스는 단일 routed 입력도 획득 시각으로 읽고, callback 적용 전의 좌표 공개를 막는다.
  callback 소유자가 그래프 입력 큐의 producer epoch를 갱신하고, 그래프 소비자는 epoch를
  확인한 뒤에만 좌표를 선택한다. 실제 그래프 주기 사이의 SRC 발행 묶음을 버퍼 여유분에 포함한다.
  준비 64ms 후 callback 앞/뒤 순서와 1·2·4개 묶음의 여섯 경우 모두 60개 임펄스·2프레임 이내
  지연 변동·RT 할당 0을 확인했다. 단일 입력의 FIFO 분기를 다시 넣으면 같은 시험이
  129프레임의 지연 이동으로 실패하며, 결함 주입 후 소스를 복원했다. 독립 클록 입력이
  하나일 때도 직접 렌더링을 우회해 worker의 시각 읽기를 사용하고, 같은 IOProc 입력은
  직접 렌더링을 유지한다. 최종 `981f8b8`의 workspace lib/bin 37개 실행 파일은
  2,306개 통과·0개 실패·기존 ignore 8개다. fmt·32개 crate 계층·workspace all-targets
  clippy `-D warnings`와 실제 앱·PKG 서명 사전 시험도 통과했다. 원시는
  `single-route-workspace.log`, `single-route-clippy.log`, `single-route-mutation.log`다.
  이 수정의 설치본 결과는 다음에 구분한다.
- `981f8b8`의 ARM·Intel 엔진과 Universal Developer ID 서명을 검증했다.
  14:28 KST [후속 설치 파일](/Volumes/dev/mixrelay/target/macos-capture-verification/MixRelay-Installer-0.1.0.pkg)로
  앞선 PKG를 원자적으로 교체했다. 엔진 SHA-256은
  `a4a335c13586b2c2c718708c4d16c05c400c881dda4c7685186b48d2ce9d33ca`,
  패키지 SHA-256은 `d353a29a56fe653ebba6bccb628a175d398d3371434b0383e26c10d7123a949b`다.
  Developer ID Installer `38UUAD7TUP`와 trusted timestamp 05:28:34 UTC를 확인했고,
  임시 포장 폴더를 회수했다. 사용자 설치 완료 후 설치 엔진의 SHA-256이 위 산출물과 일치하고,
  새 엔진 프로세스가 실행 중임을 확인했다.
- `single-input-installed/`의 실제 왕복은 추가 settle 없이 기본 보고 안정화 후 통과했다.
  5초 동안 20개 임펄스가 모두 같은 지연에 정렬됐고 녹음 완전성·장치 시각 연속성도 통과했다.
  지연은 1,740.7–1,741.6프레임(변동 0.9), 평균 1,741.09프레임(36.273ms)이다.
  Loopback 자체 미보고 지연 767프레임과 MixRelay 보고 972프레임을 합친 1,739프레임의
  오차는 2.09프레임으로 기존 50프레임 수락 한도 안이다. 전체 측정 시간에는 Loopback 자체의
  지연이 포함된다. 시험 뒤 원래 빈 그래프와 지연 보고 0을 복원했다.
- `single-input-capture-installed/`에서 실제 설치 앱의 tap→SRC→공유 IPC→graph→가상 출력이
  통과했다. 마지막 10개 미터 창 모두 source·목적지 양쪽 채널 약 0.05이며 source/input/output
  sample fault·fault block은 0이다. 원래 빈 그래프를 복원하고 신호 앱을 종료했으며 시험 노드의
  layout 항목도 남아 있지 않다. `single-input-passthrough.json`의 기본 가상 장치 왕복은
  144,000프레임 비트 일치, 누락/중복/손상/시각 단절/추가 지연 0으로 통과했다.
  이번 설치본에서 녹음 입력을 별도로 다시 측정하지 않았다. 설치가 필요 없는 회귀는
  유닛 테스트에서 먼저 검증하고, 설치본은 실제 권한·서비스·장치 경계의 최종 확인으로 한정한다.
- 사용자 요청으로 `/Volumes/dev/mixrelay-release-*` 설치 산출물 폴더 6개(386.1MiB)를 삭제했다.
  원본 저장소·Loopback 연구 자료와 `/tmp`의 검증 증거는 보존했다. 위 패키지 해시는 설치 이력이며
  삭제된 설치 파일의 다운로드 링크가 아니다.
- 추가 임시 정리로 이전 검토 워크트리·커버리지 캐시·옛 payload/PKG·미참조 로그 등
  376개 최상위 항목, 약 9.39GiB를 삭제했다. 오래된 release-log tail도 종료했다.
  검토 워크트리의 고유 변경은 `clippy.toml` 설정 세 줄뿐이며, Git stash
  `331fd07c5d50d7fdca01061fe55e805d40ca731b`로 보존한 뒤 워크트리를 정상 제거했다.
  현재 컴파일 캐시·최신 PKG·실행에 필요한 fixture와 문서에서 인용한 검증 자료는 유지했다.

이 소스의 v5 앱 전달·독립 입력 정렬·재시작 보호를 **설치본에서 모두 확인한 것으로 표시하지 않는다.**
아래 2026-10-16의 설치본 수락은 이전 실행 파일에 대한 기록이다.

LG HDR 4K는 CoreAudio 단독 시작도 실패하는 현 장치 상태의 예외로 둔다.
function/block IOProc, AUHAL, 별도로 동작하는 ARK clock을 master로 한 private aggregate에서도
Start가 약 10초 뒤 `stop`을 반환하고 callback은 0이었다. SDQHD·built-in 출력은 따로 통과했다.
사용자는 두 LG의 Loopback 출력을 시험하지 않았다고 답했다. Loopback meter만 움직인 관찰로
HDR 물리 출력 성공을 주장하지 않는다. 이 예외를 우회하기 위해 coreaudiod를 강제 종료하거나
전역 장치·권한·전원 정책을 변경하지 않았다. 스크린리더 시험은 사용자가 마지막에 수행한다.

## 2026-10-16 후속 소스 검사

아래 첫 집계는 Loopback 구현 대조 전 단계다. 이후 최종 소스·VM·설치본 결과는
다음 절의 오디오 구현 경로 대조와 Linux 검증 VM에 기록한다.

직접 소유하는 주요 PCM/FIFO/녹음 버퍼의 예약 실패를 오류로 돌려주고,
부분 준비의 회수와 기존 활성 PCM의 보존을 검사했다. 외부 SRC·플러그인과
프로세스 전체의 OOM 복구를 보장하지 않는다.

- macOS workspace lib/bin 2,229개 통과·8개 ignore, native 검사 499개 통과·5개 ignore.
  집계에서 자식 fixture의 중복 실행은 제외했다. workspace all-features clippy도 통과했다.
- Linux 코드 검사 workspace lib/bin 1,657개 통과·5개 ignore, workspace all-features clippy 통과.
  이후 PipeWire 버전 하한 변경은 추가 all-targets/all-features clippy와 17개 lib 검사로 확인했다.
- 스크립트 349개, Rust fmt·crate layer, 왕복 프로브 자체 시험 통과.
  FFmpeg의 모든 PCM 형식·RF64 4GiB 경계 디코딩을 포함한 녹음 검사 39개도 통과했다.
- 실환경 시험은 `native-lab`과 명시적 fixture를 쓰도록 옮겼다. 성능·soak 및
  종료용 자식 fixture의 ignore는 유지했다. 기본 코드 검사 통과를 실환경 PCM 통과로 쓰지 않는다.
- 전용 키체인의 실제 앱·PKG 서명 시험과 새 Universal 패키지 빌드가 통과했다.
  첫 업그레이드는 이 Mac의 이전 개발 설정에 필수 `active`가 없어 Driver가 반복 종료해 실패했다.
  설치기는 이전 실행 파일로 복원했고, 원래 설정을 돌려준 뒤 기존 장치가 다시 복구됐다.
  원본 백업을 보존하고 새 파일 교체 시점에 로컬 설정을 보완해 재설치했다.
  20:25:35 설치 성공 로그와 설치된 Driver·엔진의 빌드 산출물 SHA-256 일치를 확인했다.
  Driver는 Ready이고 기존 장치의 ID·2채널·48kHz 설정이 유지됐다.
  제품의 구형 저장 형식 호환 코드는 추가하지 않았다. 최신 설치본 오디오 검증은 아직 완료하지 않았다.
  Mac의 스왑 사용량이 약 48GiB여서 로컬 측정과 빌드·시험을 중지하고,
  검증 컨테이너·Docker VM·남아 있던 MixRelay Storybook 서버 2개를 종료했다.

## Loopback 비교와 현재 설치본 오디오 확인

2026-10-16 같은 `MRRoundTripProbe`로 기존 설치본의 기본 패스스루를 측정했다.
48kHz·Float32·2채널·클라이언트 버퍼 512프레임·3초 결정적 PCM 조건에서 MixRelay IO는
타임스탬프 기준 추가 지연 0프레임, Loopback 2.5.0/ARK 13.5.2는 768프레임(16ms)이었다.
둘 다 신호 144,000프레임 전체가 비트 일치했고 누락·중복·손상·채널 뒤바뀜·시각 단절은 0이었다.
이는 물리 장치 지연이나 앱 캡처·DSP·녹음을 포함한 전체 경로의 지연·CPU 우위를 증명하지 않는다.
원자료는 `/tmp/mixrelay-installed-pass-through.json`과 `/tmp/loopback-installed-pass-through.json`이다.

Loopback을 소스로 삼은 기존 설치본의 엔진 경유 시험은 미통과다. 외부 소스의 단독 기준선을
그래프 변경 전에 측정하고, 그 미보고 지연을 MixRelay의 HAL 보고와 분리해 비교하도록 수락 도구를 고쳤다.
기준선은 768프레임·20/20 임펄스로 통과했고, 엔진 경유 HAL 보고는 748프레임으로 안정화됐다.
전체 경로에서 약 1,559프레임이 관측됐지만 단일 정렬은 12개·경쟁 정렬 8개로 수락하지 않았다.
진단 필드를 추가한 다음 실행도 20개 도착 중 같은 지연으로 정렬된 것이 2개에 그쳐 수락하지 않았다.
시험 후 그래프와 지연 보고의 복원을 확인했다. 원자료는
`/tmp/mixrelay-installed-latency-20261016-baseline-verified/`와
`/tmp/mixrelay-installed-latency-20261016-alignment-diagnostic/`다.

현재 소스의 실제 aggregate 시작 시험에서 내장 스피커·Loopback 조합은 양쪽 순서 모두
두 멤버의 시작과 정상 회수가 통과했다. LG 모니터 출력 두 개는 aggregate 첫 시작에서 timeout이고,
같은 출력은 aggregate 없는 단독 시작도 timeout이다. 스택상 대기는 `AudioDeviceStart` 내부다.
따라서 이 조건을 두 번째 aggregate 멤버만의 결함으로 확정하지 않는다.
이 조합의 원인과 최신 설치본 확인은 아직 끝나지 않았다.

### 오디오 구현 경로 대조

Loopback 2.5.0/ARK 13.5.2의 보관된 Ghidra 출력
`/Volumes/dev/loopback/ghidra/export/{arkaudiod,ACP}/decompiled.c`와
MixRelay의 실제 호출 경로를 대조했다. 아래 정적 관찰은 실행 측정과 구별한다.

| 경로 | Loopback의 확인 근거 | MixRelay의 확인과 변경 |
| --- | --- | --- |
| 실행 클록 | `ACPStableIOClock::startIO:`와 타이머 handler `0x1a748` | 직접 IOProc 렌더, 실시간 대체 worker, native frame credit, 이벤트 깨우기가 이미 있다. 감독 스레드의 우선순위만 바꾸는 대체 구현을 추가하지 않았다. |
| 독립 클록 변환 | `ACPClockConverter` `0x5cc04`, sample→host→다른 sample 변환 | 기존 sinc SRC·FIFO PI·epoch를 유지했다. 이는 PCM 연속성 경로이며 독립 입력들의 절대 콘텐츠 시각 정렬과는 구별한다. 현재 FIFO에는 범위별 소스 타임스탬프가 없어 이 정렬 보장은 미완료다. |
| 캡처 정지 감시 | `updateWatchdogs` `0x100011b54`, `recreateTap` `0x1000348a0` | 프로세스·형식만 같으면 콜백 정지를 놓치던 경로에 독립 비음소거 진단 탭을 추가했다. 원래 탭은 조용한데 같은 선택의 진단 탭에서 PCM이 확인될 때만 재생성한다. 실제 무음·원래 신호 복귀·lease 변경·중지·부분 실패를 검사한다. |
| 복구 중 일시 실패 | tap/aggregate의 소유 수명과 재생성 경로 | 캡처가 중단된 상태의 일시 실패를 pending으로 유지한다. unregister 등 어느 단계에서 실패해도 전체 재생성 의도를 보존해 다음 갱신에서 재시도한다. |
| 엔진 입력 이력 | 장치·캡처 세션의 시계·수명과 상태 갱신 경로 대조 | 같은 노드·연결 ID에서 소스만 교체하면 이전 효과·detector·버스 PCM이 유지되던 결함을 고쳤다. 실제 입력 출처·상위 경로·포트 구성을 준비 단계에서 비교하며 콜백에서는 할당하지 않는다. |
| 네이티브 입력 전달 | 물리 입력 IOProc의 버퍼 처리 경로 | 입력 PCM을 큐에 넣기 전에 변환 hook을 호출해 한 native quantum을 더 기다리던 순서를 수정했다. 출력은 변환 후 소비 순서를 유지한다. |
| 시각·측정 유효성 | `ACPClockConverter`의 연속 좌표 검증 | 중간 타임스탬프 누락 뒤 역행 검출이 사라지던 문제를 수정했다. SRC reset을 가로지른 residence 구간도 무효화하고 새 측정 창은 다시 수락한다. |
| 입력별 복구 전달 | 입력 클록·세션 변경을 해당 처리 경로에 전달 | native graph read가 새 PCM epoch를 비우면서도 그래프에는 불연속을 전달하지 않던 결함을 고쳤다. 초기 priming과 복구를 구별해 같은 복구는 한 번 보고하고, 해당 입력의 하위 이력만 초기화한다. |
| PCM 준비 실패 | 링·장치 준비와 소유 수명 대조 | HAL의 정수 PCM 변환 scratch도 fallible 예약으로 옮겼다. 일부 예약이 실패하면 기존 활성 형식·매핑·클록·PCM을 보존하고 준비한 메모리만 회수한다. |
| 장치 변경 수용 | native callback과 형식 변경의 수명 대조 | callback이 관찰한 invalidation을 안정된 atomic latch로 발행한다. 제어 폴링 전에도 준비 상태와 그래프 큐 읽기·쓰기를 차단하며, 이전 등록의 drain을 확인한 준비만 latch를 해제한다. |
| 비정상 소스 PCM | 상태를 가진 변환기에 들어가는 실제 입력 경계 대조 | Mac 입력은 copy/SRC 전 NaN·Inf만 0으로 바꾼다. 앱 pump가 잘못된 샘플 하나로 영구 종료되던 경로를 수정했다. 공유 clock v4의 고정 pending 카운터는 producer 경쟁·lease·epoch와 mapping 교체를 지키며 그래프에 한 번 합산한다. |
| Linux Off와 원음 권한 | 실제 source read·큐 소비자·Forward lease의 연결 대조 | 미사용 앱·물리 입력과 재활성화 첫 읽기에서 과거 PCM을 버린다. 공유 source를 읽은 참조는 보존한다. PCM 진행이 그래프 프레임 3초 동안 멈추면 원음 차단 권한을 철회하고 신선한 PCM 뒤에만 재승인한다. |
| Linux PCM 경계 | 같은 무음·손상·수명 계약을 PipeWire 경계에 적용 | EMPTY payload의 재활용 바이트를 읽지 않고 선언된 길이의 무음을 발행한다. CORRUPTED payload는 거부하고 이전 PCM epoch를 무효화한다. |
| Linux 원음 차단 해제 | 정확한 소스·출력 선택을 보존하는 계약 대조 | 실제 시험에서 Off 뒤 원래 경로 복원이 실패했다. 기존 WP session link의 같은 peer를 재활성화하도록 수정했다. 원음 선택 차단·다른 앱 보존·Off 복원·재활성화·외부 reroute·기본 metadata 복원이 실제 시험에서 통과했다. |

최종 macOS workspace lib/bin 대상은 2,271개 통과·8개 ignore다. 중간 실패한 시험 조건을
수정한 뒤 중단된 대상을 재개했고, 같은 lib/bin 및 자식 fixture의 중복 실행은 집계에서 제외했다.
앱 캡처 46개·HAL 112개·공통 transport 216개·엔진 실행 파일 182개가 포함된다.
Mac과 Linux의 workspace all-targets/all-features 엄격 clippy도 통과했다.
최신 Universal HAL은 실제 Developer ID 서명·서명 검증·factory/COM ABI probe를 통과했으며 설치하지 않았다.
소스 교체 이력·같은 콜백 내 변환·소스 세션의 DSP reset 결함을 각각 재주입했을 때 실제
회귀 assertion이 실패했고, 정확한 소스로 복원한 뒤 다시 통과했다. 원시는 VM의
`loopback-history-mutation-runtime-proof-20261016` 및 Mac의
`/tmp/mixrelay-loopback-{callback,source-epoch}-mutation-proof-20261016.log`에 남겼다.
VM의 수정된 공통 엔진 185개·오디오 I/O 101개·PCM 링 20개 검사는 통과했다.
Forward의 엔진 출력 PCM도 480,000프레임에서 손상·단절 0이며,
`source-mute-owned-policy-explicit-witness-20261016` 작업의 원시는
VM의 `target/native-tests/app-lab-BTi7r5`에 보존했다.
Float32·Signed16/24/32 각각의 실제 pause/resume 5회도 오래된 샘플 0으로 통과했다.
EMPTY·CORRUPTED 버퍼의 중립 샘플/epoch/카운터 검사도 같은 C fixture에서 통과했다
(`loopback-native-chunk-lifecycle-20261016`, `app-lab-HjQKru`).
추가 엔진 전체 경로 시험에서는 엔진 출력과 별도 pw-record 소스가 연속이었지만,
별도 CaptureSet 관측 탭에서 1,024프레임의 단일 누락이 나와 전체 시험은 실패했다
(`loopback-native-engine-pcm-final-flags-20261016`, `app-lab-AHIUNl`).
실패 원시는 보존했다. 독립 관측 탭을 그래프 준비 중 열고 소비하지 않아 32,768프레임 큐가
넘친 시험 준비 문제를 수정했다. 입력 탭은 그래프 준비가 끝난 뒤 수집 직전에 열고,
동시에 동작하는 pw-record는 준비 단계부터 별도로 유지한다. 새 조건은 native ingress 거부·
소유자 교체·큐 discard·restart·비정상 샘플 교체도 거부하며 PCM 수락 기준을 낮추지 않았다.
최종 엔진 재검사 `loopback-engine-pcm-sourcefix-20261016`은 엔진 출력·독립 관측 탭 각각
480,000프레임, 별도 소스 캡처 480,896프레임에서 손상·단절 0으로 통과했다
(`app-lab-jOWetw`). 최종 Forward 재검사 `loopback-forward-sourcefix-20261016`도
차단·다른 앱 보존·Off 복원·재승인·외부 reroute 뒤 ACK/기본 metadata 복원과
엔진 출력 480,000프레임 연속성으로 통과했다(`app-lab-5Xisub`).
설치된 별도 Driver 프로세스의 2채널 PCM도 일반 클라이언트에서 검증했다.
`loopback-installed-driver-pcm-20261016`의 두 실행은 각각 유효 480,255프레임에서
누락·중복·잘못된 샘플·짧은 읽기 0으로 통과했다. 두 번째 실행은 그래프 클라이언트 종료 뒤다.
Driver를 임시 테스트 구현으로 바꾸지 않았으며 종료 시 처음의 비활성 user unit으로 복원했다.
Linux workspace lib/bin은 1,679개 통과·5개 ignore다. private lab의 umask에 의존하던
소켓 거부 시험은 의도한 잘못된 권한을 직접 설정하도록 고쳤고 제품 수락 조건은 유지했다.
수정된 macOS 앱·엔진·Driver·HAL의 새 Universal PKG를 만들고 실제 앱·Installer 서명을 검증했다.
파일은 `/Volumes/dev/mixrelay-release-20261016-loopback-engine-fixes/MixRelay-Installer-0.1.0.pkg`,
SHA-256은 `cffad9e9e02902ef098cf8dab32edb76982ef1deb8ea7444bbee59100720adfb`다.
사용자가 설치한 뒤 23:07:00 설치 성공·TargetedHost HAL 재적재·Ready 로그를 확인했다.
설치된 앱·엔진·Driver·HAL 네 실행 파일 모두 서명된 Universal 빌드 산출물과 SHA-256이 일치한다.
실제 편집 화면에서 빈 기존 그래프·Free 상태를 확인했고 기존 장치 ID·2채널·48kHz를 보존했다.
새 설치본 기본 패스스루는 144,000프레임 전체 비트 일치, 추가 지연·누락·중복·손상·시각 단절 0이다
(`/tmp/mixrelay-installed-pass-through-new-20261016.json`).

새 설치본 엔진 경유의 초기 측정은 통과하지 못했다. 원시 임펄스 시각을 추가한 실행에서는
20개가 모두 도착했지만 순서대로 대응시킨 후보 지연이 1,466.891→1,474.795프레임으로 변했다.
이 값은 단일 정렬의 수락값이 아니며 실패 원시는
`/tmp/mixrelay-installed-latency-{new,timeline}-20261016/`에 남겼다.
같은 경로를 활성화한 뒤 30초 유지한 진단은 기존 ±2프레임 정렬·50프레임 보고 오차 기준으로
통과했다. 20/20개 정렬, 경쟁 정렬 1개, 완전한 녹음, 양쪽 시각 단절 0이다.
첫 유지 실행은 평균 1,527.8프레임(31.829ms), Loopback 자체 미보고 지연 767프레임,
MixRelay 보고 738프레임으로 전체 경로 보고 오차 22.8프레임이었다.
명시적 `--settle-seconds=30`으로 반복한 실행도 평균 1,469.23프레임(30.609ms),
자체 기준선 767·MixRelay 보고 680·오차 22.23프레임으로 통과했다.
원시는 `/tmp/mixrelay-installed-latency-settled{,-cli}-20261016/`다. 두 번 모두 기존 그래프와
지연 보고 0으로 복원됐다. 초기 실패와 유지 후 통과를 구분하며 즉시 시작의 지연 안정성이나
독립 입력들의 절대 콘텐츠 시각 정렬까지 검증했다고 표시하지 않는다. LG 시작 대기는 미해결이다.

추가 native 앱 캡처 smoke의 첫 실패에는 시스템 오디오 권한 대기가 있었다.
`AudioDeviceCreateIOProcID` → CoreAudio stream-usage IPC에서 멈춘 PID 9978과 같은
TCC 요청 `43699.556`이 23:20:36.815에 `kTCCServiceAudioCapture` 승인을 요청했고,
23:26:29.065에 승인 결과와 응답을 받았다. 책임 앱은 `com.openai.codex`였다.
스택만 보고 등록 장애로 판단했던 설명을 정정한다. 메인 run loop를 추가한 진단도
승인 전의 같은 30초 제한에 걸렸으므로 해당 변경은 남기지 않았다.
설치된 MixRelay와 시험의 책임 앱 양쪽에 `NSAudioCaptureUsageDescription`이 이미 있다.

승인 뒤 블록형 등록을 대신 호출한 진단과 기존 함수형 등록으로 되돌린 진단이 모두 통과했다.
함수형 등록은 원음 보존·scoped 준비·원음 차단 중 PCM·해제의 네 창에서 각각
112,128/116,736/116,224/112,640프레임을 읽었고, 48kHz·2채널·peak 0.05·유효 PCM이었다.
블록형도 네 창이 통과했으므로 API 교체를 해결책으로 채택하지 않았다.
이는 현재 소스의 native 캡처 라이브러리와 설치된 가상 장치를 연결한 시험이다.
설치된 MixRelay 실행부 자체의 권한·캡처 장애 복구나 LG 시작 대기를 검증한 결과는 아니다.
시험의 신호 앱·캡처 프로세스를 모두 회수했고 기존 빈 그래프를 유지했다.
권한 원시는 `/tmp/mixrelay-tap-first-permission-request-20261016.log`, 성공 기록은
`/tmp/mixrelay-tap-registration-{block-comparison,function-reversal}-20261016.log`다.
첫 실패 스택과 실행 기록은 `/tmp/mixrelay-native-tap-smoke-sample-20261016.txt` 및
`/tmp/mixrelay-installed-native-app-capture-{fixed,bounded,runloop}-20261016.log`에 보존했다.
smoke 도구는 native 등록을 포함한 정확한 자식 프로세스를 30초로 제한한다.
후속 도구 변경은 스크립트 355개와 왕복 프로브의 합성 자체 시험으로 확인했다.
여기에는 끊긴 응답의 동일 작업 재시도·영수증 회수·외부 작업 ID 거부,
유지 중 중단 시 그래프 복원, 실제 지연 변동을 보존하면서 실패 판정을 유지하는 검사가 포함된다.

## Linux 검증 VM

추가 장치 경계 검사 `loopback-linux-device-boundary-serial-20261016`은 통과했다
(`app-lab-oU7gvJ/device-evidence/`). 2/8/16/32/64채널 반전 매핑, 64→16 게인 수정,
8→2 합산의 7개 PCM 창에서 최소 78,506개 활성 프레임, 잘못된 샘플 0, 최대 내부 무음 간격 1프레임이었다.
입력 4회 재연결·정상 반대편 스트림 유지·양쪽 소실 후 입력 단독 복귀·오프라인 게인 수정·
삭제된 노드 비복원·32회 생성/제거와 프로세스 종료도 통과했다. 마지막 28회 RSS 범위는 1,464KiB였다.
첫 실행의 비동기 영수증 미회수와 이전 스트림 이름을 단정하던 검증 도구를 고쳤으며,
제품의 PCM·복구 수락 조건은 바꾸지 않았다. 원래 실패 원시도 각 작업 폴더에 보존했다.
이 fixture를 물리 64채널 ADC·장기 내구성·오디오 서버 재시작의 증거로 쓰지 않는다.

2026-10-16 사용자가 지정한 Proxmox에 VM `107` (`mixrelay-audio-verify-20261016`)을
Ubuntu 템플릿에서 새로 만들고 Debian 13으로 부팅했다. 일반 사용자 접속은 `lab@172.16.69.41`이며 DHCP 주소다.
CPU 8개·메모리 16GiB를 배정했다. Debian 루트는 `scsi1` 96GiB이며, 준비에 사용한
Ubuntu 디스크(`scsi0` 96GiB)와 실패한 빌드 원시는 보존했다. 기존 VM과 서비스는 변경하지 않았다.
Ubuntu 24.04의 PipeWire 1.0.5는 기존 비동기 스케줄링 코드의 헤더 요구를 충족하지 못했다.
이 VM의 Debian 13 정상 사용자 PipeWire 세션에서 검증하며 Docker 코드 검사 결과와 구분한다.
최신 전체 DEB 빌드·패키지 검사·설치와 설치 직후 일반 사용자 Driver 서비스 시작이 통과했다.
같은 VM의 별도 사용자 소유 PipeWire lab에서 32/64채널의 정확한 형식·명시적 제거 검사가 통과했다.
Float32·Signed16·Signed24·Signed32 각각에서 그래프 클라이언트 종료 전후의
480,000프레임 PCM 연속성 검사도 모두 통과했다(16구간, 누락·중복·손상·짧은 읽기 0).
같은 장치의 캡처→그래프→출력에서 실제 인과 PCM이 전달되는 검사도 통과했다.
실행 로그는 VM의 `.lab/jobs/mixrelay-lab-native-driver-formats-20261016/`와
`.lab/jobs/mixrelay-lab-native-driver-pcm-20261016/`에 있다.
headless Weston의 Wayland 세션에서 설치된 `.desktop` 파일로 앱을 실행했지만,
React Aria 초기화가 `window.screen.orientation.angle` 접근에서 실패해 런처 복구 검증은 미통과다.
공식 수정 버전 React Aria Components 1.22.1/React Aria 3.53.1을 적용했다.
그 외의 UI 의존 버전은 유지했다. VM에서 UI 품질 검사, 동작 2,154개·커버리지 100%,
접근성 자동 검사 232개, 앱·Storybook 빌드와 Core 문서 계약 68개가 통과했다.
첫 앱과 Weston은 종료했다. 최신 설치본의 다음 실행에서는 데이터 폴더의 그룹 쓰기 권한 때문에
편집 앱의 로컬 제어 소켓이 거부되는 실제 시작 실패를 확인했다. Linux 편집 앱의 제어 소켓을
`$XDG_RUNTIME_DIR/mixrelay/control/control.sock`으로 옮겼다. 데이터 폴더나 소켓 보안 조건은 바꾸지 않았다.
최종 전체 DEB의 빌드·payload/ELF 검사·업그레이드 설치가 통과했다. SHA-256은
`971062670b448e65fe0c8d52f6d433be946f55ef7d3b4fb3a21173d9184cadc7`이다.
비활성 Driver user unit에서 설치된 `.desktop`을 실제 실행해 Driver Ready·엔진 health와
WebKitGTK에 렌더된 작업 공간을 확인했다. 정상 Secret Service 로그인 세션에서는 Free 상태이며
첫 headless 세션의 자격 증명 저장 불가 알림도 없다. 시험 뒤 앱·엔진 소유자 0개,
Driver unit 비활성으로 회수됐다. 원시는
`loopback-installed-desktop-login-session-20261016` 및
`target/installed-desktop-login-session-20261016/`에, 확인한 화면은
`/tmp/mixrelay-installed-linux-login-desktop-20261016.png`에 보존했다.
실제 설치된 Linux UI에서 모든 편집·키보드·스크린리더 조합을 확인한 것은 아니다.
별도 private lab의 실제 프로덕션 엔진 수명 시험 2개도 통과했다
(`loopback-linux-service-lifecycle-20261016`, `app-lab-vu7yPN`): 중복 소유자 거부,
부모 lease 종료·정상 종료·빈 그래프 재시작 뒤 native 소유자 0개를 확인했다.

엔진 전체 경로의 첫 실환경 창은 480,000프레임 중 시작의 1,024프레임이 무음이어서 실패했다.
독립 소스 캡처는 정상이며 실패 원시는 `mixrelay-lab-native-engine-pcm-20261016`에 보존했다.
Forward/WP 시험은 Driver가 준비되지 않은 lab에서 캡처 어댑터가 준비 중 제거돼 실패했다.
이 시험에 실제 Driver와 무료 제어 API를 소유하는 fixture를 추가했다.
재활용 출력 버퍼의 `SPA_CHUNK_FLAG_EMPTY`를 이번 전송에 맞게 갱신한 뒤,
기존 전체 경로 검사가 통과했다. 엔진 출력·독립 증거 탭 각각 480,000프레임,
독립 소스 캡처 506,496프레임에서 손상·단절 0을 확인했다.
원시는 `mixrelay-lab-native-engine-pcm-chunk-flags-20261016`에 보존했다.
Forward 시험은 Driver의 실제 소유 PID·역할을 조회하도록 고친 뒤 소유 원음 경로 복원까지 통과했다.
최종 소스의 재검사 결과는 위 오디오 구현 경로 대조와 구분해 기록한다.

## 현재 소스

- 2026-10-14 로컬 제어 Unix 소켓(소스): 엔진(19341)과 Driver(19342)의 loopback TCP를 없애고 사용자 소유 Unix 소켓(`engine.sock`·`driver.sock`)만 연다. 같은 컴퓨터의 다른 계정과 웹 페이지(DNS rebinding 포함)는 더 이상 엔진·Driver 제어에 닿지 못한다. 소켓 디렉터리는 `0700`, 상위 경로는 이 사용자나 root만 바꿀 수 있어야 하고, 연결은 peer UID로 거른다. 시작하는 서비스는 lock으로 직렬화해 살아 있는 소켓을 바꾸지 않는다. 실행부의 `control.sock`도 같은 구현을 쓴다. 소켓을 찾지 못한 클라이언트는 요청을 보내지 않는다. 설치기는 서비스 계정의 소켓으로 준비를 확인하고, 소켓 이전 버전으로 되돌릴 때만 TCP로 확인한다. 브라우저 개발 화면은 `MIXRELAY_DEV_ENGINE_BRIDGE=1`일 때만 loopback dev 서버가 엔진에 전달한다. Codex 리뷰 4회의 지적(dev 서버 계정 경계, 상위 디렉터리 TOCTOU, 동시 시작 경합과 lock 교착, TCP 대체 연결, 스트림 종료 전파, 되돌리기 확인, 재정의로 인한 앱·서비스 분리, 환경 의존 시험, IPv6 Host)을 고쳤다. local-socket coverage 100%, workspace clippy·fmt, UI 2,196개·axe 231개·UI 품질, 엔진 lib 340개·데스크톱 177개·Driver 46개·설치기 138개 검사가 통과했다. Linux 전용 시험은 이 Mac에서 컴파일·실행하지 못했다. `pnpm test:scripts`의 기존 번역·Linux 패키지 검사 4개 실패는 이번 변경과 무관하다. 설치본 확인은 하지 않았다.
- 2026-10-13 미터 push(소스): [설계](../design/realtime-and-control-protocol.md#적용-상태)대로 미터를 폴링 대신 push로 전달한다. 엔진은 50ms 창마다 SSE로 보내고, 실행부는 엔진 스트림 하나를 모든 편집 앱에 나눠 준다. 느린 구독자의 미전달 창에는 구간 최대값을 합치고, 웹뷰에는 credit 2개 안에서만 보낸다. 엔진 재시작(incarnation 변경) 시 이전 미터를 지우고, push 거부 시 폴링하며 5초마다 다시 시도한다. Codex 리뷰 5회에서 지적된 문제를 모두 고쳤다(정적 신호 소실, 편집 앱 종료 미감지·연결 누수, 종료 지연, 웹뷰 무제한 버퍼, 엔진 graceful shutdown 차단, 미지원 시 폴링 미전환, 작업 목록 누수, 파서 한도 우회, 중복 폴링, 이전 폴링의 덮어쓰기, credit 시간 초과 뒤 재구독 불가, 늦은 구독 해제 경합). UI 2,195개 검사·coverage 100%·axe 231개·UI 품질, 엔진 lib 331개·control-server 58개·데스크톱 178개 검사, workspace clippy·fmt가 통과했다. 변경한 Rust 줄은 coverage에 포함된다. 설치본 측정(지연·CPU)은 하지 않았다.
- 2026-10-13 실시간 실행 적용(소스): 앱 캡처 pump는 탭 콜백의 알림으로 깨어나고 time-constraint 정책으로 변환하며(예산은 탭의 실제 콜백 주기), 형식·lease 갱신과 탭·변환기 교체·해제는 실시간 밖에서 한다. 10초마다 탭 콜백 후 깨어나기까지의 지연(p50/p99/p99.9/max)과 바빴던 횟수·timeout·실패를 실행부 로그에 남긴다. 격리된 플러그인 DSP 스레드도 실시간으로 실행하며, macOS 14.4 이상에서는 공유 메모리 futex로 엔진의 입력 발행·출력 수거를 기다린다(이전 버전과 실패 시 폴링). 장치 없는 native CLAP qualification(PCM·상태 복원·지연 보정·정리)이 통과했다. 설치본의 실제 앱 캡처·플러그인 경로 측정과 지연 정책 조정은 남아 있다. 장치 클록 없이 그래프를 진행하는 대체 워커는 감독 스레드와 분리한 뒤 적용하며, 실행 경로 계측으로 사용 여부를 먼저 확인한다.
- 2026-10-13 실시간 실행 기반: [설계](../design/realtime-and-control-protocol.md)에 따라 `mixrelay-realtime` crate에 time-constraint 정책(적용값 재확인), audio work interval, 절대 시각 대기, coalescing Mach 세마포어 알림을 추가했다. 앱 캡처 pump에 적용했다(아래 측정은 그 전의 기반 benchmark). 부하 평균 약 16인 이 개발 기기에서 128프레임(48kHz) 주기 발행 3,000개의 지연은 일반 우선순위 250µs 폴링이 p50 233µs·p99 7.9ms·p99.9 22.8ms·max 28.2ms(한 주기보다 늦은 발행 170개), 실시간 + 세마포어가 p50 3.8µs·p99 8.7µs·p99.9 11.9µs·max 15.0µs(0개)였다. 같은 프로세스 안의 측정이며 설치본 오디오 경로의 지연이나 BlackHole 비교는 아니다.
- 2026-10-13 macOS Dock 중복 수정: 편집 앱을 Cmd+Q로 닫고 다시 열면 실행 상태의 실행부가 같은 번들의 실행 인스턴스로 선택돼 LaunchServices가 Dock 앱으로 승격했고 Dock에 MixRelay가 두 개 보였다(실행부에 Quit이 전달되면 오디오 전체가 정지). 실행부는 Reopen을 받을 때 백그라운드(Accessory) 정책을 다시 적용한 뒤 편집 앱을 연다. 서명 PKG 설치본에서 Cmd+Q → 다시 열기 후 실행부는 UIElement, 편집 앱만 Dock 앱으로 남는 것을 확인했다. 같은 설치본에서 최초 실행 시 '라이선스를 확인하지 못했습니다'가 표시되지 않았고, 설치 프로그램과 같은 terminate 요청으로 편집 앱·실행부·엔진이 약 2초 안에 종료됐다. 승격된 이전 실행부가 남아 있으면 업그레이드 설치의 Quit 대기가 130초 제한에 걸려 실패할 수 있으며, 이 경우 MixRelay를 종료한 뒤 다시 설치한다.
- 2026-10-13 안정성: 전체 일시정지를 엔진 설정 폴더의 `engine-paused.json`에 저장해 재개 전까지 엔진 재시작·앱 재실행·재부팅 뒤에도 유지한다(저장된 그래프는 일시정지 상태로 복원). 저장 실패는 일시정지 버튼 옆 경고와 저장 경고로 알리고 제어 유지보수가 성공할 때까지 다시 저장한다(`enginePower.unsaved`). macOS 실행부는 오디오를 소유하는 동안 엔진 소유권을 1초마다 확인해 비정상 종료된 엔진을 다시 깨우고, 이전 엔진에 연결된 앱 캡처를 해제한 뒤 새 엔진에 다시 연결한다(이전 엔진의 상태 응답은 세대 번호로 무시). 종료 요청·실패한 종료 뒤에는 깨우지 않는다. 브라우저 개발 화면의 읽기 요청에 5초 제한을 뒀다(설치본 경로는 이미 3초·30초 제한). Codex 리뷰 5회의 지적(캡처 재연결·실패한 종료 뒤 재시작·경고 유실·이전 응답의 ACK·경고 표시)을 수정했다. UI 2,175개 검사·coverage 100%·axe 231개·UI 품질, 엔진 lib 330개·데스크톱 158개 검사, workspace clippy가 통과했다. 이번 변경 줄은 모두 coverage에 포함되며, workspace coverage 100%는 시험용 오디오 장치(`MIXRELAY_TEST_ENDPOINT_UID`)가 필요한 기존 실장치 검사 때문에 이 환경에서 확인하지 못했다. Linux 실시간 처리 실패 원인 로그는 Linux 빌드 환경이 없어 하지 않았다. 서명 PKG 업그레이드 설치본에서 편집 앱으로 일시정지 → 엔진 강제 종료(`kill -9`) 시 실행부가 약 2초 뒤 엔진을 다시 시작하고 일시정지를 유지했으며(제어 응답까지 약 11초), 편집 앱 메뉴 종료·실행부 종료 후 재실행에서도 일시정지와 '재개' 표시가 유지됐다. 재개 후 표시 파일이 지워지고 다시 연결해도 실행 상태로 돌아왔다. 이전 설치본은 엔진 없이 Quit을 끝내지 못해 업그레이드 전에 프로세스를 종료했다(새 감시 경로가 해결하는 상태). 편집 앱과 실행부는 같은 번들 ID라 AppleEvent quit은 실행부로 전달돼 전체 오디오를 정지한다.
- 2026-10-13 노드 추가 › 프로세서 메뉴가 플러그인을 채널 구성마다 나열하던 것을 플러그인당 1개로 바꿨다. 새 노드는 스테레오 메인 버스 1개씩(입·출력)인 구성이 있으면 그것으로, 없으면 플러그인 기본값으로 시작한다. 구성은 노드 Options의 '채널 구성'(Core AudioNodeCard `channelLayoutControl`)에서 바꾸며, 남는 채널의 케이블은 유지하고 사라진 채널의 케이블은 제거하는 한 번의 runtime 변경이다. 같은 구성 재선택은 그래프를 다시 적용하지 않는다. 열려 있던 플러그인 에디터는 새 구성이 적용된 뒤 다시 연다. 상태 응답에 인스턴스 식별자가 없어, 적용 전 짧은 구간에 사용자가 이전 에디터를 닫아도 새 인스턴스에서 다시 열릴 수 있다. Codex 리뷰 3회의 지적(에디터 유실·이중 모노를 스테레오로 판정·재선택 재적용·의도 잔류)을 수정했다. 176개 파일의 2,172개 검사와 coverage 100%, Storybook axe 231개, UI 품질·문서 계약 검사가 통과했고 Storybook에서 드롭다운을 확인했다. 설치본에는 반영하지 않았다.
- 2026-10-13 그래프 성능: 상태 폴링마다 프레젠테이션 모델이 새 객체로 만들어져 모든 노드가 다시 렌더되던 문제를 Core `structuralShare`/`useSharedState`로 고쳤다. 대기 중 캔버스 재렌더가 3초당 약 5회에서 0회가 됐고, 컨텍스트 메뉴 hover는 캔버스를 다시 렌더하지 않는다(hover당 중앙값 0.6ms, Storybook 측정). 1초 미만의 일시적 경고(적용 중·세션 이슈·한 번 놓친 상태 응답)는 Core `useSustainedProblem`으로 표시하지 않고, 처리 실패는 즉시 표시한다. 대기 중 세션은 '오디오 준비됨'으로, 일시정지/재개 버튼은 같은 중립 색으로 표시한다.

- 2026-10-12 채널 연결의 키보드 대체 경로를 Core에 추가했다. 노드·연결선·그래프에서 Shift+F10·ContextMenu 키로 컨텍스트 메뉴를 열고 '채널 연결…'로 Core ChannelConnectionDialog를 연다. 포인터 드래그와 같은 `channelConnections` 매핑·검증으로 쌍과 건너뜀 사유를 미리 보이고 한 번의 runtime commit으로 연결하며 출발 노드로 포커스를 돌린다. React Flow의 문서 전역 Delete/Backspace는 `onBeforeDelete`로 포커스가 그래프 안에 있을 때만 삭제한다(실제 React Flow 검사 포함). Codex 리뷰 지적 4건(삭제 키 추적 잔류·사라진 채널 자동 대체·표시/제출 불일치·경계 자기 연결 누락)을 수정했다. UI 품질·경계·타입·앱/Storybook 빌드·68개 문서 계약 검사, 172개 파일의 2,141개 검사와 coverage 100%, Storybook axe 230개가 통과했다. 새 스토리 play 4개는 빌드된 Storybook의 실제 브라우저에서 통과했다(Playground는 English, 나머지는 한국어). 스토리의 열림 상태는 React state로 두어 Storybook 훅 재렌더가 인자 spy를 초기화하지 않게 했다. 설치본 반영·실제 스크린리더 청취는 별도다.

- 2026-10-12 단일 MixRelay 제품을 소스와 실제 macOS 설치 파일에 반영했다. 공개 앱·메뉴·업데이트 카탈로그는 MixRelay 하나이며, Cargo·앱·HAL·서비스·제거 앱은 모두 0.1.0이다. 전체 PKG에는 편집 앱·HAL·두 서비스·제거 앱이 항상 포함되고 Driver 선택은 없다. 완성 PKG를 확장해 Publisher 서명·Universal 2·전체 구성·버전을 다시 확인했다. 장치·그래프·설정·녹음·인증 저장 위치와 서비스 식별자는 보존하며, 기존 분리 앱의 검증·퇴역·실패 시 복원과 부분 설치 조합은 native fixture로 확인했다. 네이티브 설치 검사 153개·데스크톱 검사 154개, 패키징 검사 235개·추가 lifecycle 검사 8개, strict Clippy·전체 Rust 포맷 검사가 통과했다. UI 검사 163개 파일·2,081개와 네 coverage 항목 100%, 앱·Storybook 빌드·문서 계약 검사가 통과했다. 390px에서 단일 업데이트 모달의 Light/English·Dark/Pseudo 렌더와 Tab 순환·Enter 열기·Escape 반환 포커스, 한국어 버전 불일치 복구 화면을 직접 확인했다. 긴 다운로드 명령의 넘침은 Core ActionButton의 최소 높이·부모 폭·줄바꿈 계약으로 수정하고 Storybook 예제를 추가했다. 산출물·확장 검사 결과는 `.work/product-integration-artifact.json`, 화면 증거는 `.work/storybook-review/unified-*.png`에 있다. 설치 파일은 Developer ID 서명본이며 공증·호스트 설치·기존 설치본 실제 이관 검증은 수행하지 않았다. Linux는 패키지 소스·검사 계약을 통합했으며 새 전체 패키지의 실제 배포판 빌드·설치 검증은 별도다.

- 2026-10-12 앱 전체 TSX 86개·조작 소스 위치 107개·소유 파일 49개를 대조하고 반복 아이콘 행동의 대상 이름·Tooltip·실제 키 실행·포커스 복귀를 보완했다. 그래프 삭제·노드 Options/Bypass/에디터·녹음/폴더·비밀번호 행동을 Core와 실제 Feature에 연결했다. 동작별 무료 한도·중복 상태·일반 설명을 줄이고 가상 장치 부재 안내를 수정했다. Pseudo 전체 키 생성과 보간 토큰 보존으로 긴 번역 검수의 영어 fallback을 제거했다. UI 품질·경계·타입·앱/Storybook 빌드·61개 문서 계약 검사, 163개 파일의 2,082개 검사와 coverage 100%가 통과했다. 현재 문서 69개·스토리 459개다. Core 아이콘 검사 19개·브라우저 KeyboardOwners·RecoveryKeyboard play, 주요 App/Feature 렌더와 390px Dark/Pseudo를 확인했다. `pnpm --filter @mixrelay/control-ui audit:ui`는 재사용 소스 목록 도구이며 실제 접근성 합격 판정과 구별한다. 설치본·실제 스크린리더·모든 play 조합의 검증은 별도다. 이 UI 검수 당시 단일 제품은 명세 기준이었으며 후속 소스·패키지 통합 결과는 위 기록을 따른다.

- 2026-10-12 기호 행동과 기본값의 중복 문구를 정리했다. Core AudioGraphCanvas의 노드 추가·StudioScreen의 새 그래프는 +만 표시하고 접근성 이름·툴팁·키 실행을 유지한다. Core NameDialog의 placeholder를 표시 이름 편집이 소비하며 별칭이 없으면 빈 값·원래 이름 placeholder를 사용한다. 효과 메뉴의 일반 설명과 업데이트의 동일 버전 중복 표기를 생략했다. OriginalPlaceholder play, 실제 App의 Enter 생성/취소 포커스, 효과 메뉴·업데이트 상태와 390px Dark/Pseudo 모달을 확인했다. UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 162개 파일의 2,056개 검사와 coverage 100%가 통과했다. 현재 Storybook은 문서 69개·스토리 455개다. 설치본 반영·실제 스크린리더 검증과 구별한다.
- 2026-10-12 전체 모달의 공통 Dialog 토큰·테마와 FormActions 규격을 통일했다. 폭 300/360/420px·기본 헤더 32px/제목 13px·본문 12px, 폼 하단 우측 정렬과 위 12px/버튼 간격 8px를 사용한다. NameDialog·ConfirmationDialog·GraphCreateDialog·LicenseDetails의 생성/변경/확인 버튼이 이를 소비한다. 이름·생성·삭제·업데이트·라이선스·제품 정보 6종의 Light/Dark와 390px Dark/Pseudo, 긴 제목·라이선스 활성화 폼·Tab 순서·Enter/Escape를 확인했다. UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 162개 파일의 2,056개 검사와 coverage 100%가 통과했다. 현재 Storybook은 문서 69개·스토리 454개다. 개발 설치본에는 아직 반영하지 않았다.
- 2026-10-12 그래프 인스턴스별 표시 이름을 Core AudioGraphCanvas·NameDialog와 Feature RoutingWorkspace에 연결했다. 이름 더블클릭·F2·단일 노드 메뉴로 편집하고, 빈 값은 원래 이름 복원, Escape는 취소·같은 노드로 포커스 반환이다. GraphLayout.nodeLabels가 별칭 저장·자동 병합을 소유하며 앱 이름·selector·runtime config·오디오 revision은 유지한다. Storybook 변경·복원·취소 play 3개와 390px Dark/Pseudo에서 긴 이름·Tab·Enter·넘침을 확인했다. UI 품질·경계·타입·앱/Storybook 빌드·문서 계약 검사, 162개 파일의 2,056개 검사와 coverage 100%가 통과했다. 프로토콜 호환 1개·엔진 메타데이터 9개·파일 저장/열기/재시작 1개 검사도 통과했다. 현재 Storybook은 문서 69개·스토리 451개다. 이 변경은 개발 설치본에 아직 반영하지 않았다.
- macOS 종료 경로의 데스크톱 검사 153개, 엔진 소유권 상실·종료 실패 검사 3개, 런처 패키징 검사 12개와 런처 수명 검사 4개가 통과했다. AppKit 초기화 스레드에서 Tokio 신호 등록이 충돌하던 문제를 수정하고 회귀 검사에 포함했다. 런처는 종료 신호를 Driver에 전달하고 종료를 회수하며, 종료 신호와 자식 종료 사이의 대기 경쟁을 막는다.
- 녹음 마무리의 자동 검사는 활성 tap이 있는 `stop_all`, 마지막 tap 회수, PCM 캡처와 Stop의 경쟁을 다루며, 마무리된 실제 파일을 읽어 수락한 PCM의 보존과 종료 후 쓰기 차단을 확인한다(`crates/audio-recorder/src/tests.rs`). 엔진 종료는 같은 `stop_all`을 수행하고, 소유권 상실·마무리 실패 뒤에도 native 오디오 중지를 시도하며 실패를 보존하는 종료 검사가 있다. 실제 Dock에서 이 종료 경로로 진입하는 연결은 아래 설치본 검사로 확인했다.
- Engine 유닛 검사는 공통 PCM 경계·효과 이력·예약 실패 회수·활성 상태 보존을 포함해 통과했다.
- macOS Driver의 순수 자원 한도 유닛 검사는 Mapping 합산·공유 영역 중복 제외·한도/overflow 거부를 포함해 통과했다.
- 관련 Rust 패키지의 all-target strict Clippy와 변경 파일 포맷 검사는 통과했다.
- 2026-10-12 `pnpm check:ui-quality`와 `pnpm --filter @mixrelay/control-ui check`가 통과했다. Core 경계·타입, 160개 파일의 동작 검사 2,042개, 앱·Storybook 빌드가 통과했고 statement 5,936/5,936·branch 4,988/4,988·function 1,737/1,737·line 5,096/5,096으로 모두 100%다. 자동 revision 병합·반복 충돌·지연 응답·초안 보존·수동 재시도·배치 저장 취소와 포커스 회귀를 포함한다. coverage 기준은 낮추지 않았다.
- Storybook 기준 문서 6개·독립 컴포넌트 문서 61개와 실제 스토리 217개를 Carbon 검수표 기준으로 리뷰했다. 빌드 검사는 계약 61개·공개 Core UI/audio-graph export 57개의 API·타입·기본값, 소비자·테스트 경로, 독립 문서와 고정·상태 스토리 링크를 대조한다. 토큰·타이포의 실제 값, 지역 글꼴, 지원 상태 조합, 페이지의 복구·편집 제한 계약을 수정했다. 브라우저에서 문서 67개와 기본 스토리 198개의 Light/Dark 렌더, 추가 상태 예시, 검색·키보드 선택, 390px Pseudo 복구 화면, 아이콘 명령의 Tab·Enter·툴팁, 연속 revision의 자동 병합과 재시도 후 편집·포커스 보존을 확인했다. 렌더 검사는 모든 play 입력 조합의 완료를 뜻하지 않는다. 문서용 UI는 제품 코드 coverage에서 제외하고 실제 렌더·빌드 계약으로 검수한다. 실제 엔진·파일·장치와 스크린리더 검증은 별도다.
- 2026-10-12 Storybook 공식 문서 세 편을 기준으로 GetStarted·Contributing을 추가했다. 첫 적용 당시 운영·기준 문서 8개·독립 컴포넌트 문서 61개·스토리 230개였다. ActionButton·TextField·StatusBadge의 전용 Playground를 실제 Core 메타데이터·Controls·ArgTypes·Source에 연결하고 AST 빌드 검사에 소유 경로 대조를 추가했다. 추가 스토리 13개의 Light/Dark 렌더와 변경 문서 5개의 390px 레이아웃, 버튼 Tab·Enter·Space, 필드 빠른 입력·blur·Controls 연동을 확인했다. 첫 적용의 전용 Playground는 위 세 컴포넌트였으며 전체 play·시각 회귀 기준 이미지 검증으로 표시하지 않는다.
- 2026-10-12 전체 컴포넌트 계약 61개에 실제 Core export의 Playground·API·기본값·Source·개별 상태·사용 조합을 연결했다. 현재 Storybook은 문서 69개·스토리 445개다. 전체 61개 계약에서 실제 CSF 전체 코드와 입력 상태 helper를 읽을 수 있다. 빌드 검사는 Playground import 소유자와 실제 JSX 소비, 명시 기본값, 독립 API 연결, App/Screen 공개 화면의 Pages 목록, 10개 화면·과업 소유 파일과 상태 링크를 대조한다. 새 컴포넌트 예제는 189개, 페이지 예제는 26개다. preview 훅과 로컬 입력 상태를 사용하고 외부 args의 실제 변경만 동기화하며 최신 변경 값만 blur에서 공유한다. 숫자 확정·Select 선택의 값 복귀와 재렌더 오류를 수정했다. 문서 61개의 390px 레이아웃, 61개 Playground·새 상태 131종의 Light/Dark 렌더, 숫자 빠른 입력·Controls 양방향, Select·토글·TextField 입력을 확인했다. 새 Core 회귀 play 4개·Feature 페이지 play 24개가 통과했고 실제 App의 catalog 실패·대기에서 작업 유지·생성 잠금을 확인했다. Pages의 긴 inline 코드 넘침은 공통 DocsLayout에서 수정했다. 한국어 제목의 목차는 smooth-scroll의 ID 인코딩 불일치를 피하도록 문서 소유 DOM에서 hash를 해석해 제목으로 이동·포커스를 옮기고 제목 hash 링크를 보존하며 실제 이동을 확인했다. 전체 입력 조합·시각 회귀 기준 이미지·실제 스크린리더·설치본 검증은 구별한다.
- 일반 그래프 revision 업데이트는 자동 병합하고 통신 실패·확인되지 않은 적용은 초안을 보존한 채 재시도 하나만 표시한다. 10초 동안 지속된 확정 revision 충돌만 초안 적용·취소를 선택하게 한다. GraphApplicationQueue·GraphLayoutController가 병합·재시도·취소를 소유하고 RoutingWorkspace가 실패·복구 표시와 포커스를 소비한다. 임의의 저장본 선택을 요구하지 않는다.
- 공통 비정상 PCM 차단·진단 카운터, 효과 이력 통합, fallible PCM 버퍼 준비와 Driver Mapping 한도는 개발 설치본에 반영하지 않았다. UI 후속 수정과 진단 파일명 `dump.json` 변경도 미반영이다.
- 메모리 부족 방어는 준비·교체 경계의 예약 실패와 제품 자원 한도를 다룬다. 작은 객체의 모든 할당이나 원격 프로세스 자원까지 보호하지 않는다.
- 방어 검증은 유닛 실패 주입과 정적 검사로 충분하다. 새 장시간 시험이나 시스템 강제 오류 통합 시험은 진행하지 않는다.

## 로컬 개발 설치본

- 2026-10-12 `pnpm signing:check` 통과(인증창 없음) 뒤 `pnpm release:macos`로 Developer ID 서명 PKG(0.1.0, 미공증)를 만들고 사용자가 관리자 인증을 승인해 설치했다. 키체인 검색 목록은 로그인 키체인만으로 복원됐다. 설치 직후 편집 앱의 다시 연결이 Driver 런처를 실행하지 않아 엔진 Mach 서비스가 한 번도 요청되지 않았다(`runs = 0`, 수명 lease 보유자 없음). 설치된 `mixrelay-driver-launcher`를 직접 실행하자 런처·Driver 실행부·엔진이 시작됐고 편집 앱의 다시 연결이 성공했다. 아래 '설치·업데이트 뒤 최초 연결 실패'의 재현 근거이며 해결되지 않았다. 연결 중 '오디오 클록이 동기화되지 않았습니다' 오류 알림도 한 번 표시됐다. 설치본 한국어 화면에서 System Events 실제 키 입력으로 Shift+F10 노드 메뉴(입력 전용 노드에는 채널 연결 없음, 메뉴 이름 '작업'), '채널 연결…' 대화상자의 미리보기 '연결: 1 · 건너뜀: 0', 대화상자 안 Backspace/Delete의 노드 미삭제, Escape 취소 후 출발 노드 포커스 복귀, 그래프 안 Delete 삭제를 확인했다. 대화상자가 첫 필드가 아닌 dialog 자체에 포커스를 두던 점은 Core SelectField `autoFocus`로 수정했고 소스 검사로 확인했다(설치본 미반영). 시험 중 추가된 노드는 삭제해 그래프를 원래의 빈 상태로 돌렸다. 실제 연결 생성·스크린리더 청취는 하지 않았다.

- 2026-10-12 사용자가 서명된 전체 제거 CLI를 관리자 권한으로 실행하고 남은 제거 도우미의 launchd 등록을 해제했다. 실제 설치 대상 10개가 모두 없고 오디오 서비스·엔진·제거 도우미의 system 등록이 없으며 CoreAudio의 HAL 부재를 확인했다. 장치 설정 `drivers.json`은 남아 있다. 결과는 `.work/product-integration-removal.json`에 기록했다. 새 전체 PKG의 설치·업그레이드 검증과 구별한다.

macOS 백그라운드 실행 중단은 완료했다. 아래 설치본 검사와 녹음·종료 경로 자동 검사를 완료 근거로 삼는다.

- 2026-10-11 macOS 27.0.1에서 Driver·엔진·HAL과 편집 앱을 Developer ID 서명 패키지로 업데이트하고 설치 파일 일치·서명을 확인했다. 최종 편집 앱은 비 UI 런처를 직접 실행하며, 이전 시험 Agent의 등록 해제도 확인했다. 편집 앱 일반 종료 뒤 런처·Driver 실행부·엔진·가상 장치 1개가 유지됐다.
- 사용자가 실제 MixRelay의 Dock **백그라운드 실행 중단**을 누른 뒤 런처·Driver 실행부·엔진이 모두 종료되고, native 장치 목록과 CoreAudio 장치 목록에서 MixRelay 장치가 사라졌다. 중지 마커와 저장 장치 설정이 보존됐으며, 반복 상태 조회와 39초 경과 후에도 장치 0개·실행부 및 엔진 종료 상태가 유지됐다. 앱을 명시적으로 다시 열었을 때 같은 장치 ID·설정으로 장치 1개와 엔진이 복원됐고, 장치 설정 해시도 중지 전과 일치했다. 재연결 시험 후에는 검증된 런처에 SIGTERM을 보내 다시 중지했다. 이 마지막 정리는 실제 Dock 클릭 검증과 구별한다.
- 실제 녹음 중 Dock 클릭으로 파일을 마무리하는 조합은 직접 실행하지 않았다. 녹음 파일·종료 경로의 자동 검사와 실제 Dock 종료 연결 검사로 이번 변경을 검증하며, 이 조합의 별도 실기 검사를 필수 미완료 항목으로 두지 않는다. 설치 패키지는 로컬 검증용 Developer ID 서명본이며 공증한 공개 배포판은 아니다.
- macOS 내장 효과의 PCM·bypass·Off·Mute·Dynamics 텔레메트리와 HAL의 유한 구간 오디오 검사는 통과했다. 전체 제품의 장시간 연속성·무누수 판정은 미완료다.
- Linux VM의 소프트웨어 source/sink와 다채널 검사는 통과했다. 사용자가 제공한 실제 사용 결과에서도 Proxmox 등 x64 Linux VM은 정상 동작한다. 기존 가상 환경의 PCM 누락·되돌림 제한은 Docker 컨테이너·Docker 기반 VM의 오디오 실행에 한정하며, 일반 Linux VM이나 ARM 전체의 미해결 문제로 두지 않는다. 해당 환경에서는 제품 오디오를 실행하지 않도록 안내한다.
- 실제 설치본의 효과 설정·프리셋·EQ 키보드 조작은 확인했다. EQ 휠·드래그와 모든 입력 수단, 실제 스크린리더·전체 번역·플랫폼 접근성 검증을 대신하지 않는다.
- 2026-10-13 설치·업데이트 뒤 최초 연결 실패를 해결하고 설치본에서 확인했다. 원인은 편집 앱이 이전 빌드의 SMAppService agent 등록을 정리하지 못하면(`SMAppServiceErrorDomain:1` Operation not permitted) 런처를 실행하지 않고 연결을 끝내던 것이었다. 데스크톱 앱에 로그가 없어 오류가 버려졌다. 이제 해당 job이 launchd에 로드돼 있지 않으면 경고만 남기고 런처를 실행하며, 로드돼 있으면 기존처럼 중단한다. 데스크톱 로그는 `~/Library/Logs/MixRelay/<역할>-<pid>.log`(0600, 실행 중 5 MiB 상한)에 남는다. 서명 PKG 업그레이드 뒤 앱을 일반 실행만 했을 때 런처·Driver 실행부·엔진이 시작되고 작업 영역이 열렸으며, 로그에서 위 원인을 확인했다.

현재 소스의 설치 검증, 제품 전체의 실제 사용 흐름과 출시 요건은 제품 문서의 미완료 상태를 따른다.
소스 테스트나 유한 구간 통과를 설치본 전체의 연속성·무누수·출시 완료로 표시하지 않는다.
