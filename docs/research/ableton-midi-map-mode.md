# Ableton Live의 MIDI Map Mode

2026-10-30에 Live 12 공식 자료와 매뉴얼의 화면 이미지를 확인했다.

- 우측 상단 MIDI 스위치 또는 macOS Cmd+M·Windows Ctrl+M으로 모드를 전환한다. 지정 가능한 조작은 파란색으로 강조된다. 대상을 고르고 MIDI 입력을 보내면 바로 목록에 추가하며, 모드를 나간 뒤 제어가 적용된다.
- Mapping Browser는 채널·Note/Control·Path·Name·Min·Max를 표시한다. 연속 제어의 범위를 편집하거나 반전할 수 있고, 상대 인코더의 인코딩은 하단 Mode 선택기로 정한다. 선택한 매핑은 Delete(macOS)·Backspace(Windows)로 지운다.
- 같은 입력을 다른 대상에 차례로 지정하는 다중 대상 흐름을 MixRelay에 적용한다. 확인한 공식 절차는 대상별 반복 지정을 설명하지만 같은 입력 재사용·같은 대상 재지정의 세부 전이를 명시하지 않는다. 이 전이는 이번 제품 요구사항을 기준으로 한다.
- 공식 Concepts 문서는 지정이 모드 종료 후 적용되고 지정된 입력은 MIDI 트랙으로 전달되기 전에 걸러진다고 설명한다. 모든 MIDI 입력·악기 연주까지 모드 안에서 차단된다고 확대 해석하지 않는다.
- 매뉴얼 화면에서 작은 MIDI 버튼과 행 단위 선택, 입력·대상·범위의 정보 위계를 확인했다. 설치된 Live 앱의 직접 조작은 도구 접근 거부로 확인하지 못했다. 공식 Scene Selector 도움말의 화면에서는 지정된 조작 위에 짧은 음이름이 붙고 목록에는 채널 1·Note C3·Master·Scene Down이 표시된다. MixRelay도 조작 위에 CC 7·Note C3 같은 짧은 입력 레이블을 둔다.

## MixRelay 적용

편집기 헤더 오른쪽 끝의 단일 소형 MIDI 버튼·단축키(macOS Cmd+M, Linux Ctrl+M), 파란 지정 표시, 대상 선택 → 입력 → 즉시 저장, 입력 하나의 여러 대상 지정, 목록 선택·삭제와 모드 안에서 Mapping Browser가 왼쪽 브라우저 영역을 대체하는 배치를 채택한다. MixRelay에서는 왼쪽 그래프 목록을 매핑 패널로 바꾸고 종료하면 복원한다. Escape와 창 닫기도 모드를 끝내며 모든 MIDI 액션 실행을 차단한다. 같은 대상을 유지한 채 다른 입력을 움직이면 이번 선택에서 만든 매핑만 교체한다. 정확한 중복은 옵션을 유지한다.

MIDI 토글은 중립색 pressed 채움으로, 매핑 대상은 파란 테두리로 구분한다.
테마·일반 명령은 모드에서 비활성화한다. 목록은 하드웨어 입력별 그룹과 매핑 제목의 단일 선택 버튼을 사용한다.
목록은 매핑 모드 안에서만 제공한다. 경로는 그래프·노드의 표시 이름이며 삭제 뒤에도 마지막 이름을 보존한다. 볼륨은 기존 −60~+12 dB·0.5 dB 규격 안에서 Min/Max와 상대 인코더 단위를 제공한다. 선택·포커스·실행 상태는 구분한다.

Control Surface 자동 매핑, MIDI 출력 피드백, Key/Macro 매핑, 클립·씬·악기 연주는 제외한다. MixRelay의 범위는 오디오 노드와 녹음기의 입력 전용 원격 제어다.

## 출처

- [Live 12: MIDI and Key Remote Control, 34.1–34.2](https://www.ableton.com/en/manual/midi-and-key-remote-control/)
- [Making custom MIDI Mappings](https://help.ableton.com/hc/en-us/articles/360000038859-Making-custom-MIDI-Mappings)
- [Live Concepts: MIDI and Key Remote](https://www.ableton.com/en/manual/live-concepts/#midi-and-key-remote)
- [Live Keyboard Shortcuts: Key/MIDI Map Mode](https://www.ableton.com/en/manual/live-keyboard-shortcuts/#keymidi-map-mode-and-the-computer-midi-keyboard)

- [Controlling the scene selector with a virtual MIDI bus: 지정된 조작과 목록 화면](https://help.ableton.com/hc/en-us/articles/360000185640-Controlling-the-scene-selector-with-a-virtual-MIDI-bus)
