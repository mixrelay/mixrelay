# Cross-OS development lab

제품 동작과 현재 결과·다음 실행은 [제품 문서](../../docs/product.md)에서 관리한다.
이 문서는 검증 환경 사용법이며 별도 출시 체크리스트가 아니다.
검증 대상은 macOS·Linux다.

## 환경과 범위

| 대상 | 사용 경로 | 한계 |
| --- | --- | --- |
| macOS | 현재 Mac의 정상 로그인 세션 | Universal 빌드가 Intel 실제 실행을 증명하지 않는다. |
| Linux | 기존 Debian/Ubuntu VM의 정상 사용자 PipeWire 세션 | 배포판 결과를 복제하지 않는다. VM timing은 물리 timing이 아니다. |

Kali/Arch는 선택 환경이다. 이 문서 때문에 새 VM·백엔드·물리 64채널 장비·
플러그인 전수 조합을 출시 조건으로 추가하지 않는다. 기존 실제 PCM·채널 폭·
클록·자원 수명·설치 조건은 매트릭스에서 유지한다.

Docker는 컴파일용이며 desktop 로그인·native audio·editor 검증을 대체하지 않는다.
Docker 컨테이너·Docker 기반 VM에서 제품 오디오를 실행하지 않는다. Proxmox 등 x64 Linux VM의
정상 사용자 PipeWire 세션을 사용하며, Docker의 오디오 제한을 일반 Linux VM의 결함으로 기록하지 않는다.
macOS VM은 필요 없다. 기존 VM을 재사용하고 무거운 빌드/오디오 계측은 한 번에 하나씩
실행한다. 경량 확인과 독립 코드 수정까지 이 제한으로 직렬화하지 않는다.
자원과 여유 공간을 확인하고 host audio interface를 임의로 guest에 넘기지 않는다.

## 실행과 결과 수집

```sh
pnpm lab:vm list
pnpm lab:vm start <UUID>
pnpm lab:vm status <UUID>
pnpm lab:vm shutdown <UUID>
```

정확한 VM UUID·사용자·실행 경로를 확인한다. 로그인은 승인된 기존 계정의 정상 절차로
수행한다. 암호를 저장하거나 계정·UAC·서명 정책·키링을 우회하지 않는다.
`shutdown`은 정상 종료 요청이며 VM을 강제 kill하지 않는다.

UTM 명령 반환은 guest 작업 완료가 아니다. Linux의 긴 작업은 기존
`scripts/lab/guest-job.sh <unique-id> <command> ...`를 사용한다.
해당 job의 `output.log`와 실제 `exit-code`를 함께 읽는다. marker가 없으면 미완료,
nonzero면 실패다.
수집 실패로 생긴 빈 파일이나 wrapper exit0을 성공 증거로 쓰지 않는다.

실행 전 source/profile/artifact·설정·기본 장치를 확인하고, 실행 후 기존 상태와
소유 PID·task·socket·native 객체의 정리를 확인한다. 실패 로그는 덮어쓰지 않는다.
기존 성공 결과는 영향받은 조건이 바뀌지 않으면 재사용한다.

## 빌드

guest 자신의 디스크에서 고정 Rust/Node/pnpm과 lockfile을 사용한다. macOS의
`target`·`node_modules`를 guest 빌드에 재사용하지 않는다. source archive/container에
credentials·사용자 graph·local agent context를 포함하지 않으며 private 이미지를 공개하지 않는다.

```sh
docker compose -f infra/lab/compose.yaml build debian
docker compose -f infra/lab/compose.yaml run --rm debian build
```

`test`는 코드 검사를 추가한다. image/package inventory와 실제 종료코드를 보존한다.
compose/CI에 target이 있다고 실제 오디오 검증이 끝난 것은 아니다.
emulated Arch의 package sandbox 실패를 보안 설정 해제로 우회하지 않는다.

desktop 코드 검사는 공통 경로를 사용한다:

```sh
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-desktop --lib
```

제품 빌드는 `pnpm desktop:build`이며 실제 driver/sidecar·서명 검사를 유지한다.
검사용 dummy payload나 VM별 Tauri 설정 우회를 만들지 않는다.

## OS 준비

