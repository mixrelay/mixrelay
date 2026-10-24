# OS별 지원·검증 인벤토리

조사일은 2026-10-23(Asia/Seoul), 조사한 소스 HEAD는 `e345e6a652dedb8790f72291597811d38fa91b0b`다. 제품은 출시 전 `0.1.0`이다. 이 문서는 [제품 명세](../product.md)의 지원 범위를 [검증 기록](current.md), 현재 테스트 코드·CI 설정과 대조한 인벤토리다. 제품 범위·미완료 작업의 기준은 제품 명세에 유지한다.

이번 조사에서 제품 테스트·설치·오디오 측정을 새로 실행하지 않았다. 아래 통과는 날짜와 빌드가 붙은 기존 기록의 결과다. 현재 HEAD나 출시 후보 전체의 통과로 승계하지 않는다. 과거 기록에 실행 아키텍처·배포판 버전·패키지 해시가 없으면 미기록으로 남긴다. 원시 로그의 현존 여부와 원격 CI의 최신 상태는 이번에 재검증하지 않았다.

현재 후보 `c5259184`의 GitHub Actions 설치본 실행은 [후보 기록](candidates/2026-10-24-c525918/README.md)을 따른다. 13개 환경의 전체 판정은 진행 중이며, 아래 표는 과거 빌드의 조사 기록으로 유지한다. 새 후보의 설치·오디오 통과 여부는 후보별 결과에서 확인한다.

## 판정 방법

지원 범위, 검사 구현 여부, 실행 결과, 검증한 빌드를 따로 읽는다. `지원 대상`은 제품 정책이며 시험 통과를 뜻하지 않는다. 표의 결과는 다음과 같다.

| 결과 | 의미 |
| --- | --- |
| 확인 | 적힌 환경·동작·빌드에서 통과 기록이 있음. 생략한 UI·OS·장비 조합까지 포함하지 않음 |
| 부분 | 시나리오 일부만 확인했거나, 설치본 대신 소스·fixture에서 확인함 |
| 미확인 | 해당 환경·시나리오의 통과 근거를 찾지 못함. 검사 코드의 존재와 별개 |
| 실패 | 해당 조건에서 미해결 실패 기록이 있음. 다른 조건의 성공으로 덮지 않음 |
| 구현 공백 | 명세상 제공 범위지만 현재 제품 경로가 해당 요청을 거부하거나 구현이 빠짐. 실행하지 않은 시험과 구분 |
| 비대상 | 제품 범위 밖이거나 해당 OS에 적용되지 않음. 미실행을 통과로 집계하지 않음 |

검사 층은 `소스`(유닛·mock·합성 PCM), `실환경 프로브`(실제 CoreAudio/PipeWire·서비스·파일), `설치본 UI`(실제 앱 조작), `공통 자동 E2E`로 구분한다. Storybook·jsdom 통과는 설치본 UI·접근성 전체의 확인이 아니다. 동일 후보 파일을 설치해 공통 API·native 신호·녹음을 검사하는 runner와 CI gate를 추가했다. 설치본 UI 자동화와 macOS 전체 캡처 수락은 미구현이며, 이 필수 항목들은 blocked로 남는다. 새 runner의 실제 전 환경 통과 결과는 아직 없다.

## 지원 OS·아키텍처와 검증 환경

현재 지원 정의와 필수 환경 ID는 [`support/environments.json`](../../support/environments.json)과 [생성된 환경표](supported-environments.md)를 따른다. 아래 표의 대문자 ID는 2026-10-23에 정리한 과거 기록의 묶음이며 현재 CI 행렬이나 출시 대상 정의로 소비하지 않는다. 새 후보의 환경별 결과는 `pnpm validation:inventory`로 파일 해시와 함께 생성한다.

macOS는 14.2 이상 Apple Silicon·Intel, Linux는 x86_64·arm64의 정상 사용자 PipeWire 세션이 제품 범위다. Linux는 PipeWire 1.2 이상·WirePlumber 0.5 이상을 요구한다. 현재 Linux 패키지의 glibc 요구는 [패키징 문서](../../packaging/linux/README.md)의 2.39이며, 오디오 서버 하한만으로 모든 배포판 설치를 보장하지 않는다.

