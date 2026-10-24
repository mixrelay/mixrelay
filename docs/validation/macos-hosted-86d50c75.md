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
