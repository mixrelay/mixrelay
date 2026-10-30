# Stream Deck Marketplace 제출 키트

v0.1.0 플러그인의 제출 자료와 소유자 작업 절차다. 제품 동작과 미완료 검증의 기준은
[제품 문서](product.md)다. 이 작업에서는 계정 생성·약관 동의·업로드·제출을 실행하지 않는다.

## 등록 정보

| 필드 | 값 |
| --- | --- |
| 제품명 | MixRelay |
| 조직·Author | rawsector |
| 유형 | Stream Deck plugin |
| 카테고리 제안 | Audio (Maker Console의 실제 선택지 확인) |
| 플러그인 내 Category | MixRelay |
| 태그 제안 | Audio, Volume, Mute, Recording, macOS |
| 지원 URL | https://rawsector.com/products/mixrelay/manual |
| 지원 메일 | support@rawsector.com |
| 가격 제안 | 무료 플러그인. MixRelay 무료 기능으로도 사용 가능 |
| 플랫폼 | macOS 14.2+, Stream Deck 7.1+ |
| UUID | com.rawsector.mixrelay |
| 동작 UUID | com.rawsector.mixrelay.power / .mute / .volume / .recording |
| 제품·매니페스트 버전 | 0.1.0 / 0.1.0.0 |
| 업로드 파일 | MixRelay-StreamDeck-0.1.0.streamDeckPlugin |

