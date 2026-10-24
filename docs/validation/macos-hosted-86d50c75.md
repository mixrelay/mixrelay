# 86d50c75 macOS hosted E2E 조사

실행 `40048258432`, workflow `.github/workflows/candidate-e2e.yml`, 후보·suite
`86d50c7530c87ab45c5304a9a81c1bde04e4ae50`을 조사했다. 증거 루트는
`/tmp/mr-e2e-r4/art/`다. 기존 네 대상에 `installed-target-macos-14-arm64`를
추가로 내려받았다. GRAPH-01의 도구 오류는 수정했고 AUDIO-01은 미해결이다.

## AUDIO-01: 확정된 대기 호출과 남은 원인 조사

`installed-target-macos-26-arm64/fresh/live-coreaudio-probe.txt:5`에는 MixRelay IO의
입출력이 모두 있다. `fresh/live-coreaudio-system.log:1212`에는 HAL 로드,
1213행에는 원격 HAL 호스트 PID 10185 생성, 1234행에는 장치 168 활성화가 있다.
`fresh/live-app-log-engine.stderr.log:9`에는 입력 owner=1 준비,
10행에는 `01:11:26.203466` 연결 시작이 기록됐다.

`fresh/live-engine.sample.txt:418`의 장치 작업자 스택은 다음과 같다.

```text
AudioDeviceCreateIOProcID_mac_imp
  HAL_HardwarePlugIn_DeviceCreateIOProcID
    HALC_ShellDevice::CreateIOProcID
      HALC_ProxyIOContext::_TellServerAboutStreamUsage
        HALC_ProxyObject::SetPropertyData
          mach_msg2_trap
```

새로 받은 macOS 14를 포함한 다섯 대상의 fresh 스택이 같은 호출에서 기다린다.
upgrade 스택이 있는 macOS 14·15 x64·27 arm64도 같다. macOS 26 arm64와
15 arm64의 upgrade 보고서는 baseline 설치 실패로 시나리오가 blocked다.
두 폴더의 엔진 로그에는 fresh 시각의 내용이 남아 있으므로 독립된 upgrade 재현으로
계산하지 않는다.

호출 경로는 `macos_devices::worker::WorkerContext::connection` →
`Native::connect` → `Host::connect` → transport `device_io::lifecycle::connect` →
`Registration::create` → `System::create`다. `AudioDeviceStart`는 create가 반환한 뒤
호출된다. 따라서 이 스택은 StartIO·zero timestamp·PCM 진행 대기를 가리키지 않는다.
`docs/validation/current.md`의 LG 출력 기록은 `AudioDeviceStart` 내부 대기다.
공통 서버 원인이 있는지는 현재 자료로 판정할 수 없다.

`installed-target-macos-26-arm64/upgrade/live-coreaudio-system.log:32818`에는
fresh 작업자의 `01:11:55.912` SetPropertyData 오류 `0x10004003`이 있다.
32820행의 `01:12:25.919`에도 같은 오류가 있다. SDK의 `mach/message.h`에서
이 값은 `MACH_RCV_TIMED_OUT`이다. 엔진 준비 응답의 5초 timeout과 별개로
네이티브 IPC가 이후에도 기다렸음을 보여 준다.

서버측 스택은 수집되지 않았다. 기존 로그 predicate도 coreaudiod 메시지에
`mixrelay` 문자열이 있을 때만 보존하고 원격 HAL 호스트의 일반 메시지는 버린다.
`fresh/live-app-log-driver-runtime-10406.log`는 0바이트다. 이 자료로는 서버의
어느 lock·callback·알림이 응답을 막았는지 확정할 수 없다. 이전 stream-active
재알림 수정의 회귀 시험 통과를 이번 실패의 원인 증명으로 사용하지 않는다.

제품 동작을 추정으로 변경하지 않았다. 수집기는 사용자 ID 제한 없이 coreaudiod와
원격 Core-Audio-Driver-Service의 PID 목록·권한 있는 sample을 저장하고, 해당
프로세스의 CoreAudio 로그를 메시지 문자열 필터 없이 보존한다. Driver 서비스의
스택도 추가한다. AUDIO-01의 제품 수정·설치본 검증은 남아 있다.

## GRAPH-01: codesign 출력 파싱과 csreq 인자

