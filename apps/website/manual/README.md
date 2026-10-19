# Manual source and translation notes

The public manual is plain HTML. Start at [the language chooser](index.html),
[English](en/index.html) or [한국어](ko/index.html). No build step or JavaScript is required.
The site keeps its prerelease disclosure because downloads and sales are not public.
Product status remains in [docs/product.md](../../../docs/product.md).

## Pages

Each filename exists under both `en/` and `ko/`.

| Filename | Coverage |
| --- | --- |
| `index.html` | Manual landing page and chapter summaries |
| `installation.html` | Requirements, verification scope, macOS PKG, Gatekeeper, Linux packages |
| `first-launch.html` | Connection messages, capture and microphone permissions, first route |
| `workspace.html` | Graphs, nodes, channel mapping, keyboard alternatives, files, pause and exit |
| `devices.html` | MixRelay IO directions, virtual-device settings, PCM, rates and processing buffers |
| `app-capture.html` | App selection, 48 kHz requirement, missing sources and whole-system capture availability |
| `recording.html` | Recorder setup, RF64 WAV formats, pause, finishing files and free limits |
| `effects.html` | Built-in effects, bypass, presets and effect chains |
| `plugins.html` | Platform formats, discovery, channel layouts, native editors and recovery |
| `stream-deck.html` | macOS installation, targets, keys, dials and recorder settings |
| `licensing.html` | Free quotas, paid rights, price, buying, activation, 30-day offline access, deactivation and refunds |
| `updates.html` | Automatic/manual checks, truthful result states, manual installation and preserved data |
| `uninstall.html` | macOS removal app, Linux remover/package manager, platform-specific data retention |
| `troubleshooting.html` | Signal checks, recovery, saving failures and manual diagnostic export |
| `support.html` | Support contact, response target and information to include |

The old [guide.html](../guide.html) is a link page, not a second manual.
Its seven previous section IDs remain as links to the relevant Korean chapters.
The product home page now links directly to the Korean manual.

## Adding a language

Copy the complete `en/` tree into a directory named with the target BCP 47 tag.
Keep filenames and section IDs unchanged. Translate document titles, descriptions,
landmark labels, visible text and image alternatives. Set the document `lang`.
Use the app's localized labels for controls; keep technical identifiers unchanged.

Add the language to the chooser and every chapter's visible language navigation and
`rel="alternate"` links, including the existing languages. Each switch must open the
same filename. Navigation, styles and validation do not need restructuring.
Run `node apps/website/check-site.mjs` from the repository root. The checker discovers
language directories and requires matching chapter paths, section IDs and switch links.

## Translator glossary

These are current app labels or established manual terms. Context matters where
one English label has two Korean translations. Do not translate MixRelay IO,
Driver, PipeWire, WirePlumber, RF64, WAV, PCM, AU, AUv3, VST3, CLAP or LV2.

