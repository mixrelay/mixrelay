# MixRelay Driver 배포 준비 기록 — 2026-10-10

이 기록은 현재 소스와 검증된 배포 파일의 범위를 구분한다. 제품 버전은 0.1.0이며
패키지 파일은 로컬 `artifacts/`, 원본 시험 결과는 `.lab/`에 보관한다.
패키지 파일·인증 자료·사용자 설정 백업은 Git에 포함하지 않는다. 공개 배포는 수행하지 않았다.

## 현재 구현

- 명시적인 Driver 메뉴 종료는 장치와 루프백을 중지하고 저장된 설정과 종료 상태를 보존한다.
  관리 앱을 다시 열면 장치와 형식을 복원한다. 그래프 앱 종료와 창 닫기는 별도 동작이다.
- 앱·Driver 계약 전문, 메뉴와 설치/제거 안내는 영어·일본어·한국어·중국어 간체/번체·
  프랑스어·독일어·스페인어·포르투갈어·이탈리아어·러시아어를 지원한다.
- macOS는 설치·업데이트 PKG에 전용 제거 앱을 함께 설치한다. Installer 플러그인과
  설치 유형의 제거 선택을 사용하지 않는다. 업데이트가 종료시킨 Driver 관리 앱과
  MixRelay 앱은 성공 후 원래 사용자 세션에서 다시 실행한다.
- macOS 제거는 고정된 권한과 서명된 XPC helper를 사용한다. 이미 없는 제거 대상은
  완료된 상태로 처리하며 임시 transaction/실행 파일 복사본은 성공과 실패 후 모두 정리한다.
  CoreAudio 조회는 별도 프로세스와 제한 시간으로 무한 대기를 방지한다.
- Linux는 DEB·RPM·Arch 패키지 관리자 경로를 사용한다. 일반 사용자 PipeWire 서비스로
  실행하며 기본 제거는 설정을 보존하고 선택 삭제는 정확한 Driver 설정 파일만 삭제한다.

## macOS 실측과 후속 리팩터링

최종 서명 PKG의 공증 Accepted, ticket 및 Gatekeeper 허용을 확인했다.
사용자가 이 패키지로 설치 1회와 제거 1회를 완료했다. 설치 후 서비스 Ready와
설치된 서명 payload 일치를 확인했고 제거 후 여섯 대상 경로·두 launchd job·HAL·
설정 파일·권한 등록·임시 staging 및 helper 실행 파일 복사본이 없음을 확인했다.

이후 설치/제거 코드를 정리하고 Rust 검사 136개, 패키징 및 서명 XPC 검사 174개,
fmt·경고 금지 Clippy·x86_64 전체 target 컴파일 검사를 통과했다.
리팩터링 소스는 아직 새 PKG로 다시 빌드·서명·공증하지 않았다.
아래 PKG는 실제 설치/제거를 검증한 기존 파일이며 리팩터링 후에도 해시가 동일하다.

## Linux 실측

| 환경 | 배포 파일 | 실제 확인 |
| --- | --- | --- |
| Debian 13 x86_64 KVM | DEB | 설치, 관리자 메뉴 종료/복원, 실행 중 제거 차단, 같은 버전 교체, 설정/종료 상태 보존, 제거, 재설치 |
| Fedora 44 x86_64 KVM | RPM | Debian과 같은 수명 검사, PCM 검사, 제거 창의 설정 선택 삭제 |
| Arch rolling x86_64 KVM | pacman 패키지 | Fedora와 같은 검사, native makepkg 빌드와 PreTransaction 제거 차단 |

세 환경의 설치 서비스는 동일한 SHA-256의 root 소유 0755 실행 파일이었다.
RPM의 잘못된 glibc 의존성 이름은 실제 ABI capability 요구로 수정했다.
제한적인 빌드 umask에서 service sidecar가 0700으로 복사되는 문제도 수정했다.
Arch recipe는 검사한 DEB 공통 payload를 SHA-256으로 고정하며 실행 파일을 strip하지 않는다.

Fedora와 Arch에서 2채널 Float32 및 64채널 Float32·PCM16·PCM24·PCM32를 각각 두 독립
client scope로 검사했다. 총 20개 구간에서 각각 fresh=480,255, 누락·반복·오샘플·short=0이었다.
이는 유한 PCM 구간의 판정이며 시작/종료의 모든 transfer counter가 0이라는 뜻은 아니다.
Fedora의 최종 수명 검사에서는 정확히 같은 RPM 해시에서 먼저 통과한 PCM 결과를 유지했다.
Debian의 이전 PCM 실측은 [제품 기록](../product.md)에 별도로 보존하며 이번 재실행으로 표시하지 않는다.

관리자 메뉴는 최소 X11 세션의 시험용 StatusNotifier 호스트에 실제 D-Bus 이벤트를 전달했다.
Fedora/Arch 선택 삭제는 실제 Zenity 창과 Polkit 경로를 실행했고 시험 계정의 고정 패키지
관리 프로그램만 허용하는 임시 인증 규칙을 사용했다. 규칙은 검사 후 제거했다.
일반 데스크톱의 트레이 아이콘 시각 배치와 인증 암호 입력 자체는 이 시험 범위에 포함하지 않는다.
패키징·번역·빌드 경계 회귀 검사 69개가 통과했다. 세 시험 VM은 종료해 두었다.

## 산출물 SHA-256

| 파일 | SHA-256 |
| --- | --- |
| `MixRelay-Driver-Installer-0.1.0.pkg` | `3f10a4b5b59c396ff3e96049657dcc5d722e32f3a82e44f0734ae2b1a41ec71b` |
| `MixRelayDriver_0.1.0_amd64.deb` | `30e2aabda037f5e36a5016c66ef2a545f3818b413b99a8adec2da1d6266be36b` |
| `MixRelayDriver-0.1.0-1.x86_64.rpm` | `b815074ac4de68c704c2aee446d4eb2fb5558fb88b42ad2cb17259d7a97cbe73` |
| `mix-relay-driver-0.1.0-1-x86_64.pkg.tar.zst` | `ea865a8a016a4d9e6ed29b9f8cdb8a38e09f177853319a3c44305d04299fa933` |
| `arch-recipe-release.tar.gz` | `d0d8ac42b4be44184c5f3986e185880656c83b539390b3c6a0d7fd4ae800b5da` |

Linux 패키지 저장소 서명·공개 게시·AUR 등록은 수행하지 않았다.
서로 다른 버전 사이의 업그레이드, Linux ARM64 설치 실행, 모든 파생 배포판 및 실제 장비의
장시간 운용까지 검증한 것으로 확대하지 않는다. LinuxKit 가상화 환경의 간헐 PCM 누락은
기존 known issue로 유지한다. App 유료 정책과 판매 운영의 미완료 항목도 Driver 준비와 별개다.

세부 절차는 [Linux 패키징](../../packaging/linux/README.md),
[macOS 패키징](../../packaging/macos/README.md), [제품 체크리스트](../product.md)를 따른다.
원본 근거는 `.lab/linux-release-20261010/release-verification.json`,
`.lab/driver-release-20261009/always-cleanup-final/`,
`.lab/driver-maintenance-20261009/refactor-verification.json`에 보관한다.