`installed-target-macos-26-arm64/fresh/installed-ui.log`의 실제 출력은 다음과 같다.

```text
# designated => cdhash H"32ba2e5952d60363e6b223f989834320e067bc61"
Executable=/Users/runner/work/mixrelay/mixrelay/evidence/fresh/ui-tools/DerivedData/Build/Products/Debug/InstalledUITests-Runner.app/Contents/MacOS/InstalledUITests-Runner
```

기존 파서는 stderr에서만 `^designated =>`를 찾았다. 수정은 stdout·stderr에서
선택적인 `# ` 접두어를 처리하고 요구사항이 정확히 하나인지 검사한다.
로컬 실제 codesign 호출에서도 요구사항은 stdout에, Executable 진단은 stderr에 왔다.

후속 csreq 호출에도 오류가 있었다. `csreq -r`에 문자열을 직접 넘기면 파일 이름으로
해석해 `No such file or directory`로 실패한다. `-r =<requirement>`로 컴파일하고,
생성된 바이너리 파일을 `codesign --verify --strict -R <file> <runner>`로 검증한 뒤
그 파일의 바이트를 TCC SQL에 넣는다. build-for-testing → 서명 → 요구사항 검증 →
grant → test-without-building 순서를 유지한다.

## 검증과 실행 제약

- `node --test scripts/e2e/*.test.mjs`: 97개 통과, skip 없음.
  literal hosted 출력, 기존 서명·grant·재빌드 없는 실행 순서, 서버 스택 수집을 포함한다.
- 실제 로컬 앱 fixture에서 제품의 codesign·csreq 경로를 실행했다. ad-hoc 서명의
  cdhash 요구사항을 바이너리로 변환하고 TCC SQL의 바이트와 일치하는지 확인했다.
  같은 코드의 검증은 통과하고 다른 코드로 교체하면 거부된다. TCC DB 쓰기와
  AX 신뢰 확인은 대체한 경계다.
- 수정 전 제품 스크립트에 literal 출력을 주입하면 원래 assertion으로 실패한다.
  수정 전 csreq 인자도 실제 도구에서 실패한다. 원시는
  `/tmp/mr-e2e-r4/graph-regression.log`, 전체 Node 결과는 `node-tests.log`에 있다.
- `cargo fmt --all --check`, `git diff --check` 통과. Rust 소스 수정이 없어
  cargo test·clippy는 실행하지 않았다.

`tart clone mixrelay-sonoma-clean mixrelay-r4-macos-fix`는 성공했다.
이 샌드박스에서 `tart run --no-graphics mixrelay-r4-macos-fix`는 출력 없이
종료 코드 134로 중단됐다. `/tmp/mr-e2e-r4/tart-run-crash.ips`의 스택은
`Darwin.graphicsDevice → AppKit → GetCurrentProcess → _RegisterApplication → abort`다.
게스트 부팅에 도달하지 못해 후보 PKG 설치, 로컬 HAL·엔진 교체, 전체 XCTest의
build·sign·grant·run 흐름을 VM에서 검증하지 못했다. 복제본은 삭제했다.
원본 VM을 실행하거나 호스트에 MixRelay를 설치하지 않았다.

AUDIO-01 제품 수정은 남아 있다. 새 서버 스택 수집의 hosted 실행, 실제 TCC
Accessibility 승인, fresh·upgrade 수락은 미검증이다. 커밋·push·workflow dispatch는
수행하지 않았다.

## Round 5: run 40049943446

후보는 `qa-install-86d50c7`(소스 `86d50c75`), suite는 `0db9d7e1`이다.
이번 작업은 `codex/e2e-r5-macos` worktree에서 수행했다. 아래 증거 경로는 별도 표시가
없으면 `/tmp/mr-e2e-r5/art/installed-target-macos-26-arm64/` 기준이다.

### A. GRAPH-01: 가려진 제목에 대한 좌표 클릭

`fresh/installed-ui.error`에는 Compressor·Recorder 노드와 열린 Actions 메뉴가 모두 있다.
메뉴는 Rename display name·Disconnect cables·Delete node만 제공한다. 메뉴가 열리지
않았거나 Recorder 생성이 실패했다는 해석은 이 트리와 맞지 않는다.