| English | Korean | Context |
| --- | --- | --- |
| User manual | 사용 안내서 | Website |
| Routing workspace | 라우팅 작업 영역 | App screen |
| Graphs / New graph / Graph name | 그래프 / 새 그래프 / 그래프 이름 | Graph list and creation |
| Create | 생성 | Graph creation |
| Add node | 노드 추가 | Canvas command |
| Source / Processor / Output | 소스 / 프로세서 / 출력 | Node menu |
| Running applications | 실행 중인 앱 | Source list |
| Options | 옵션 | Node settings; do not leave English in Korean instructions |
| Rename display name / Display name | 표시 이름 변경 / 표시 이름 | Node alias, not native device identity |
| MixRelay Virtual I/O | 가상 장치 | Virtual-device submenu label |
| Device → graph / Graph → device | 장치 → 그래프 / 그래프 → 장치 | Audio direction |
| New virtual device | 새 가상 장치 | Paid device creation |
| Device name | 장치 이름 | Virtual-device settings |
| Device name | 기기 이름 | License activation |
| Channels / Bit depth | 채널 수 / 비트 심도 | Device or recorder |
| Sample rate | 샘플레이트 | Device format |
| Processing buffer / Buffer delay | 처리 버퍼 / 버퍼 지연 | App footer |
| Connect channels… | 채널 연결… | Context menu; dialog title omits ellipsis |
| First output channel / First input channel | 첫 출력 채널 / 첫 입력 채널 | Channel dialog |
| Number of channels / Destination | 채널 수 / 대상 | Channel dialog |
| Preview / Connect / Cancel | 미리 보기 / 연결 / 취소 | Channel dialog |
| Disconnect channel | 채널 연결 해제 | Port menu |
| Fit graph | 그래프 전체 보기 | Canvas menu |
| Source gain | 입력 레벨 | Source node |
| Output gain | 출력 게인 | Output level |
| Mute / Unmute | 음소거 / 음소거 해제 | Processing continues |
| Bypass | 바이패스 | Dry signal passes |
| Pause / Resume / Stop | 일시정지 / 재개 / 정지 | Global or recorder; state the scope |
| Open MixRelay / Quit | MixRelay 열기 / 종료 | Background status menu |
| Recorder / Record | 녹음기 / 녹음 | Node and action |
| Recording folder / Choose folder… | 녹음 폴더 / 폴더 선택… | Recorder |
| WAV sample rate / Input rate | WAV 샘플레이트 / 입력과 동일 | Recorder format |
| Recorded time / Recording error | 녹음 시간 / 녹음 오류 | Recorder status |
| Preset / Custom / Effect chains | 프리셋 / 사용자 설정 / 효과 체인 | Built-in effects |
| Equalizer / Compressor / Limiter | 이퀄라이저 / 컴프레서 / 리미터 | Built-in effect names |
| Threshold | 스레숄드 | Compressor node; other built-in effect fields use 문턱값 |
| Release | 릴리스 | Compressor node and built-in effect fields |
| Add plug-in folder… / Rescan audio effects | 플러그인 폴더 추가… / 오디오 이펙트 다시 스캔 | Plug-in discovery |
| Channel layout / Open | 채널 구성 / 열기 | Plug-in settings and native editor |
| Power / Mute / Volume / Recording | 전원 / 음소거 / 볼륨 / 녹음 | Stream Deck actions |
| File / New Workspace / Open… | 파일 / 새 작업 공간 / 열기… | Workspace menu |
| Save / Save As… | 저장 / 다른 이름으로 저장… | Workspace menu |
| License / License key | 라이선스 / 라이선스 키 | Licensing dialog |
| Buy a license / Activate | 라이선스 구매 / 활성화 | Licensing actions |
| Check license online / Deactivate this device | 온라인으로 라이선스 확인 / 이 기기 비활성화 | Licensing actions |
| Check for updates / Check again | 업데이트 확인 / 다시 확인 | Update controls |
| Save Diagnostic Report… | 진단 리포트 저장… | Native status menu |
| Remove MixRelay Driver | MixRelay Driver 제거 | Current macOS remover button, despite full-product removal |

Use `effects.ts` for the remaining effect and preset labels. It deliberately differs
from the older `studioGraph` inventory, which includes planned capabilities.
Keep sentences direct and preserve conditions, counts, units and pause scopes.

## Evidence and publication review

The manual follows [product.md](../../../docs/product.md),
[playback.md](../../../docs/manual/playback.md), the platform packaging READMEs,
`en.ts`, `ko-KR.ts`, `effects.ts`, product components and their Storybook stories.
OS guidance is linked inline to Apple's official support pages.

Two product/code differences are reflected in the manual rather than filled in:

- Product policy reserves whole-system capture for paid use, but
  `routingNodeMenus.ts` offers no whole-system capture command. The manual states
  that it is unavailable in the current menu.
- The single-product contract uses MixRelay naming, but the macOS removal dialog
  in `native/macos/installer/src/bin/uninstaller/localization.rs` still says
  “Remove MixRelay Driver” and “Delete saved MixRelay Driver settings.” The manual
  uses those exact labels and explains that removal covers the complete product.

Removal retention also differs by platform. macOS `native/user_data.rs` deletes
only `drivers.json` when requested and retains paid authentication. The Linux
remover clears local sign-in after successful package removal and can optionally
delete saved graphs/settings. Direct Linux package-manager removal keeps user data.
These implementation details take precedence over a generic removal description.
The footer provides processing-buffer control, not a processing-rate selector;
application capture requires 48 kHz. The manual does not invent either control.

Screenshots are represented only by `TODO screenshot` HTML comments. Before public
publication, capture actual installed EN/KO screens for installation, permissions,
workspace/channel connections, device options, capture recovery, recording,
effects, plug-ins, Stream Deck, licensing, updates, removal and diagnostics.
Owner input is needed for official download and checkout URLs, production licensing
configuration, support-mail readiness, and final installed platform qualification.
These release decisions remain tracked in product.md.