- Linux: `infra/lab/install-linux-deps.sh`와 고정 toolchain을 준비하고, 정상 사용자
  세션에서 `systemctl --user status pipewire wireplumber`, `wpctl status`를 확인한다.
  다른 PipeWire server를 같은 socket 위에 시작하거나 root로 MixRelay를 실행하지 않는다.
  bootstrap은 기존 non-root 계정만 사용한다. editor는 실제 XWayland DISPLAY를 확인한다.
- macOS: full Xcode와 기존 signed 앱/엔진/HAL을 사용한다. 정상 OS 권한·Keychain
  승인 절차를 사용하며 권한 DB·ACL·시스템 오디오를 임의로 초기화하지 않는다.

## 검증 선택

변경한 실제 실행 경계를 먼저 검사한다. 가벼운 unit/parse 검사로 테스트 도구의
입력·출력·종료코드부터 확인한 후 native 검증한다. 테스트 도구 오류와 제품 결함은
분리해 기록한다. 새로운 가설 없이 같은 실행을 반복하거나 작은 변경마다
전체 workspace·driver 설치를 다시 하지 않는다.

plugin은 실제 검색→PCM→설정/editor 변경→저장→재시작/복원→제거 한 실행에서
확인한 조건을 함께 기록한다. clock은 실제 frame/timestamp·누락/반복·지연을 측정한다.
수동 block 처리나 sleep pacing을 실시간 증거로 쓰지 않는다. 기준·warmup·무음 허용을
결과에 맞춰 바꾸지 않으며 실패 원시도 보존한다.

실제 환경 시험은 `native-lab` feature와 정확한 시험 이름을 함께 지정한다. 기본
workspace 검사는 이 feature를 켜지 않는다. PipeWire 시험은 기존 private lab의
fixture·소켓·설정 환경을 먼저 준비하고 직렬 실행한다. feature만 켰다고 사용자
세션에 시험 장치·앱을 만들지 않는다. 시험이 요구하는 명시적 fixture 변수도 필요하다.
제어 API를 여는 lab의 작업 폴더와 상위 경로는 해당 사용자 또는 root만 바꿀 수 있어야 한다.
다른 사용자가 쓸 수 있는 경로에서는 실제 제어 소켓의 보안 검사가 시작을 거부한다.

설치된 Linux 런처는 `python3 scripts/lab/linux-desktop-launch-check.py /절대/새/증거폴더`로
검사한다. 전용 VM의 일반 사용자 로그인 세션, Xvfb·curl·Python Gdk 3가 필요하다.
기존 앱·엔진과 실행 중인 Driver unit이 있으면 거부한다. `.desktop`으로 실행해
Driver Ready·엔진 health와 실제 렌더 스크린샷을 남기고 시험 소유자를 종료해
처음의 비활성 서비스 상태를 복원한다. 자격 증명 상태까지 검사하려면 정상 사용자
로그인의 Secret Service도 준비한다. 이 도구가 키체인이나 사용자 암호를 설정하지는 않는다.

```sh
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-engine-service --features native-lab --bin mixrelay-engine-service EXACT_NATIVE_TEST -- --exact --test-threads=1
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-linux-driver --features native-lab --lib EXACT_NATIVE_TEST -- --exact --test-threads=1
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-pipewire --features native-lab --lib EXACT_NATIVE_TEST -- --exact --test-threads=1
node scripts/lab/native-check.mjs cargo test --locked -p mixrelay-audio-recorder --features native-lab --lib ffmpeg_decodes -- --test-threads=1
```

FFmpeg 시험은 실제 `ffmpeg`·`ffprobe`가 필요하며 RF64의 4GiB 경계까지 독립 디코딩한다.
macOS aggregate 시험은 `MIXRELAY_LAB_AGGREGATE_OUTPUT_UIDS`에 개행으로 구분한
출력 UID 두 개를 지정하고 `macos_devices::aggregate::native_tests::`의 정확한 시험을
직렬로 실행한다. 두 출력에는 무음만 쓰며 시스템 기본 장치는 바꾸지 않는다.
앱 캡처 clock 시험은 기존 `scripts/check-macos-application-clock.sh`의 전용 build
fixture와 명시적 `--run`을 사용한다. soak·성능 비교와 종료용 자식 fixture의 ignore는 유지한다.

빌드·합성 fixture·실제 앱/장치·최종 패키지의 결과는 구분한다.
매트릭스보다 넓은 필수 환경이나 시나리오를 이 문서에서 만들지 않는다.