XCTest의 `fresh/installed-ui.xcresult`에서 화면 녹화 attachment를 legacy object export로
추출했다. `/tmp/mr-e2e-r5/hosted-ui.mp4`의 6초 프레임
`/tmp/mr-e2e-r5/hosted-ui-6.png`를 직접 확인했다. Recorder가 Compressor의 제목 왼쪽을
덮고 있고 커서도 Recorder 안에 있다. AXStaticText Compressor의 중심 좌표를 우클릭하면
그 위치를 덮은 Recorder가 대상이 된다. 출력 포트가 없는 Recorder의 메뉴에는 채널 연결이 없다.

도구는 Compressor의 바깥 AXGroup을 찾아 AXShowMenu를 먼저 시도한다. 지원하지 않으면
그 그룹에 AX 포커스를 주고 제품이 이미 제공하는 Shift+F10 경로로 메뉴를 연다.
포커스 확인 없이 키를 보내거나 가려진 제목 좌표로 돌아가지 않는다. 실패 화면도 저장한다.
제품의 기존 키보드 메뉴 경로를 사용하는 수정이며, 제품 UI의 메뉴·배치·조작 계약은 바꾸지 않았다.

추가로 받은 `installed-target-macos-15-x64/upgrade/installed-ui.error`는 메뉴·연결 창·
Destination 목록을 연 뒤 Recorder 선택에서 실패했다. AXList Destination의 항목은
AXCell이 아니라 AXStaticText Recorder다. `/tmp/mr-e2e-r5/hosted-15-x64-ui.mp4`의
12초 프레임 `hosted-15-x64-ui-12.png`에서도 열린 Recorder 선택지를 직접 확인했다.
도구는 이름 있는 Destination 목록 안에서만 AXStaticText·AXCell·AXRow 선택지를 찾는다.
목록 바깥의 Recorder 노드 제목이나 다른 목록의 같은 이름을 누르지 않는 회귀 시험을 추가했다.

### B. AUDIO-01: 첫 사용 마이크 승인과 준비 한계의 충돌

`fresh/live-coreaudio-system.log:34618`에서 엔진 자신의 TCCAccessRequest는 background
session이라는 이유로 거부된다. 그러나 coreaudiod가 대신 요청한 user tccd 경로는 이어진다.
같은 파일 34664–34667행의 `msgID=4415.51`은 엔진 PID 4622를 대상으로
`01:32:02.938`에 `kTCCServiceMicrophone` 프롬프트를 시작한다.
34882행에는 `01:32:07.755`에 엔진 경로에 대한 TCC 레코드 생성 이벤트가 있다.

`fresh/live-app-log-engine.stderr.log:13`의 연결 시작은 `01:32:03.082357`이다.
`fresh/audio-graph-setup.failure.json`의 결과는 5초 준비 대기의 `Err(Timeout)`이다.
엔진 로그 15–16행은 `01:32:08.107999`의 이전 그래프 복귀와 `01:32:08.341120`의
연결 회수를 보여 준다. `fresh/audio-permission-actions.json`에는 UserNotificationCenter의
`01:32:08Z` 클릭 기록이 있다. GRAPH-01 실패 뒤에도 observer는 실행됐고 실제 프롬프트를
처리했다. 승인은 연결 준비 한계에 근접했고, 준비 완료는 그 안에 도달하지 못했다.
upgrade의 `native-signal.json`·`recording-analysis.json`은 같은 VM에서 AUDIO-01·REC-02가
통과했음을 보여 준다. fresh의 승인 상태가 upgrade에 남았다는 설명과 일치한다.

제품은 마이크 권한 상태를 구별하지 않았고 모든 자원 구성 오류를
`audio_device_disconnected`로 바꿨다. 엔진의 주입 가능한 Host 경계에 AVFoundation의
읽기 전용 권한 조회를 추가했다. 초기 상태가 미결정인 입력의 준비 timeout은 타입이 있는
`PermissionPending`과 `audio_permission_pending`으로 전달한다. 거부·제한된 입력의 실패는
`audio_permission_denied`다. 준비 한계와 native owner 회수는 유지하고, 실패한 후보 그래프를
게시·저장하지 않는다. 편집 앱의 기존 진단·초안 보존·재시도 경로에 각 언어의 권한 안내를 연결했다.
이미 승인된 입력·출력 전용 초기화의 장치 실패는 기존 오류로 유지한다.