DRM의 최소 기준은 SDKVersion 3·Stream Deck 6.9다. 현재 SDK `@elgato/streamdeck` 3.0.4는
공식 [3.x 업그레이드 절차](https://docs.elgato.com/streamdeck/sdk/releases/upgrading/v3/)에 따라
Stream Deck 7.1과 Node.js 24를 요구하므로 실제 요구사항은 7.1로 맞췄다.
플러그인 코드는 실행 중 매니페스트 메타데이터를 읽지 않는다. SDK 자체의 보호 매니페스트
처리는 공식 SDK에 맡기며, 읽을 수 없는 매니페스트 fixture로 실행 회귀를 검사한다.
Maker Console이 처리한 DRM 배포본의 실제 설치 시험은 소유자가 별도로 수행한다.

## 영문 설명

짧은 설명:

> Control MixRelay audio nodes from Stream Deck: power, mute, volume and recording on macOS.

긴 설명 (250–1,500자 범위, 첫 250자는 서식 없는 본문으로 입력):

> Control your MixRelay audio setup from Stream Deck on macOS. Assign Power, Mute, Volume and Recording actions to individual graph nodes. Keep confirmed node status on your keys while MixRelay handles audio processing in the background, even with the editor window closed.
>
> Use Volume keys to increase or decrease gain in steps of 0.5, 1, 2 or 3 dB. On Stream Deck +, turn a dial to adjust volume and press it to mute a supported node. Recording actions start or resume, pause and stop a recorder using the folder and format already selected in MixRelay.
>
> Choose a graph and a supported node in the property inspector. Settings save as you change them. Failed actions show an alert; disconnected commands are never queued for later replay.
>
> Requires macOS 14.2 or later, Stream Deck 7.1 or later, and MixRelay running. The plug-in is free and works with MixRelay’s free edition within its limits. Audio control stays on your Mac through a private local Unix socket. No account or external network connection is required by the plug-in.

영문 개인정보 안내:

> MixRelay audio controls use only a private Unix socket on your Mac. The plug-in makes no external network requests, requires no account and sends no analytics. Stream Deck communicates with the plug-in through its own local WebSocket. Graph and node selections are stored by Stream Deck. Recording files and destinations are managed locally by MixRelay; audio is not sent to the plug-in or to a remote service.

이 안내는 플러그인의 처리 범위다. MixRelay 데스크톱의 선택적 라이선스 인증·업데이트 조회나
Elgato Marketplace의 계정·배포 처리를 플러그인의 통신으로 소개하지 않는다.

## 아이콘·갤러리 준비

[제품 미디어 규격](https://docs.elgato.com/guidelines/products/)과
[플러그인 아이콘 규격](https://docs.elgato.com/guidelines/stream-deck/plugins/)을 적용한다.
설치 패키지의 PNG 256/512px와 Marketplace 등록용 PNG 288px는 서로 다른 크기다.
`pnpm --filter @mixrelay/streamdeck-plugin run pack`은 브랜드 원본에서 설치 아이콘과
`apps/streamdeck-plugin/dist/marketplace/app-icon.png`(288×288)를 생성한다.
카테고리는 흰색·투명 28px SVG, 동작은 흰색·투명 20px SVG여서 별도 @2x 파일 없이 확대한다.
정적 키는 72px SVG, 동적 키는 144px SVG를 쓴다.

등록에는 앱 아이콘, 썸네일과 갤러리 최소 3개가 필요하다. 썸네일·갤러리는 PNG 1920×960으로
만들고 이미지 안의 문구는 영어로 쓴다. 실제 제품 화면을 캡처하며 장치·상태를 합성해
검증된 동작처럼 보이게 하지 않는다. 개인 경로·사용자 이름은 촬영용 세션으로 치환한다.

| 파일 제안 | 캡처 절차·보여 줄 내용 |
| --- | --- |
| thumbnail.png | 영어 MixRelay 그래프와 Stream Deck의 Power·Mute·Volume·Recording 키를 함께 배치한다. 승인 브랜드 심벌과 짧은 제품명만 덧붙인다. 실제 설치한 버전을 촬영한다. |
| gallery-01-controls.png | Stream Deck에서 Power·Mute 키를 각 노드에 할당한다. 키를 눌러 MixRelay의 같은 노드와 확인된 On/Muted 표시가 바뀐 화면을 캡처한다. |
| gallery-02-volume.png | Volume 속성창에 그래프·노드·0.5 dB 단위를 표시한다. 실제 Stream Deck + 다이얼을 돌린 뒤 확인된 gain과 터치 스트립을 촬영한다. 다이얼 누르기로 mute도 확인한다. |
| gallery-03-recording.png | 녹음기 속성창의 Record/Pause/Stop 선택과 읽기 전용 폴더·형식을 캡처한다. MixRelay에서 같은 take를 녹음→일시정지→재개→정지하며 상태를 확인한다. |

캡처 전에 영어 Stream Deck과 MixRelay, 비개인 테스트 이름·폴더를 준비한다. 원본을 고해상도로
보관한 뒤 1920×960 캔버스에 잘림·늘어남 없이 배치한다. 키의 글자와 속성창을 실제 배율에서도
읽을 수 있어야 한다. 캡처 파일은 아직 준비하지 않았으며 생성한 아이콘만 제공한다.

## 0.1.0 릴리스 노트

> First release of MixRelay for Stream Deck on macOS.
>
> Control node power and mute, adjust volume from keys or Stream Deck + dials, and start, resume, pause or stop recordings. Choose targets in the property inspector with settings saved on change. Recording uses MixRelay’s existing folder and format settings. Keys and dials show confirmed state and alert when an action fails.
>
> Requires macOS 14.2+, Stream Deck 7.1+ and MixRelay running. Includes English and Korean action labels. Prepared for Marketplace DRM with SDKVersion 3.

## 패키지·자격 검증

저장소 루트에서 서명·키체인 접근 없이 플러그인만 빌드하려면 다음을 실행한다.

```sh
pnpm --filter @mixrelay/streamdeck-plugin build
pnpm --filter @mixrelay/streamdeck-plugin test
pnpm --filter @mixrelay/streamdeck-plugin validate
pnpm --filter @mixrelay/streamdeck-plugin run pack
node scripts/streamdeck-artifact.mjs build --output /absolute/new-artifacts
```

마지막 명령은 실제 패키지를 다시 풀어 공식 CLI와 매니페스트를 검증하고 파일명·크기·SHA-256을
출력한다. 기존 파일은 덮어쓰지 않는다. 앱·네이티브 빌드나 키체인 준비가 필요하지 않다.

macOS 릴리스 도구는 같은 소스의 PKG 옆에 이 파일을 둔다. `candidate-e2e.yml`은 macOS 후보
릴리스에서 PKG·플러그인·`source-revision.txt`를 함께 받는다. 후보 생성 시 네 설치 파일에
다음 필수 항목을 더한다.

```text
--artifact macos-universal-streamdeck:MixRelay-StreamDeck-0.1.0.streamDeckPlugin
```

`mix-relay.candidate.json`에는 다섯 산출물의 크기·해시가 들어간다. 후보 준비 작업은 실제
패키지를 풀어 CLI 오류·경고 0, SDKVersion·호스트·Node.js·제품 버전·UUID를 확인하고
`STREAMDECK-01` 증거를 남긴다. 전체 gate는 이 보고서가 없거나 다른 파일·후보·실행에
해당하면 실패한다. 해시로 고정한 원시 증거도 공개 전 다시 확인한다.

앱이 읽는 `mix-relay.release.json`은 앱 설치 프로그램 선택용이다. Stream Deck의 별도
설치기를 여기에 넣으면 앱 설치기로 잘못 선택할 수 있으므로 제외한다. 후보 명세·공개 검증에는
플러그인이 계속 필수이며, 후보와 최종 GitHub 릴리스에 같은 다섯 파일을 첨부한다.
설치기·후보·자격 검증 명세와 공개 영수증도 기존 절차대로 첨부한다. 공개 검증 workflow는
플러그인도 다운로드해 후보 해시를 재확인한다. 파일을 다시 패키징하면 새 후보 검증이 필요하다.

## 소유자의 Maker Console 제출 절차

1. [Maker Console 시작 안내](https://docs.elgato.com/maker-console/getting-started/)를 따라
   소유자가 로그인하고 `rawsector` 조직을 선택하거나 생성한다. 표시 이름과 매니페스트
   Author가 일치하는지 확인한다. 제품명 MixRelay의 사용 가능 여부도 확인한다.
2. 조직 Settings에서 Maker Agreement를 읽고 권한 있는 소유자가 동의한다. 에이전트는 대신
   계정을 만들거나 약관에 동의하지 않는다. 무료 배포·Audio 카테고리와 태그를 실제 콘솔
   선택지에서 확정한다. AI가 코드·설명 준비를 도운 범위도 현재 가이드에 따라 신고한다.
3. [제품 제출 절차](https://docs.elgato.com/maker-console/submitting-products/)의
   Home → Create product → Stream Deck plugin에서 자격 검증한 동일 파일을 업로드한다.
   이름·영문 설명·요구사항·지원 URL·개인정보 안내·릴리스 노트와 미디어를 입력한다.
4. 처음에는 Publish after review를 선택하지 않는다. DRM 처리 뒤 Versions에서 보호된
   패키지를 내려받는다. [공식 DRM 시험 절차](https://docs.elgato.com/streamdeck/sdk/introduction/distribution/)에
   따라 실제 Stream Deck에 설치해 네 동작·다이얼·속성창 저장·연결 해제·실패 알림을 확인한다.
   로컬 파일은 DRM 호환 준비본이며 업로드 전에는 Elgato가 암호화한 배포본이 아니다.
5. 갤러리 캡처와 실제 설치 검증을 끝낸 뒤 검토를 요청한다. 수정 요청이 오면 해당 내용을
   반영한 새 패키지·후보를 검증한다. 승인 후 소유자가 게시하며, 실제 제품 URL을 확인해
   매뉴얼에 Marketplace 직접 링크를 추가한다. 현재 문구는 등록 후 설치가 가능하다고 안내한다.

## 확인 범위

자동 검사는 키·다이얼 실패 알림, 변경 즉시 설정 저장, 100ms 표시 제한, 아이콘 크기와
브랜드 경로, 보호 매니페스트 fixture, 실제 패키지의 공식 CLI 검증과 자격 검증 게이트를 다룬다.
터치 스트립은 사용자 정의 좌표 없이 공식 `$B1` 레이아웃을 사용한다. 각 동작에 툴팁이 있다.
브랜드 참조 이미지와 생성한 플러그인 PNG는 직접 확인했다. 실제 Stream Deck 앱·하드웨어,
속성창의 hover/focus·선택·스크롤, Storybook 미리보기와 DRM 처리본은 이번 환경에서
시각·설치 검증하지 못했다. 이 범위를 자동 검사 통과로 대신하지 않는다.