| 환경 ID | OS·CPU | 제품 범위 | 빌드·소스 검사 근거 | 설치·실행·오디오 근거 |
| --- | --- | --- | --- | --- |
| MAC-14-ARM | macOS 14.8.7 · arm64 · Tart/headless | 지원 대상 | Universal PKG | 설치·업데이트·제거 일부 확인 [M19](#m19). UI·권한 대화상자·캡처·녹음 미확인 |
| MAC-14-OTHER | macOS 14.2 이상 14.x 중 14.8.7 이외 · arm64/Intel | 지원 대상 | 최소 버전 설정·Universal 산출물 있음 | 해당 버전 설치본 미확인. 14.8.7 결과를 복제하지 않음 |
| MAC-14-INTEL | macOS 14.8.7 · Intel | 지원 대상 | Universal x86_64 slice 빌드·서명 기록 | Intel OS에서 실제 실행 미확인 |
| MAC-15 | macOS 15 · CI runner | 지원 대상 | 소스 검사 통과 기록 [CI18](#ci18). runner 실행 아키텍처는 인용 기록에 없음 | 설치본·권한·오디오 미확인 |
| MAC-26 | macOS 26 · arm64/Intel | 지원 대상 | 개별 OS 실행 기록 없음 | 설치본 미확인 |
| MAC-27-DEV | macOS 27 개발 Mac · 10월 11일 기록은 27.0.1 | 지원 대상 | 로컬 소스·Universal 빌드 기록 [M17](#m17)·[M18](#m18) | 설치·캡처·일부 왕복·수명 확인. 후속 기록의 정확한 OS 패치·실행 CPU는 미기록 |
| MAC-27-INTEL | macOS 27 · Intel | 비대상: Apple 공식 호환 모델 없음 | Universal의 x86_64 slice는 Intel용 macOS 14.2–26에서 사용 | [Apple 공식 목록](https://support.apple.com/en-us/127255)에 따라 필수 조합에서 제외 |
| LINUX-DEB-X64 | Debian 13 · x86_64 · Proxmox VM 107 | 지원 대상 | CI 기록 [CI18](#ci18), VM 소스 검사 [L16](#l16) | DEB 설치·업그레이드·런처·실제 PipeWire PCM 일부 확인 [L16](#l16) |
| LINUX-DEB-ARM | Debian 13 · arm64 | 지원 대상 | Docker CI 소스 검사 통과 기록 [CI18](#ci18) | 해당 CPU의 정상 사용자 세션·설치본·오디오 미확인 |
| LINUX-UBUNTU | Ubuntu 26.04 · x86_64/arm64 | 실행 의존성 충족 시 지원 대상 | Docker CI 소스 검사 통과 기록 [CI18](#ci18) | 이 배포판의 설치본·오디오 미확인 |
| LINUX-KALI | Kali rolling · x86_64/arm64 | 실행 의존성 충족 시 지원 대상 | Docker CI 소스 검사 통과 기록 [CI18](#ci18). 이미지 digest는 compose에 고정 | 이 배포판의 설치본·오디오 미확인 |
| LINUX-ARCH | Arch · x86_64 | 실행 의존성 충족 시 지원 대상 | Docker CI 소스 검사 통과 기록 [CI18](#ci18) | Arch 패키지 실제 설치·오디오 미확인. Arch ARM은 현재 CI 대상 아님 |
| LINUX-RPM | RPM 계열 · x86_64/arm64 | RPM 산출물 설정 있음. 개별 배포판 검증 범위 미정 | 패키지 구조 검사 코드 있음 | 실제 RPM 배포판 설치·업그레이드·제거·오디오 미확인. Fedora/RHEL 지원을 새로 확정하지 않음 |
| LINUX-OLD-AUDIO | 예: Ubuntu 24.04 기본 PipeWire 1.0.5 | 의존성 하한 미달 | VM 준비 중 헤더 요구 불충족 기록 [L16](#l16) | 기본 패키지 상태는 비대상. Ubuntu 전체나 Linux VM 전체의 결함으로 확대하지 않음 |
| DOCKER-AUDIO | Docker 컨테이너·Docker 기반 VM의 제품 오디오 | 비대상 | 빌드·소스 검사 용도로 사용 | native audio·로그인 세션·플러그인 에디터 확인을 대신하지 않음 |
| OTHER-OS | Windows · 기타 OS | 이번 제품 지원 범위 밖 | 이 인벤토리의 실행 대상 아님 | 비대상 |

MAC-27-DEV는 기존 실환경 기록을 묶는 식별자다. CPU별 수락 결과를 얻으면 실제 아키텍처를 명시한 환경으로 나눈다. Universal 산출물의 양쪽 slice 확인을 Intel 실행 통과로 바꾸지 않는다. Debian VM의 결과를 다른 배포판·arm64·물리 장비로 확장하지 않는다. Linux의 X11·Wayland 전체 조합도 아직 확인하지 않았다.

## OS별 기능 계약

이 표는 제품 명세의 제공 범위이며 실환경 통과표는 아래 시나리오에서 따로 본다.

| 기능 | macOS | Linux |
| --- | --- | --- |
| 가상 장치 | CoreAudio HAL, 방향별 1–64ch. 44.1/48/88.2/96/176.4/192kHz, F32·PCM16·packed PCM24·PCM32 | PipeWire, 방향별 1–64ch. PipeWire가 지원하는 레이트와 F32·PCM16·packed PCM24·PCM32 조합 |
| 개별 앱·물리 장치 입력, 그래프·내장 효과·녹음 | 공통 제품 범위. 물리 장치와 플러그인의 실제 capability 적용 | 공통 제품 범위. 물리 장치와 플러그인의 실제 capability 적용 |
| 앱 선택의 현재 엔진 제약 | 정확한 bundle ID·하위 프로세스 포함 안 함·스테레오 2ch만 허용 | bundle ID·실행 경로·PID·`AllExcept` 선택 분기 있음. 각 선택의 UI 노출·실환경 확인은 별도 |
| 전체 시스템 캡처 | 무료 입력 노드 하나로 계산. 현재 엔진의 `AllExcept` 거부로 캡처 구현 공백 | 기존 유료 정책과 `AllExcept` 앱 선택 경로 있음. 전체 시스템 신호 수락은 미확인 |
| VST3·CLAP | 지원 명세 있음 | 지원 명세 있음 |
| AU·AUv3 | 지원 명세 있음 | 비대상 |
| LV2 | 비대상 | 지원 명세 있음 |
| 편집 창 닫기 | 창 숨김. 편집 앱 종료도 백그라운드 오디오 유지 | 편집 프로세스 종료. 백그라운드 오디오 유지 |
| 설치·제거 | 단일 PKG·서명/공증·전용 제거 앱 | DEB/RPM/Arch 전체 제품 패키지·패키지 관리자 경로. AppImage는 통합 설치 경로 아님 |
| 구매·무료 한도·업데이트 | 공통 정책. OS Keychain 경계는 실제 검증 별도 | 공통 정책. 정상 로그인 Secret Service 경계는 실제 검증 별도 |
| 노이즈 억제·LUFS·BPM·MIDI | 이번 범위 밖 | 이번 범위 밖 |

전체 시스템 캡처의 소스 근거는 [무료 한도 검사](../../apps/engine-service/src/feature_policy/tests.rs), [macOS 캡처 준비](../../apps/engine-service/src/macos_devices/application.rs), [Linux 앱 선택](../../apps/engine-service/src/linux_applications.rs)이다. macOS는 무료 입력 노드 하나로 계산하며, Linux는 기존 유료 정책을 유지한다. macOS `resolve`는 정확한 `BundleId`와 `include_descendants: false`만 허용하고, 같은 파일의 테스트도 `AllExcept` 거부를 기대한다. [실제 macOS 어댑터](../../apps/engine-service/src/macos_devices.rs)가 이 함수를 호출한다. 무료 제한 검사 통과와 macOS 캡처 구현 완료는 별개다. 이 공백은 제품 명세의 구현 상태에도 기록했다.

UI 번역 원문의 설명은 [영어 문구](../../apps/control-ui/src/core/i18n/locales/en.ts)에서 macOS 전체 출력으로 적혀 있다. 두 OS의 구현과 UI 제공 범위·설명의 일치도 미확인이다. 이는 소스 텍스트 관찰이며 실제 화면을 본 결과가 아니다.

2026-10-23 macOS 소스에서 전체 캡처의 유료 차단을 제거하고 관련 정책 검사 13개가 통과했다. [무료 정책 검증 기록](current.md#2026-10-23-macos-전체-시스템-캡처-무료-정책)을 따르며, Linux 분기·설치본·실제 전체 캡처 수락 결과로 확장하지 않는다. 이 정책 변경은 조사한 HEAD 이후 작업 트리에 반영돼 있다.

## 공통 시나리오별 검증 범위

아래 ID는 같은 기대 결과를 OS별로 대조하기 위한 식별자다. 자동 E2E가 구현됐다는 뜻은 아니다. macOS 열은 주로 MAC-27-DEV이고 14.8.7 확인은 명시했다. Linux 열은 LINUX-DEB-X64만 뜻한다. 나머지 환경의 설치본 시나리오는 위 환경 표대로 미확인이다.

### 설치·앱 수명

| ID | 확인할 결과 | macOS 기록 | Debian 13 x86_64 기록 |
| --- | --- | --- | --- |
| PKG-01 | 전체 제품 패키지의 실행 파일·구성·버전·서명 확인 | 확인: Universal·Developer ID·공증·staple [M19](#m19). 단일 최종 후보의 모든 동작 수락은 별도 | 부분: DEB 빌드·payload/ELF·설치 확인 [L16](#l16). RPM·Arch 실설치로 확대 안 함 |
| PKG-02 | 깨끗한 OS에 설치 후 앱·서비스·가상 장치 준비 | 부분: 14.8.7 headless 설치·앱 프로세스·2ch 48kHz 장치 확인, 화면·권한·PCM 제외 [M19](#m19) | 부분: DEB 설치·실행 확인. 깨끗한 OS부터 같은 흐름을 자동 재현한 근거 없음 [L16](#l16) |
| PKG-03 | 업그레이드 후 장치·그래프·설정·녹음·인증 보존 | 부분: 장치 설정과 payload 일치 확인. 나머지 데이터 전부의 설치본 대조는 미확인 [M18](#m18)·[M19](#m19) | 부분: DEB 업그레이드·런처 확인. 모든 사용자 데이터 보존 대조는 미확인 [L16](#l16) |
| PKG-04 | 제거 UI·인증·취소·설정 보존·제품 파일과 장치 회수 | 부분: CLI/도우미 제거·장치 회수·설정/사용자 파일 보존 확인. 14.8.7 GUI 인증·XPC 완료 응답 제외 [M12](#m12)·[M19](#m19) | 미확인: lifecycle hook·제거 검사 코드는 있으나 배포판별 실제 제거 완료 근거 없음 |
| APP-01 | 기본 실행 아이콘으로 연결해 Ready·작업 화면·Free 상태 도달 | 확인: 설치 후 최초 연결 복구와 작업 화면 [M13](#m13) | 확인: 설치된 `.desktop` 실행·Driver Ready·엔진 health·렌더·Secret Service 로그인 [L16](#l16) |
| APP-02 | 편집 앱 닫기·재실행 중 오디오 유지와 같은 상태 재연결 | 부분: 일반 종료 뒤 실행부·장치 유지, Dock 중복 해소 확인. 모든 DSP·플러그인·녹음 조합 제외 [M11](#m11)·[M13](#m13) | 부분: 그래프 클라이언트 종료 전후 PCM 연속성 확인. 실제 UI 닫기·재열기 전체 흐름 제외 [L16](#l16) |
| APP-03 | 일시정지 → 엔진 재시작 → 일시정지 유지 → 명시적 재개 | 확인: 설치본 엔진 종료 후 복구·앱/실행부 재실행과 pause 유지 [M13](#m13). 재부팅은 미확인 | 미확인: 소스 경계 검사와 프로덕션 엔진 소유권 시험은 있으나 설치본 UI 흐름 근거 없음 |
| APP-04 | 명시적 실행 중지 → 장치 회수 → 중지 유지 → 다시 열어 같은 장치 복원 | 확인: 실제 Dock 중지·장치/프로세스 회수·명시적 재연결·설정 동일 [M11](#m11) | 부분: 소유권·정상 종료·빈 그래프 재시작의 native 소유자 회수 확인. 설치 UI의 전체 중지/복원 흐름 제외 [L16](#l16) |
| APP-05 | 권한·자격 증명 서비스 거부/미준비 안내 후 정상 조건에서 복구 | 부분: 실제 TCC 승인 대기와 Launch Services 권한 책임 수정 확인. 거부→재허용 전체 설치 UI 흐름 미확인 [M16](#m16)·[M17](#m17) | 부분: Secret Service 없는 첫 세션 경고와 정상 로그인 Free 표시 확인. 같은 실행에서 거부→복구 흐름 미확인 [L16](#l16) |

### 오디오 경로

| ID | 확인할 결과 | macOS 기록 | Debian 13 x86_64 기록 |
| --- | --- | --- | --- |
| AUDIO-01 | 가상 장치 기본 루프백에서 채널·PCM 보존 | 확인: 48kHz Float32 2ch·144,000프레임 비트 일치·누락/중복/손상 0 [M17](#m17) | 확인: 설치 Driver의 2ch PCM 480,255프레임, 그래프 클라이언트 종료 뒤도 확인 [L16](#l16) |
| AUDIO-02 | 개별 앱 캡처 → 엔진 → 가상 출력의 실제 신호 | 확인: 설치 실행부 경로 양쪽 채널 약 0.05·fault 0 [M18](#m18) | 부분: fixture 앱→실제 PipeWire→엔진→`AudioDeviceOutput` 신호 연속성 확인 [L16](#l16). 같은 가상 출력·UI 경로의 확인으로 합치지 않음 |
| AUDIO-03 | 실제 마이크/물리 입력 → 처리 → 실제 물리 출력 | 부분: built-in·SDQHD 출력 시작 기록 있음. 전체 마이크→출력 수락은 미확인. Loopback 소스 측정으로 대신하지 않음 [M17](#m17) | 부분: software source/sink 장치 경계 확인. 실제 물리 ADC/DAC 경로는 미확인 [L16](#l16) |
| AUDIO-04 | 소유한 앱 원음만 차단·다른 앱 보존·Off 후 복원 | 부분: native tap의 보존/차단/해제 PCM 확인. 설치 앱의 전체 장애 복구는 미확인 [M16](#m16) | 확인: 원음 차단·다른 앱 보존·Off 복원·재활성화·외부 reroute·metadata 복원 [L16](#l16) |
| AUDIO-05 | 지원 채널·레이트·PCM 형식 변경 후 readback·신호 보존 | 부분: 기본 2ch 48kHz와 native 44.1↔48kHz 전환 기록. 1–64ch·6개 레이트·4개 PCM 전체 조합은 미확인 [M17](#m17) | 부분: 32/64ch 형식·제거 및 F32/S16/packed S24/S32 PCM 확인. 모든 채널·레이트 조합은 미확인 [L16](#l16) |
| AUDIO-06 | 다채널 매핑·게인·합산의 기대 PCM과 미터 일치 | 부분: 공통 엔진 소스 검사. 설치본의 같은 다채널 시나리오는 미확인 | 확인: fixture 2/8/16/32/64ch 반전 매핑·64→16 게인·8→2 합산 [L16](#l16). 물리 64ch 장비 증거 아님 |
| AUDIO-07 | 입력·출력 소실/복귀 시 정상 경로 보존·오프라인 편집 유지 | 부분: 소스 장애 주입·재시작 보호. 실제 장애 경계의 설치본 확인 남음 [P](#p) | 확인: fixture 입력 4회 재연결·반대편 유지·양쪽 소실·입력 복귀·오프라인 수정·삭제 비복원·32회 회수 [L16](#l16). 오디오 서버 재시작은 제외 |
| AUDIO-08 | 서로 다른 입력 클록·레이트의 콘텐츠 시각 정렬 | 부분: 44.1/48kHz·±ppm·다른 callback의 합성 20초 정렬 통과. 다중 독립 입력 설치본 수락 남음 [M17](#m17) | 미확인: 같은 콘텐츠 정렬 시나리오의 Linux 실환경 결과 없음 |
| AUDIO-09 | 보고 지연과 실측 왕복 지연의 일치·초기 안정성 | 확인: `981f8b8`, 20/20 임펄스·변동 0.9frame·보고 오차 2.09frame [M17](#m17). 후속 `fdaf814`는 보고만 확인 [M18](#m18) | 미확인: 같은 보고/실측 일치 수락 근거 없음. VM 결과로 물리 지연 보장 안 함 |
| AUDIO-10 | 앱 캡처·장치·플러그인 부하에서 연속성·자원 회수 | 부분: 유한 PCM 창·소스 수명/무할당 검사. 전체 제품 장시간 무누수 판정 없음 [P](#p) | 부분: 유한 PCM 창·32회 자원 회수·RSS 범위. 전체 제품 장기 내구성 판정 없음 [L16](#l16) |
| AUDIO-11 | 전체 시스템 캡처에서 복수 앱·효과음 수신과 제외 규칙 확인 | 구현 공백: 무료 정책과 별개로 현재 엔진 `resolve`가 `AllExcept`를 거부함. 무료 제한 검사만으로 캡처 완료 처리 안 함 | 부분: 기존 유료 정책·`AllExcept` 선택 소스 경로. 설치본 전체 시스템 신호 수락 없음 |
| DEVICE-01 | 가상 장치 추가·이름 변경·제거·재실행 후 ID/형식 보존 | 부분: 장치 설정 보존·중지/재연결 기록. 유료 장치 편집 전체 설치 UI 흐름 미확인 [M11](#m11)·[M19](#m19) | 부분: native 형식·제거와 소스 장치 설정 검사. 유료 장치 편집 전체 설치 UI 흐름 미확인 [L16](#l16) |

### 편집·효과·녹음·판매·사용성

| ID | 확인할 결과 | macOS 기록 | Debian 13 x86_64 기록 |
| --- | --- | --- | --- |
| GRAPH-01 | UI로 그래프 생성·노드/채널 연결·Undo 후 실제 신호 변화 | 부분: App/jsdom·Storybook 동작 및 일부 설치본 키 조작 확인. UI→신호 전체 자동 E2E 없음 [UI12](#ui12)·[M12](#m12) | 부분: UI 소스 검사와 설치 작업 화면 확인. 동일 사용자 흐름 미확인 [L16](#l16) |
| GRAPH-02 | `.mix` 저장·열기·앱 재시작 후 그래프·배치·별칭 복원 | 부분: 파일/복원 소스 검사. 최종 설치본 파일 선택 대화상자를 포함한 전체 흐름 미확인 [UI12](#ui12) | 부분: 공통 소스 검사. 설치본 전체 흐름 미확인 |
| GRAPH-03 | 여러 그래프 동시 실행·공유 장치 소유자 1개·일부 삭제 시 나머지 유지 | 부분: 공통 catalog/소유권 소스 검사. 유료 다중 그래프 설치본 수락 미확인 [P](#p) | 부분: 공통 catalog/소유권 소스 검사. 유료 다중 그래프 설치본 수락 미확인 [P](#p) |
| DSP-01 | 내장 효과·bypass·Off·Mute·미터·파라미터 저장 | 부분: PCM·bypass·Off·Mute·Dynamics 검사 및 설치본 효과 설정·프리셋·EQ 키보드 확인. 전체 입력 수단·효과 조합 제외 [M11](#m11)·[P](#p) | 부분: 공통 DSP/엔진 소스 검사. 설치본의 같은 효과 조작·청취 흐름 미확인 |
| DSP-02 | 프리셋 적용·수정·저장·Undo의 결과와 실제 청취 | 부분: 설치본 설정 조작 기록. 프리셋 청취/튜닝 완료는 아님 [P](#p) | 미확인: 설치본 청취·복원 완료 근거 없음 |
| PLUGIN-01 | VST3·CLAP 검색→PCM→에디터→상태 저장/복원→제거 | 부분: 소스 검사·native CLAP fixture PCM/상태/지연/회수 기록. 대표 외부 플러그인 전체 설치본 흐름 미확인 [P](#p)·[M13](#m13) | 부분: 호스트/형식 소스 검사. 대표 외부 플러그인 전체 설치본 흐름 미확인 |
| PLUGIN-02 | AU·AUv3의 같은 호스팅 흐름 | 부분: macOS 지원 명세·호스트 코드. 대표 제품 설치본 수락은 미확인 [P](#p) | 비대상: macOS 전용 형식 |
| PLUGIN-03 | LV2의 같은 호스팅 흐름 | 비대상: Linux 전용 형식 | 부분: Linux 지원 명세·호스트 코드. 대표 제품 설치본 수락은 미확인 [P](#p) |
| REC-01 | 앱 캡처 → 녹음기 입력 신호 도착 | 확인: 설치본 source/recorder 양쪽 2ch 약 0.05·fault 0 [M18](#m18). 파일 완성/재생 검증은 아님 | 미확인: 설치본 녹음기 입력의 명시적 수락 기록 없음 |
| REC-02 | UI로 녹음 시작·pause·resume·정지 후 파일 PCM·길이·헤더 확인 | 부분: 녹음 파일·종료·경쟁 소스 자동 검사. 설치본 UI의 전체 파일 흐름 미확인 [UI12](#ui12)·[P](#p) | 부분: 공통 녹음 소스 검사. 설치본 UI의 전체 파일 흐름 미확인 |
| REC-03 | RF64 3개 형식·1–8ch·4GiB 경계·외부 리더 호환 | 부분: 소스 자동 검사와 FFmpeg 디코딩 기록. 각 OS의 설치본 장시간 파일 수락은 미확인 [M16](#m16)·[P](#p) | 부분: 같은 녹음 코드가 있으나 Linux 설치본에서 생성한 경계 파일·외부 리더 확인은 미확인 |
| LIC-01 | 무료 한도 경계·초과 시 기존 작업 유지 | 부분: UI/엔진 소스 검사와 설치본 Free 표시. 모든 한도 실조작 수락은 미확인 [P](#p) | 부분: 소스 검사·정상 Secret Service 세션 Free 표시 [L16](#l16) |
| LIC-02 | 실제 구매·키 활성화·다른 기기·30일 오프라인·해제·환불 후 무료 유지 | 부분: 가짜 시계·HTTP·저장소 검사. 실제 구매/기기 이전/환불 연동 미확인 [P](#p) | 부분: 공통 인증 소스 검사. 실제 구매·Linux 자격 증명 저장을 잇는 흐름 미확인 [P](#p) |
| UPDATE-01 | 공개 업데이트 조회·버전/OS/CPU 선택·다운로드·설치 보존 | 부분: 조회·선택·UI 소스 검사. 공개 배포와 실제 전체 업데이트 흐름 미완료 [P](#p) | 부분: 공통 조회 코드와 DEB 수동 업그레이드 기록. 공개 조회→설치 전체 흐름 미확인 |
| UX-01 | 테마·좁은 창·긴 번역·키보드·포커스 복귀 | 부분: Storybook·axe·일부 설치본 키보드 확인. 전체 11언어·입력 조합의 설치본 확인은 아님 [UI12](#ui12)·[M12](#m12) | 부분: UI 소스/axe와 WebKitGTK 작업 화면 확인. X11/Wayland·키보드 전체 조합 미확인 [L16](#l16) |
| UX-02 | 실제 스크린리더에서 역할·이름·상태·조작 확인 | 미확인: 실제 청취 미완료 [P](#p) | 미확인: 실제 플랫폼 접근성 미완료 [P](#p) |
| CONTROL-01 | Stream Deck에서 그래프·노드·볼륨·녹음 제어 | 부분: 플러그인 소스 테스트 있음. 실제 앱·Stream Deck 장치의 연결 수락 근거 없음 | 부분: 공통 제어 코드 있음. 실제 장치·클라이언트별 지원·수락 근거 없음 |
| DIAG-01 | 수동 진단 저장·취소·덮어쓰기·오디오/개인정보 제외 | 부분: 소스 계약. 최종 설치본의 OS 파일 대화상자까지 포함한 실행 근거 없음 [P](#p) | 부분: 소스 계약. 같은 설치본 흐름 미확인 |

## 기존 검사 도구와 아직 없는 연결

검사 도구가 존재하는 것과 해당 환경에서 실행한 결과는 별개다. 아래 도구는 현재 코드에서 확인했다. 파라미터·권한·fixture 조건은 각 파일과 [lab 운영 문서](../../infra/lab/README.md)를 따른다.

| 도구·테스트 | 검사 층·대상 | 연결할 시나리오·한계 |
| --- | --- | --- |
| [native-check.mjs](../../scripts/lab/native-check.mjs), `pnpm lab:test` | macOS/Linux 빌드·소스 검사. `notVerified`에 설치·실오디오·플러그인 UI 등이 명시됨 | 실행 결과를 설치본 E2E 통과로 올리지 않음 |
| [App.test.tsx](../../apps/control-ui/src/app/App.test.tsx), `pnpm --filter @mixrelay/control-ui check` | mock port·jsdom·Storybook/axe·빌드 | GRAPH-01/02·LIC-01·UX-01의 일부. 엔진·OS 대화상자·실제 오디오 없음 |
| [catalog 실행 검사](../../apps/engine-service/src/catalog_execution_tests.rs), [endpoint 설정 검사](../../apps/engine-service/src/endpoint_settings_tests.rs), [장치 생성 UI 검사](../../apps/control-ui/src/features/routing-workspace/useVirtualEndpointCreation.test.tsx) | 그래프·공유 소유권·장치 편집의 소스 경계 | GRAPH-03·DEVICE-01. 실제 유료 키·OS 장치·설치 UI 흐름과 구분 |
| [macos-installed-capture-acceptance.mjs](../../scripts/lab/macos-installed-capture-acceptance.mjs) | macOS 설치본 신호 앱·실행부·엔진·가상 출력/recorder 입력 | AUDIO-02·REC-01. 빈 선택 그래프와 소유 endpoint 요구. UI 조작·녹음 파일 완성 제외 |
| [macos-latency-acceptance.mjs](../../scripts/lab/macos-latency-acceptance.mjs), [왕복 프로브](../../scripts/probe-macos-roundtrip.sh) | macOS 설치본 지연 보고·실제 신호 왕복 | AUDIO-01/09. `--self-test`는 합성 분석이며 실장치 결과가 아님 |
| [linux-device-probe.mjs](../../scripts/lab/linux-device-probe.mjs), [격리된 PipeWire runner](../../native/linux/pipewire/tests/run-application-retracking.sh) | Linux 프로덕션 엔진과 fixture 장치의 실제 PipeWire PCM·복구 | AUDIO-05/06/07. 일반 사용자 VM에서 명시적 fixture로 실행. 물리 장비·설치 UI 제외 |
| [linux-desktop-launch-check.py](../../scripts/lab/linux-desktop-launch-check.py) | Linux 설치된 `.desktop`·서비스 시작·화면 캡처·정리 | APP-01. 실제 녹음·그래프 편집 전체 흐름을 자동 조작하지 않음 |
| [audio-recorder tests](../../crates/audio-recorder/src/tests.rs) | PCM·종료·파일 형식 소스 검사 | REC-02/03. 외부 FFmpeg 검사는 `native-lab`과 실제 도구가 필요 |
| [licensing tests](../../crates/licensing/src/tests.rs), [데스크톱 인증 수명 검사](../../apps/desktop/src-tauri/src/licensing/lifecycle_tests.rs) | 인증 정책·시계·HTTP·저장소 경계 | LIC-02. 실제 결제·환불·OS 자격 증명 수락과 구분 |
| [updates tests](../../apps/desktop/src-tauri/src/updates_tests.rs) | 업데이트 응답·플랫폼 선택 등 소스 검사 | UPDATE-01. 공개 다운로드·설치 전체 흐름과 구분 |
| [Stream Deck tests](../../apps/streamdeck-plugin/src/plugin.test.mjs) | 플러그인 코드 검사 | CONTROL-01. 실물 장치·플랫폼 앱 연동 확인과 구분 |
| 공통 자동 E2E runner | 미구현 | 같은 ID·입력·기대 결과로 macOS와 Linux 실제 앱을 구동하고 신호/파일/상태를 판정하는 연결이 없음 |

기존 macOS/Linux 프로브는 재사용할 수 있지만 테스트 신호·길이·수락 기준이 서로 다르다. 양쪽의 유한 PCM 성공을 동일한 공통 E2E 시나리오 통과로 합산하지 않는다. UI 시나리오를 브라우저에서 수행하면 Tauri IPC·키체인·OS 대화상자·설치 경계가 빠지므로 그 실행 층도 결과에 남긴다.

## 알려진 실패와 과거 실패의 처리

| 조건 | 판정 | 근거·재검증 범위 |
| --- | --- | --- |
| 개발 Mac의 LG HDR 4K 출력 시작 | 실패: 현 장치 상태의 예외 | CoreAudio 단독·AUHAL·aggregate도 시작 timeout·callback 0. SDQHD/built-in 성공과 분리. 제품 단독 결함이나 모든 LG 지원 실패로 확정하지 않음 [M17](#m17) |
| macOS 전체 시스템 캡처 | 구현 공백: 명세와 현재 제품 경로 불일치 | 정확한 bundle 선택 이외를 거부하는 엔진 resolver와 `AllExcept` 거부 테스트를 소스에서 확인. 과거 런타임 실패 기록을 새로 만든 것은 아님. 제품 명세의 캡처 구현 상태를 따름 |
| macOS 초기 설치본의 캡처 무음·왕복 정렬 실패 | 해당 이전 빌드 실패 이력, 후속 조건에서 해결 확인 | `981f8b8` 왕복 및 `fdaf814` 캡처 통과를 각각 명시. 과거 실패를 현재 미해결로 남기거나 최신 HEAD 통과로 확장하지 않음 [M17](#m17)·[M18](#m18) |
| Linux 초기 Weston의 screen orientation 오류·데이터 폴더 소켓 거부 | 해당 이전 설치본 실패 이력, 후속 DEB에서 해결 확인 | React Aria 수정·runtime socket 이동 후 설치 런처·렌더 통과. 전체 Wayland 호환성 완료와 구분 [L16](#l16) |
| Ubuntu 24.04 기본 오디오 의존성 | 지원 하한 미달 | PipeWire 1.0.5의 헤더 요구 불충족. 정상 Linux VM 전체의 실패가 아님 [L16](#l16) |

기록에 `ignore`·환경 미준비·권한 대기·중단이 있으면 실행 안 된 범위로 남긴다. 시험 도구나 fixture 준비가 실패한 경우 제품 기능의 실패와 원인을 나눠 적되, 해당 실행을 통과로 처리하지 않는다.

## 근거와 검증 빌드

### P

[제품 명세](../product.md)의 지원 환경·무료 기능·녹음·플러그인·구현 상태. 이 문서는 제품 명세의 미완료 상태를 완료로 바꾸지 않는다.

### CI18

[2026-10-18 소스 검사 기록](current.md#2026-10-18-제품-코드-unwrapexpect-제거와-설치본-재확인): PR #2, `c8755cb`의 Debian·Ubuntu·Kali x86/ARM, Arch x86, macOS 15·UI 검사 통과 기록. 당시 CI는 macOS 15와 Linux 배포판/CPU 7조합이었다. 현재 [.github/workflows/native-lab.yml](../../.github/workflows/native-lab.yml)은 지원 계약의 행렬을 읽고, [compose](../../infra/lab/compose.yaml)도 같은 계약에서 생성한다. 현재 설정의 존재와 최신 원격 실행의 성공을 동일시하지 않는다.

### M19

[2026-10-19 macOS 14 설치](current.md#2026-10-19-macos-14-설치-확인), [업데이트·제거](current.md#2026-10-19-macos-14-업데이트제거-확인), [첫 공증](current.md#2026-10-19-첫-공증-릴리스-빌드). 설치 성공은 수정 PKG `777dcd5`, macOS 14.8.7 arm64/headless의 기록이다. 공증·설치·제거 각 절의 산출물을 한 개의 모든 시나리오 통과 후보로 묶지 않는다. 실제 GUI 인증·XPC 완료 응답·캡처·녹음은 확인하지 않았다.

### M18

[2026-10-18 설치본 재확인](current.md#2026-10-18-제품-코드-unwrapexpect-제거와-설치본-재확인): 소스 `fdaf814`, Developer ID 서명 0.1.0 PKG, 설치된 앱·엔진·Driver·HAL 해시의 패키지 일치. `/private/tmp/mr-installed-20261018/`에 캡처→가상 출력, 캡처→녹음 입력, 지연 보고 기록을 인용한다. 이 빌드의 왕복 실측은 권한 조건 때문에 하지 않았다.

### M17

[2026-10-17 입력 시각 보존·CoreAudio 안정성](current.md#2026-10-17-입력-시각-보존과-coreaudio-안정성): 최종 `981f8b8` 14:28 KST PKG, 엔진 SHA-256 `a4a335c13586b2c2c718708c4d16c05c400c881dda4c7685186b48d2ce9d33ca`, 패키지 SHA-256 `d353a29a56fe653ebba6bccb628a175d398d3371434b0383e26c10d7123a949b`. `/tmp/mixrelay-installed-stability-20261017/`의 단일 입력 왕복·가상 출력·기본 패스스루 결과와 다중 입력 합성 시험을 구분한다. 같은 절에 LG HDR 4K 장치 예외가 있다. 기록 중 덮어쓴 이전 PKG 경로를 현재 다운로드 링크로 사용하지 않는다.

### M16

[2026-10-16 후속 소스 검사](current.md#2026-10-16-후속-소스-검사), [오디오 구현 경로 대조](current.md#오디오-구현-경로-대조): FFmpeg RF64 디코딩과 native tap의 유한 구간 시험 기록. 설치된 실행부 전체의 권한·장애 복구와 구분한다.

### L16

[Linux 검증 VM](current.md#linux-검증-vm), [오디오 구현 경로 대조](current.md#오디오-구현-경로-대조): 2026-10-16 Debian 13, Proxmox x64 VM 107의 정상 일반 사용자 세션. 최종 전체 DEB SHA-256 `971062670b448e65fe0c8d52f6d433be946f55ef7d3b4fb3a21173d9184cadc7`. `loopback-installed-desktop-login-session-20261016`, `app-lab-oU7gvJ/device-evidence/`, `loopback-engine-pcm-sourcefix-20261016`, `loopback-forward-sourcefix-20261016` 등 각 작업의 실행 범위를 따로 사용한다. 설치 DEB·별도 lab의 프로덕션 엔진·fixture 시험이 모두 같은 패키지 파일에 대한 단일 E2E는 아니다. PipeWire·WirePlumber 정확한 실행 버전은 인용 기록에 없으므로 최소 요구 버전을 측정 버전으로 쓰지 않는다.

### M11

[로컬 개발 설치본](current.md#로컬-개발-설치본): 2026-10-11 macOS 27.0.1에서 실제 Dock 중지·장치 회수·재연결·설정 보존 확인. 녹음 중 Dock 클릭 조합은 직접 하지 않았으며, 종료·녹음 파일 자동 검사와 실제 Dock 연결 검사로 검증한 기존 정책을 유지한다. 이 인벤토리 때문에 해당 조합을 새 필수 실기 항목으로 추가하지 않는다.

### M12

[로컬 개발 설치본](current.md#로컬-개발-설치본): 2026-10-12 설치 UI의 일부 키보드/포커스·CLI 제거 기록. 채널 연결 생성 완료·실제 스크린리더 청취와 구분한다.

### M13

[현재 소스](current.md#현재-소스)와 [로컬 개발 설치본](current.md#로컬-개발-설치본)의 2026-10-13 기록: 엔진 비정상 종료 후 일시정지 유지, Dock 중복 해소, 설치·업데이트 뒤 최초 연결 해결, native CLAP fixture. 서로 다른 실행과 소스/설치 층을 합쳐 전체 통과로 쓰지 않는다.

### UI12

[현재 소스](current.md#현재-소스)의 2026-10-12 UI·Storybook·녹음 종료 검사와 [UI CI](../../.github/workflows/ui-quality.yml). 실제 엔진·파일 선택·장치·스크린리더 청취와 구분한다. 이후 Linux WebKitGTK 작업 화면 확인은 L16에 따로 둔다.

## 결과를 갱신할 때

각 실행은 `시나리오 ID / 환경 ID / 실제 OS 버전·CPU / 패키지·엔진 해시 / 검사 층 / 도구·명령 / 실행 시각 / 결과 / 원시 증거 / 회수 상태`를 기록한다. 테스트가 아직 없으면 `미구현`, 도구는 있으나 실행하지 않았으면 `미실행`, fixture·권한 등으로 시작하지 못했으면 `실행 불가`, assertion 실패는 `실패`로 남기고 `확인`에 포함하지 않는다. 일부 단계 성공은 `부분`으로 남긴다.

같은 시나리오를 두 OS에서 자동 실행할 때 ID와 기대 결과를 공유하고, 설치·실행·권한·신호 입출력·정리의 OS별 구현을 연결한다. OS 고유 기능은 해당 대상에만 적용한다. 앱·엔진 API를 직접 호출한 실행은 UI E2E와 구분하며, 현재 환경에 없는 검사를 조용히 skip한 뒤 전체 통과로 표시하지 않는다.

소스·의존성·패키지가 바뀌면 이전 결과의 검증 빌드를 보존하고 영향받은 범위의 재확인 여부를 기록한다. `0.1.0`이라는 버전 문자열만으로 동일 빌드라고 판단하지 않는다. 재검증은 변경 경계에 맞추며, 이 표의 빈칸 때문에 모든 OS·상용 장비·플러그인 조합을 새 출시 조건으로 추가하지 않는다.