도구는 UserNotificationCenter를 먼저 조사하고 prompt 소유자의 AX messaging 대기를
0.2초로 제한한다. 클릭 전에 action·dialog subject를 보존한다. 기존 기록의 빈 action은
클릭 후 사라진 AX 요소를 다시 읽은 결과였다. fresh 입력 적용이 명시적인
`configurationOutcome: not_applied` 마이크 권한 대기 응답이면 새 엔진 승인 기록을 기다린다.
그동안 authoritative 그래프가 바뀌지 않았는지 확인하고 같은 문서를 한 번 다시 제출한다.
거부·불확실한 응답·외부 변경·미완료 재시도에는 성공을 부여하지 않는다. PCM·녹음 assertion과
5초 장치 준비·30초 하네스 확인 한계는 늘리지 않았다.

`fresh/live-coreaudio-system.log:1410`에는 MixRelayHAL.driver의 원격 호스트 PID 4420이
있지만 `fresh/live-coreaudio-server-processes.txt`에는 coreaudiod PID 4415만 있다.
수집기는 registrar 로그에서 실제 MixRelay 호스트 PID를 가져와 현재 `ps` 신원을 확인하고
권한 있는 sample을 수집한다. 사라지거나 오디오 프로세스가 아닌 재사용 PID는 제외한다.
오디오 프로세스의 로그 predicate도 실제 실행 파일 이름에 대응하도록 확장했다.
같은 대상의 `upgrade/live-coreaudio-system.log`는 16MiB에서
`exit=null; signal=SIGTERM; error=spawnSync log ENOBUFS`로 끝난다. 시스템 로그는 pipe
버퍼에 모으지 않고 증거 파일로 직접 기록한다. 17MiB 뒤의 권한 이벤트·stderr·종료 진단을
보존하는 실제 자식 프로세스 회귀 시험을 추가했다. 수집 제한 시간은 유지했다.

### C. AUDIO-11: 그래프 게시의 202 pending

`upgrade/AUDIO-11.failure.json`은 revision 10·state pending·HTTP 202를 받자마자
`installed.mjs:431`의 200 assertion에서 실패했다. 서버의 그래프 적용 계약은 게시와
오디오 스레드의 활성화 확인을 구별하며, 이 응답은 정상적인 비동기 게시 결과다.
기존 도구는 바로 다음 활성 revision 확인에 도달하지 못했다. 이 증거만으로 revision 10이
나중에 활성화됐거나 영원히 pending이었다고 판단할 수 없다.

도구는 desktop의 작업 영수증 회수 뒤 받은 200·202 응답의 protocolVersion·revision·state를
검사한다. 요청을 반복하지 않고 같은 revision의 실제 활성화를 기다린 뒤 그래프의 revision·노드·
연결선을 대조한다. 그 뒤에만 전체 시스템 녹음을 시작한다. 실패·다른 revision·미활성화·다른
문서를 거부하는 회귀 시험을 추가했다.

### 로컬 검증과 남은 범위

완료된 macOS 15 arm64·x64 아티팩트를 추가로 내려받았다. arm64 fresh는 같은 가려진
제목 메뉴에서 GRAPH-01이 실패했고 AUDIO-01·REC-02는 통과했다. x64 upgrade는 위의
Destination 선택 실패와 AUDIO-11의 202 assertion이며 AUDIO-01·REC-02는 통과했다.
이 두 실행 단계의 AUDIO-11은 모두 같은 202 조건에서 실패했다. arm64 upgrade와 x64
fresh의 PKG-01은 `package-verification.json`에 보존된 stapler exit 68,
`NSURLErrorDomain Code=-1005`·CloudKit 조회 연결 유실이다. 제품 서명·공증을 건너뛰거나
해당 단계를 성공으로 바꾸지 않았다. 나머지 세 대상은 최종 조회 때 실행 중이었다.

- `node --test scripts/e2e/*.test.mjs scripts/lab/engine-operations.test.mjs`: 106개 통과,
  skip 없음. 원시는 `/tmp/mr-e2e-r5/node-tests-final.log`다.
- Swift UI 타깃의 Xcode `build-for-testing` 통과. 같은 helper·테스트 소스를 XCTest overlay와
  연결한 단독 실행에서 단위 시험 5개 통과. 각각 `swift-build.log`·`swift-unit-tests.log`에 있다.
  설치 앱에 입력을 보내는 전체 XCTest는 실행하지 않았다.
- `node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --lib`:
  342개 통과, 기존 격리 프로세스 시험 1개 ignored. 새 권한 실패·그래프/디스크 보존·명시적
  재시도 검사를 포함한다. `engine-lib-final.log`에 있다.
- 같은 래퍼의 엔진 `--lib --bins` 전체 실행은 lib 341개 통과(새 controller 시험 추가 전),
  bin 194개 통과·2개 실패·기존 native-lab 시험 4개 ignored였다. `cargo-tests.log`의 실패는
  입력 정렬 fixture의 source registry attach에서 발생한 EPERM과 native application clock의
  첫 attach assertion이다. 후자는 같은 공유 메모리 생성 경로를 사용하지만 C ABI가 errno를
  버려 원시 오류는 확인하지 못했다. 수정 후 권한 정책 시험 3개는 별도 실행에서 통과했다
  (`permission-policy-tests.log`). 시험을 skip하거나 기대값을 바꾸지 않았다.
- audio-io 전체는 99개 통과·1개 실패다. 실제 CoreAudio 인벤토리를 요구하는 기존 시험에서
  샌드박스에 오디오 장치가 보이지 않았다(`protocol-audio-tests.log`). control-protocol 27개는
  별도 전체 실행에서 통과했다(`protocol-tests.log`).
- touched Rust crate의 `cargo clippy --all-targets --all-features -- -D warnings` 통과
  (`clippy.log`). 이 검사에서 드러난 기존 native-lab TapConfig fixture의 누락 필드 세 개를
  단일 앱 캡처 값으로 보완했다. native-lab의 실제 캡처 실행은 하지 않았다.
- `pnpm --filter @mixrelay/control-ui check`: 2,197개 시험, 접근성 검사 243개,
  타입·경계·제품 빌드·WebKit 구문·Storybook 빌드 통과. UI 커버리지 네 지표 모두 100%다
  (`ui-check.log`). 권한 미결정·거부 때 초안과 안내를 보존하고 명령을 반복하지 않는 시험을 포함한다.
- 권한 분류 arm을 제거하면 controller 회귀가 `AudioDeviceDisconnected` 대
  `AudioPermissionPending`으로 실패했다(`permission-before-fix.log`). desktop 응답에 기존
  HTTP 200 조건을 복원하면 pending 회귀가 `202 !== 200`으로 실패했다
  (`desktop-before-fix.log`). 확인용 변경은 복원했다.
- `cargo fmt --all --check`, UI 변경 파일의 Biome, `git diff --check` 통과.
  Cargo는 기존 sccache daemon의 worktree `.d` 파일 쓰기 거부를 피해 `RUSTC_WRAPPER=`로
  실행했다. Rust 검사는 모두 `scripts/lab/native-check.mjs cargo ...` 경로를 사용했다.
- 문서는 `docs` 목적 설정으로 전체 교정했다. product 보존 검사는 통과했고, validation 보존
  검사는 새 Round 5 절을 구조 변경으로 판정해 heading-count 오류를 냈다. 이는 요청받은
  조사 기록 추가이며 기존 본문이 정확히 보존됐음을 별도 바이트 대조로 확인했다. 추가 수치·
  경로·확신 수준을 원시 증거와 다시 대조했고 새 산문의 따옴표 정규화는 변경이 없었다.

설치본의 실제 TCC 상태 조회, AX 포커스·Shift+F10,
새 원격 호스트 sample, AUDIO-01·AUDIO-11의 최종 오디오 수락은 새 후보에서 확인해야 한다.
hosted 화면 녹화는 직접 확인했으나 수정 후 실제 사용처·Storybook 프리뷰 화면은 확인하지 못했다.
CUA는 Firefox 창을 찾지 못했고 Chrome 접근이 허용되지 않았다. headless Chromium도
`bootstrap_check_in ... Permission denied (1100)`으로 시작하지 못했다.
커밋·push·workflow dispatch는 수행하지 않았다.
