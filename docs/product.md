# MixRelay 제품과 구현 상태

무료 MixRelay Driver, 무료 체험 앱, 유료 앱의 역할과 구현 상태를 이 문서에서 함께 관리한다.
제품 정책은 요구사항이며, 구현 완료 여부는 아래 체크리스트로 구분한다.
지원 운영체제는 macOS·Linux다.

## 제품 구성

| 구성 | 이용 방식 | 역할 | 저장 |
| --- | --- | --- | --- |
| MixRelay Driver | 무료 | 기본 가상 장치 하나와 루프백·장치 설정 | 장치 이름·형식·기본 설정 저장 |
| MixRelay 무료 앱 | 기간 제한 없는 기능 제한 체험 | 기본 장치와 제한된 구성에서 그래프·처리 기능 체험 | 드라이버 설정은 저장, 그래프·매핑 저장과 불러오기는 제한 |
| MixRelay 유료 앱 | 구매 인증 | 다중 가상 장치 추가·제거와 전체 그래프·처리·외부 제어 기능 | 장치·그래프·매핑 저장과 재시작 복원 |

14일 체험은 폐지한다. 무료 앱의 그래프·노드·처리 채널 수와 녹음 제한 수치는 아직 확정하지 않았다.
무료 앱의 제한을 무료 드라이버의 채널 수나 기본 설정 저장에 적용하지 않는다.
체험을 위해 음질을 낮추거나 잡음·끊김·추가 지연을 넣지 않는다.

유료 인증은 온라인 확인 성공 후 최대 30일 동안 오프라인으로 사용할 수 있다.
재시작이나 실패한 확인 요청으로 기간을 늘리지 않는다. 만료·네트워크 오류로 재생 중인 오디오를 갑자기 끊지 않는다.
결제는 Lemon Squeezy를 사용하며 스토어·구매 설정은 아직 준비되지 않았다. 별도 라이선스 서버나 상점 비밀 키를 앱에 넣지 않는다.
무료 배포와 소스 공개는 별개이며 현재 소스 배포 정책은 변경하지 않는다.

### 이용 허가와 제품 편입

MixRelay Driver는 무료 사용을 허용하는 독점 소프트웨어다. 무료 배포가 소스 공개·수정·재배포 허가를 뜻하지 않는다.
개인·기업이 방송·녹음·업무의 도구로 사용하는 것은 일반 사용이며, 무료·유료 앱의 기능 구분을 따른다.
MixRelay 자체를 다른 제품·서비스의 구성요소로 제공하는 OEM·번들·재판매·화이트라벨·상용 편입은
사전 서면 허가와 별도 유료 계약이 필요하다. 문의는 `support@mixrelay.com`으로 받는다.
일반 앱 구매는 위 제품 편입·재배포 권리를 포함하지 않는다.
제3자 구성요소의 원래 라이선스와 고지는 유지하며, 해당 구성요소에 부여된 권리를 자체 약관으로 제한하지 않는다.
앱 원문은 [LICENSE.txt](../LICENSE.txt), 무료 Driver 원문은 [LICENSE-Driver.txt](../LICENSE-Driver.txt)다.
통합 설치에서도 적용 범위는 분리한다. 사업자는 `에코 비전 랩 (Echo Vision Lab)`이며,
사업자등록번호·주소·연락처를 계약서에 기재한다. 설치 약관에는 대표자 개인 이름을 기재하지 않는다.
유료 판매 화면의 법정 사업자 고지 요건은 판매 채널에서 별도로 충족한다.

### 필수 지원 언어

앱과 Driver는 영어, 일본어, 한국어, 중국어 간체, 중국어 번체, 프랑스어, 독일어,
스페인어, 포르투갈어, 이탈리아어, 러시아어를 모두 지원해야 한다.
지원 범위에는 전체 앱 UI·도움말·오류·접근성 문구, Driver 상태 메뉴·설정·대화상자,
앱과 Driver의 전체 약관, 설치 안내와 제거 안내가 포함된다. 일부 제목만 번역하거나 지원 언어를
영어로 대체한 상태를 완료로 보지 않는다. 제품 이름·기술 식별자·서버 진단 로그는 원래 표기를 유지한다.

OS 또는 브라우저의 선호 언어 목록에서 첫 번째 지원 언어를 사용하며, 목록에 지원 언어가 없을 때 영어를 사용한다.
중국어는 명시된 Hans/Hant 문자 체계를 우선한다. 문자 체계가 없으면 TW·HK·MO는 번체,
CN·SG 및 일반 zh는 간체로 해석한다. 지역 변형은 해당 지원 언어로 연결한다.

앱 약관과 Driver 약관은 각각 11개 언어의 전문을 제공한다. 약관은 언어에 따라 사용 허가·구매 조건·책임·
개인정보·법정 권리의 내용이 달라지면 안 된다. 임의의 영어 우선 조항은 추가하지 않는다.
정본 파일 및 Installer 리소스 대응은 [언어 목록](../packaging/localization/languages.json)에 정의한다.
macOS Installer는 `en.lproj`, `ja.lproj`, `ko.lproj`, `zh-Hans.lproj`, `zh-Hant.lproj`,
`fr.lproj`, `de.lproj`, `es.lproj`, `pt.lproj`, `it.lproj`, `ru.lproj`의 안내·약관과 네이티브 언어 선택 기능을 이용한다.
설치 약관의 동의 버튼과 확인창은 Installer가 제공한다.

번역 키·보간 변수·전체 계약 조항의 누락, 간체/번체 선택, 네이티브 메뉴와 안내의 언어 일치,
앱·제거 앱·PKG·Linux 패키지에 포함된 원문을 검사한다. 실제 화면에서 긴 프랑스어·독일어 문구,
CJK 글꼴·줄바꿈·접근성 레이블과 작은 창의 잘림도 확인해야 한다.
새 사용자 기능과 계약 개정은 지원 언어 전체에 함께 반영하며, 미번역 사용자 문구를 출시 조건에서 검사한다.

앱은 구매 메이저 버전·동시 3기기·30일 오프라인 인증을 구분하고,
기기 교체·서버 비활성화·제거·재설치의 의미를 명시한다. Driver는 일반 상업 오디오 사용이 무료이며
앱 구매·환불과 독립적이다. 계약 동의·사용자 작업 권리·업데이트·지원·적법 종료·책임·분쟁을 정리했다.
무료 체험이나 약관 동의만으로 법정 환불·청약철회권을 일괄 배제하지 않는다.

앱 원문·문의 링크와 패키지 리소스는 연결돼 있다. 원문 표시 검사도 개정 파일 전체와 비교한다.
macOS Driver 설치·제거에는 지원 11개 언어의 Driver 원문을 그대로 포함하고, Manager에는 앱 원문 11개도 포함한다.
영어 원문 표시와 패키지의 원문 바이트 일치 검사는 통과했다. 제거 앱도 같은 Driver 약관과 CHANGELOG를 포함한다.
개정 설치 PKG의 영어 원문 표시를 실제 Installer에서 확인했다. 이번 패키지의 설치 동의와
관리자 설치는 아직 완료하지 않았다. 웹사이트 연결 확인은 남아 있다.
2026-10-08 계약 조항을 제품 정책·인증 및 업데이트 코드·현행 법령과 대조하여 개정했다.
내부 검토 상태나 출시 체크리스트를 사용자 계약서에 넣지 않는다.
조항별 판단 근거와 구현·운영의 잔여 사항은 [법무 검토 기록](legal-review.md)에서 관리한다.
체험의 14일 만료 구현, 판매 설정, 개인정보 처리방침과 국내 이용자 고지는 별도 출시 요건이다.
Lemon Squeezy의 [거래상 판매자 역할](https://docs.lemonsqueezy.com/help/payments/merchant-of-record)과
[환불 처리](https://docs.lemonsqueezy.com/help/payments/refunds-chargebacks)는 제품 지원 책임과 구분한다.

인증은 키·활성화 이름·식별자, 업데이트는 요청·접속 정보를 처리한다. 실제 처리·보존기간·수령자·
국외 이전을 개인정보 고지에 명시해야 하며 [DSUB 웹사이트 방침](https://www.dsub.io/privacy)만으로 대신하지 않는다.
제3자 고지는 실제 출시 구성의 원래 라이선스와 필요한 소스 취득 안내를 포함한다.
MPL 적용 부분의 의무를 자체 소스 전체 공개와 혼동하지 않는다.

## MixRelay Driver

드라이버는 OS별로 한 번 설치한다. 무료 Driver는 기본 가상 장치 하나와 이름·채널·샘플레이트·PCM 설정을 제공한다.
여러 가상 장치의 추가·제거는 구매 인증된 MixRelay 앱에서만 제공한다. 체험판과 독립 Driver 메뉴에는 추가·제거를 제공하지 않는다.
유료 앱에서 만든 장치도 앱 종료 후 Driver가 유지한다. 장치나 채널 구성마다 드라이버를 다시 설치하지 않는다.
기본 장치 이름은 `MixRelay IO`, 사용자 장치 이름은 그대로 유지한다.
제품 표시 이름을 바꾸기 위해 번들·서비스·장치의 안정적인 ID를 바꾸지 않는다.

유료 MixRelay 앱을 완전히 종료해도 실행 중인 Driver가 관리하는 장치와 기본 playback → capture 루프백은 유지한다.
그래프 엔진·플러그인·녹음·앱별 캡처는 종료한다. 드라이버 소유자와 그래프 엔진의 수명을 분리하며,
종료된 그래프의 DSP나 worker를 드라이버 유지 목적으로 남기지 않는다.
창만 닫으면 트레이와 실행 중인 그래프 오디오를 유지한다.

드라이버 관리에서는 실제 설치 상태와 장치의 현재 이름·채널 수·샘플레이트·PCM 형식을 표시하고,
지원하는 값만 변경할 수 있어야 한다. 드라이버·장치 상태와 그래프 엔진 상태를 혼동하지 않는다.
독립 Driver 메뉴의 장치 설정은 샘플레이트(kHz)·비트 깊이·채널 수 세 가지다. 채널 수·PCM은 장치의 양방향에 함께 적용하며 실제 공통 지원값만 제공한다.
방향별 캡처·재생 설정과 이름·라우팅 편집은 MixRelay 앱에서 제공한다. 트레이에는 장치별 세 설정과 ‘업데이트 확인’·‘종료’만 제공한다. 앱 열기·새로고침·제거·업데이트 목록은 두지 않는다. 업데이트 확인 결과는 메뉴 안에서 짧게 표시하고, 다운로드가 있을 때만 링크를 제공한다. 단순 상태 알림 팝업은 띄우지 않는다.
업데이트 결과와 다운로드는 확인 후 표시하며 자동 설치하지 않는다. Driver 상태 메뉴의 ‘종료’는 Driver의 모든 장치와 기본 루프백을 중지한 뒤 관리 앱을 종료한다. 저장된 장치 이름·채널·형식 설정은 삭제하지 않으며, MixRelay Driver를 다시 실행하면 복원한다. 중지 상태도 저장하여 서비스 재시작만으로 장치가 부활하지 않게 한다. 중지 또는 상태 저장 실패 시 관리 앱을 유지하고 다시 종료할 수 있게 오류를 안내한다. 이 동작은 유료 MixRelay 앱 종료·창 닫기·관리 앱 강제 종료와 구분한다.
메뉴 상단은 `MixRelay Driver {설치 버전}`만 표시한다. 상태·오류를 제품명에 붙이지 않으며, 버전 확인 전에는 제품명만 표시한다.
macOS Driver 관리 앱은 실행·다시 열기 시 Dock에 나타나지 않고 상태 표시줄에만 남는다. 그래프 앱은 Dock에 표시하며, 창 닫기는 숨김으로 유지한다. 시스템의 정상 종료 요청은 명시적 메뉴 종료와 같은 엔진 종료 절차를 거친다.
Driver 채널 메뉴는 장치가 지원하는 2·4·8·16·32·64만 제공한다. 기타·범위 메뉴는 사용하지 않으며 백엔드의 1–64채널 계약은 유지한다.
macOS 설치는 기본 Installer의 약관 표시 → 계속 → 동의 흐름을 유지한다. 자체 동의창이나 중복 안내를 추가하지 않는다.
메뉴와 노드 Options는 같은 장치 제어 API를 사용하며 Driver 관리 모드는 그래프 엔진과 구매 인증을 시작하지 않는다.
단순화한 메뉴의 구현·설치본 검증은 아래에 남아 있다.
추가·제거는 네이티브 명령에서도 구매 인증된 그래프 앱으로 제한하며, 무료·독립 관리 모드의 거부 검사를 통과했다.
macOS Driver 관리 앱·독립 서비스·HAL·설치 도구의 Universal 2 빌드·서명 검사를 통과했고, 관리 앱에 최신 앱·Driver 약관을 포함했다. 새 드라이버 설치본의 오디오·수명 검증과는 구분한다.
변경 경계의 Rust 포맷·strict Clippy 검사를 통과했다.

### 채널과 형식

| OS | 가상 장치의 방향별 채널 | 현재 native 가상 형식 |
| --- | --- | --- |
| macOS | 1–64 | 44.1/48/88.2/96/176.4/192 kHz, Float32·PCM16·packed PCM24·PCM32. Universal 2 빌드·native fixture 확인 |
| Linux PipeWire | 1–64 | F32·PCM16·packed PCM24·PCM32. 실제 협상·pause/resume PCM 확인 |

이 표는 제품의 가상 장치 범위다. 물리 장치는 하드웨어가 제공하는 채널·형식 범위를 따른다.
공통 DSP·미터·플러그인의 64채널 처리는 유지한다. macOS는 arm64와 x86_64를 포함한 Universal 2,
Linux는 arm64와 x86_64를 대상으로 하며 아키텍처별 실제 실행 여부는 체크리스트에서 구분한다.

- 샘플레이트·비트 심도·채널 수는 장치가 실제 제공하는 조합만 선택한다. 독립 목록을 임의로 조합하지 않는다.
- 물리 장치의 hardware 형식, 가상 드라이버의 native 형식, OS가 변환하는 client 형식을 구분한다. 32-bit 정수와 Float도 구분한다.
- 내부 DSP는 32-bit Float를 사용하고 필요한 형식 변환은 실제 입출력 경계에서 수행한다. 녹음 파일의 비트 심도는 별도다.
- 가상 드라이버의 정수 PCM16·PCM24·PCM32 변환을 양방향으로 구현했다. PCM24는 packed 3바이트이며 Float32와 구분한다. 설치본 검증과 앱 선택의 잔여는 체크리스트에 표시한다.
- macOS 오디오 MIDI 설정과 MixRelay 설정은 같은 장치의 값을 읽고 변경한다. 외부 DAW·OS가 바꾼 값을 오래된 요청으로 덮어쓰지 않는다.
- 형식 변경은 I/O를 정지·배출하고 native 협상과 readback 후 게시한다. 실패 시 이전 형식과 사용자 설정을 보존하며, 복원 실패는 구체적인 오류로 표시한다.
- 장치는 nominal sample rate와 실제 clock을 소유한다. 같은 Hz가 같은 clock을 뜻하지 않으며 독립 clock 경계의 보정은 한 번만 수행한다.
- MixRelay processing buffer는 DSP 최대 slice다. native callback quantum·client buffer·장치 rate·공유 버퍼 용량과 구분하며 OS 전체 버퍼나 실제 지연으로 표시하지 않는다.

### 앱과 드라이버의 경계

앱의 장치 ID·방향·채널·레이트·PCM 형식은 공통 타입을 사용한다.
native 형식의 지원 목록과 현재 값은 같은 `nativeFormats` 구조로 전달한다.
CoreAudio·PipeWire의 차이는 OS 어댑터가 처리하며 앱에 별도 설정 프로토콜을 만들지 않는다.
앱 → 공통 제어 API → 공통 변경 트랜잭션 → OS 어댑터 경로를 유지한다.
장치 선택은 안정적인 ID로, 기능 선택은 OS명이 아닌 실제 capability로 처리한다.
native 전송 방식이 달라도 revision 확인·정지·readback·게시·실패 복원은 같은 의미를 따른다.
OS가 지원하지 않는 변경은 명시적으로 거부한다. 내부 F32 전송과 native PCM 형식은 구분한다.
PCM 선택과 장치 설정은 독립 Driver의 같은 제어 경로로 통합하며 OS별 API를 앱에 노출하지 않는다.
그래프 참조는 기존 Driver 장치를 안정적인 ID로 선택하며 장치를 자동 생성·제거하거나 설정을 덮어쓰지 않는다.

## 앱 기능과 사용 흐름

1. 그래프를 만들고 실행 중인 앱, 물리·가상 입력, 프로세서, 물리·가상 출력이나 레코더를 추가한다.
2. 포트를 케이블로 연결한다. 여러 그래프는 동시에 실행하며 화면의 그래프 선택은 실행 상태를 바꾸지 않는다.
3. 노드 전원은 처리를 제외한다. Off인 프로세서는 bypass하지 않고 신호를 차단한다. 음소거는 처리를 유지하면서 해당 경계의 출력만 끈다.
4. Options에서 지원하는 게인·채널·장치·플러그인 설정을 변경한다. Off·제거된 노드 제어는 정의된 응답으로 처리하며 다른 노드를 재로드하지 않는다.
5. 레코더에서 폴더·WAV 샘플레이트·비트 심도를 정하고 녹음·일시정지·재개·정지한다. 녹음 또는 일시정지 중에는 형식을 바꾸지 않는다.
6. 유료 앱은 `.mix` 작업 공간과 그래프·매핑을 저장·불러오기·재시작 복원한다. 무료 드라이버 설정 저장과는 별개다.

앱 출력 차단은 MixRelay가 소유한 전달 경로만 제어한다. 캡처 이전 신호를 보존하고 타 앱·시스템 기본 출력은 자동 변경하지 않는다.
외부에서 경로가 바뀌면 소유권을 해제한다.

UI·API·Stream Deck은 같은 그래프·노드 제어 경로를 사용한다.
Stream Deck의 전원·음소거·볼륨·녹음 동작에서 그래프와 노드를 선택하며,
볼륨 버튼은 방향과 단위로, 다이얼은 왼쪽 감소·오른쪽 증가로 제어한다. 녹음은 앱 레코더의 설정을 따른다.

### 공유 신호와 플러그인

같은 가상 장치의 OS playback과 그래프 출력은 한 번만 합쳐 같은 장치의 OS capture와 모든 그래프 입력에 제공한다.
장치 identity별 native 소유자는 하나이며 그래프별 routing·gain·layout은 독립적이다.
참조 하나를 제거해도 다른 참조의 PCM을 끊지 않는다. 드라이버가 관리하는 장치는 그래프의 마지막 참조를 제거해도 유지한다.
사용자가 만든 피드백 경로를 자동 차단하거나 수정하지 않으며 처리는 인과적이고 유한해야 한다.

AU/AUv3는 macOS, VST3·CLAP은 macOS·Linux, LV2는 Linux에서 기존 호스트 라이브러리와 공식 API로 연결한다.
형식·SDK를 자체 재구현하거나 모든 채널·버스 조합을 탐색하는 기능은 만들지 않는다.
다중 플러그인·필요한 다채널·sidechain·64→16 매핑은 지원하고 실제 플러그인이 거부한 형식은 적용하지 않는다.
플러그인 worker와 editor는 같은 instance를 사용하며 장애·제거 후 자원을 회수한다.

## 구현 경계

| 경계 | 책임 |
| --- | --- |
| GUI와 desktop | 그래프 편집·트레이·OS 인증 저장소·그래프 엔진 시작과 종료 |
| 드라이버와 native 소유자 | 장치 등록·형식·기본 루프백·장치 설정·PCM 전송 수명 |
| 그래프 엔진 | 실행 계획·DSP·앱 캡처·녹음·플러그인·제어 API |
| 공통 I/O와 OS 어댑터 | 공통 계약과 Core Audio·PipeWire의 실제 협상·clock·자원 |
| 설치 패키지 | 앱·드라이버·서비스의 설치·업데이트·제거 |

장치 등록·설정 소유자를 `mixrelay-driver-service`로 옮기고 그래프 엔진은 일반 OS 오디오 클라이언트로 연결하도록 일부 재작성 중이다.
macOS의 기본 루프백은 HAL callback, Linux는 독립 PipeWire 소유자가 처리한다. 두 OS의 장치 설정 API와 저장 의미는 같다.
장치 제거는 새 I/O를 막고 진행 중 콜백을 배출한 뒤 PCM·매핑·변환 버퍼를 해제한다. 늦은 클라이언트 종료 호출을 위한 객체 정보는 마지막 참조까지 유지한다. 장치 소실 알림 뒤 우리 콜백은 무음으로 처리하며, 타 앱의 장치 재선택은 해당 앱이 담당한다. Driver 상태 메뉴의 명시적 종료는 저장 설정을 보존한 채 native 장치를 해제하여 기본 루프백을 중지한다. OS에 설치된 HAL·서비스 실행 파일을 제거하거나 언로드하는 기능은 아니며, 제어 서비스는 재실행 요청을 받을 수 있도록 남는다. 실행 중인 Driver의 macOS HAL 루프백은 제어 서비스 재시작 중에도 유지하지만, 명시적으로 중지한 상태에서는 저장 설정을 자동으로 활성화하지 않는다.
macOS는 AudioServerPlugIn HAL과 XPC/shared memory, Linux는 PipeWire를 사용한다. 새 경계의 실제 PCM·설치 검증 전에는 완료로 취급하지 않는다.
플랫폼 ABI 외의 정책·수명은 Rust가 소유한다.

그래프 문서에 native handle을 저장하지 않는다. 변경 자원은 비실시간에서 준비하고 같은 revision의 블록 경계에서 교환한다.
실시간 경로는 무할당·무잠금·무로그·무파일 I/O·유한 실행이며 callback 배출 후 객체·mapping·thread를 회수한다.
Linux 기본 루프백은 같은 채널 수의 입력 PCM을 그대로 빌려 쓰며 추가 출력 scratch를 만들지 않는다. 64채널 장치당 8MiB를 줄였다. Debian ARM64 VM·동일 release·직렬 실행·10,000회×7구간 중앙값으로 128프레임 capture 선택/복사 단계는 2채널 204.139→0.846ns, 32채널 645.933→0.850ns, 64채널 861.847→0.850ns였다. 모든 조건에서 할당 0·PCM 비트 일치. 이 수치는 복사를 없앤 해당 단계만의 비용이며 전체 엔진 지연·현장 무결성 측정이 아니다. 채널 수가 다르면 기존 projection을 유지한다.
Linux 연속성 검사의 PCM 감시기는 정상 순서에서 재해독을 생략하되 모든 채널의 비트 검사를 유지한다. 같은 VM·release·128프레임·1,000회×7구간 중앙값은 32채널 12,157.668→4,902.409ns(59.7% 감소), 64채널 17,366.705→9,869.819ns(43.2% 감소)다. 누락·반복·전 채널 손상·실행 혼입·PCM 변환·순번 한계의 무할당 회귀 4개를 통과했다. 테스트 감시 비용 감소이지 제품 DSP 개선이나 끊김 해결은 아니다.
클록·실시간 처리 계약은 [Apple HAL](https://developer.apple.com/documentation/coreaudio/audioserverplugindriverinterface), Linux 기본 루프백의 스케줄링 비교는 실제 사용 버전의 [PipeWire 1.4.2 구현](https://github.com/PipeWire/pipewire/blob/1.4.2/src/modules/module-loopback.c)을 기준으로 한다. 공식 루프백은 F32P이므로 선택한 PCM16/24/32 형식 검증을 대체하지 않는다.
입력 부족은 정상 무음과 구분하고 부분 입력의 남은 영역은 지운다. 완료된 sink write를 뒤의 실패로 되돌리지 않는다.
공유 PCM은 단일 writer·명시적인 수명·epoch를 유지한다. 대체된 자체 구현은 호출부·테스트·빌드 경로와 함께 제거한다.

### Linux 1,024프레임 누락 조사

PipeWire 1.4.2의 1,024프레임 손실 원인은 미확정이며 upstream 결함으로 확정하지 않았다.
재현 조건은 Debian ARM64 VM·32채널 PCM24·48kHz·release이고 native quantum 1,024는 약 21.33ms다.
연속성 fixture의 요청 block/latency는 128이지만 IndexedWriter·Reader는 실제 `cycle.frames`를 처리한다.
이 fixture가 native 1,024를 8×128 DSP slice로 처리한다고 설명하지 않는다.

확인된 경계는 실행마다 다르며 아래 사건을 합치지 않는다. 첫 두 채널의 버퍼 시작 표식 추적이지
전체 PCM 보존 검증은 아니다. Queue 성공도 downstream 소비를 보장하지 않는다.

| 보존 추적 | 확인된 손실 경계 | 순번 |
| --- | --- | --- |
| `.lab/driver-durability/native-handoff.json` | Driver capture queue에는 있고 일반 capture dequeue에는 없음 | 472,064 누락; 471,040→473,088 |
| `.lab/driver-durability/callback-handoff.json` | ordinary writer queue에는 있고 Driver render dequeue부터 없음 | 102,400 누락; 101,376→103,424 |
| `.lab/driver-durability/trigger-handoff.json` | ordinary writer queue에는 있고 Driver render dequeue부터 없음 | 183,296 누락; 다음 184,320 |

Callback 추적에서 source queue 102,400은 성공했으나 Driver render는 native tick 107,520의 process 없이
106,496→108,544로 진행했다. Clock-report→콜백 진입 29.872339ms는 순수 wake 대기가 아니다.
콜백 본문은 15.209µs이고 다음 source queue는 0.586671ms 뒤였다.
최신 trigger 추적은 기록 14,140개·완성된 process 쌍 2,826개·trigger 쌍 1,414개이며
기록 누락/미완성/미추적/미회수 stream은 모두 0, trigger 반환은 모두 1(성공)이다.
Source queue는 182,272→183,296→184,320으로 연속이지만 Driver render에서 183,296이 빠졌다.
해당 writer의 clock-report→trigger는 1.404952ms, trigger→콜백 진입은 23.950451ms로
21.333ms quantum 예산을 넘었고 콜백 본문은 10.750µs였다. 손실 경계의 지연은 trigger 이후
dispatch/preemption 구간으로 좁혔지만 순수 kernel wake 지연·VM 결함으로 확정하지 않는다.
서로 다른 stream의 ticks는 절대 위치로 직접 정렬하지 않는다. `missing_gap_distance_sum`도 총 손실 프레임 수가 아니다.
Trigger 추적의 release native 실행은 scope 1에서 첫 1,024프레임 누락(183,296→184,320)으로 실패했고 scope 2는 실행하지 않았다.
Trace는 로컬 진단 자료로 보존하며 Git에 넣지 않는다.

공식 `pw-loopback` 1.4.2 대조도 같은 ordinary clients에서 479,232→480,256의 1,024프레임 손실을 보였다.
이는 F32P scheduling 대조이지 PCM24 형식 동등성·VM 원인·PipeWire 결함의 증명이 아니다.
[공식 모듈](https://github.com/PipeWire/pipewire/blob/1.4.2/src/modules/module-loopback.c#L353)은
밀린 capture buffer 중 최신 것만 전달하므로 무손실 대조군도 아니다.

**Upstream 상태:** WirePlumber 0.5.8의 `state-stream.lua:359` number/table 비교 실패는 기존 upstream #903과 일치하며 중복 보고하지 않는다.
SPA의 fixated `Choice.None`은 유효한 단일 값 표현이고([SPA iter.h](https://github.com/PipeWire/pipewire/blob/1.4.2/spa/include/spa/pod/iter.h#L356)),
WirePlumber의 Lua parser는 이를 table로 반환한다([공식 pod.lua 검사](https://github.com/PipeWire/wireplumber/blob/0.5.8/tests/wplua/scripts/pod.lua#L149)).
그런데 [state-stream.lua](https://github.com/PipeWire/wireplumber/blob/0.5.8/src/scripts/node/state-stream.lua#L349)는
`channels`를 그대로 수치 비교한다. 잘못된 생산자 Format이 아니라 외부 consumer의 unwrap 누락이다.
이미 [de0bca5 수정](https://github.com/PipeWire/wireplumber/commit/de0bca59023ef1cd655ebf5f944f8fd5952df10e)(2026-02-06)과
[Lua `elseif` 정정](https://github.com/PipeWire/wireplumber/commit/48ed27d11bfd2c952bc9a3138a4df6f81aa01038)이 반영됐다.
0.5.13 태그에는 이전 코드가 남아 있고 [0.5.14](https://github.com/PipeWire/wireplumber/blob/0.5.14/src/scripts/node/state-stream.lua#L348)는
number/table을 구분해 단일 값을 해제한다. `state.restore-props`를 자동으로 꺼서 우회하지 않는다.

**0.5.14 비교 결과:** 공식 태그 `07e730b`의 daemon·library·scripts를 VM의 프로젝트 경로에서 빌드했다.
PipeWire 1.4.2·32채널 PCM24·48kHz·release·observer·기존 정책 설정은 그대로 사용했다.
0.5.8의 Lua number/table 오류 2회는 0.5.14에서 0회였지만 연속성 검사는 여전히 실패했다.
Scope 1의 첫 누락은 196,608→197,632(1,024프레임)이며 scope 2는 실행하지 않았다.
`.lab/driver-durability/wp-fixed-handoff.json`에서 해당 표식은 Driver capture queue까지 있고
ordinary reader dequeue에는 없었다. 기록 14,104개에서 누락·미완성·미추적·미회수 stream은 모두 0이다.
따라서 0.5.14는 Lua 오류 수정이지 PCM 끊김 해결책이 아니다. 시스템 0.5.8은 교체하지 않았다.
Debian 13 저장소는 0.5.8, backports는 0.5.12이므로 최신 버전 의존성만 추가하면 설치를 막는다.
패키지 관리자 경로를 유지하고, 시스템 WirePlumber를 동봉본으로 덮어쓰거나 두 정책 관리자를 함께 실행하지 않는다.
PipeWire 손실은 추가 원인 분리 전까지 확정 upstream 버그로 보고하지 않는다.

**2026-10-08 Docker 소스/스케줄링 대조:** 기존 isolated runner를 Debian 13 ARM64 컨테이너에서 실행했다.
PipeWire 1.4.2-1·WirePlumber 0.5.8·48kHz·release이며, 컨테이너 소스 복사본의 native quantum/minimum만
1,024로 고정했다. 호스트 소스의 quantum 설정은 바꾸지 않았다. Docker Desktop의 Linux VM은
`7.0.14-linuxkit`, `CONFIG_PREEMPT_LAZY=y`, HZ=1,000이고 PREEMPT_RT 커널이 아니다.
호스트 오디오·장치·Docker socket을 연결하지 않은 별도 daemon 대조이며 설치 제품·물리 Linux의 무결성 판정이 아니다.
이미지·runner 호출·원본 로그·trace·커널 설정·분석기는 `.lab/docker-audio/`에 보존하고 Git에 넣지 않는다.

| 대조 조건 | 결과와 확인된 경계 |
| --- | --- |
| 32채널 PCM24, observer 없이 → observer → observer 없이 | 앞뒤 실행은 두 scope 모두 480,255 indexed frames를 정상 수신. observer 실행은 79,872→80,896 누락; capture queue에는 있고 reader dequeue에는 없음 |
| 64채널 PCM16, observer 없이 | scope 1 실패, 첫 누락 100,352→101,376; wrong sample 2,048도 관측. 계측 없이도 실패하므로 observer만의 문제로 확정할 수 없음 |
| 64채널 PCM16, 기존 observer | 184,320→185,344 누락; capture queue에는 있고 reader dequeue에는 없음. 모든 콜백 본문 ≤2.1805ms |
| 32채널 PCM24, callback CPU 시각 추가 | scope 1 실패, 첫 누락 139,264→140,288. reader 버퍼 시작 표식의 불연속은 17곳; 감시기의 누적 gap distance로 이 횟수를 계산하지 않음 |
| 64채널 PCM16, 서버 FIFO 88 / 클라이언트 FIFO 83 확인 | scope 1 실패, 109,568→110,592 누락. writer queue에는 있고 render dequeue부터 없음; 모든 콜백 본문 ≤2.449ms |

첫 Docker PCM24 실패의 capture trigger→진입은 42µs로 빨랐지만 콜백 벽시각이 25.604833ms였다.
79,872 queue 직후 0.641416ms 만에 다음 80,896이 queue됐고 reader는 후자만 받았다.
반면 기존 64채널 PCM16 실패의 render 진입은 해당 stream의 clock-report 뒤 21.059478ms,
capture queue는 22.449978ms였다. 그 report 주기의 reader 콜백은 없고 다음 주기로 넘어갔다.
이 report 차이를 순수 kernel wake 대기나 서로 다른 stream의 절대 tick 위치로 해석하지 않는다.

기본 컨테이너는 RTPRIO/NICE 한도 0이며 서버와 클라이언트 모두 SCHED_OTHER였다.
RTPRIO=88만 허용한 중간 대조는 클라이언트만 FIFO 83이고 서버는 여전히 SCHED_OTHER였으므로
전체 RT 대조로 취급하지 않는다. [module-rt.c](https://github.com/PipeWire/pipewire/blob/1.4.2/src/modules/module-rt.c#L1138)는
요청한 nice=-11 조정이 실패하면 RTKit 경로로 바꾸고, 이 격리 환경에는 RTKit이 없어 module 초기화가 실패했다.
RTPRIO=88와 NICE 한도 31을 함께 허용한 별도 컨테이너에서 서버 FIFO 88·nice=-11,
클라이언트 FIFO 83을 실행 중 `ps`로 확인했지만 손실은 남았다. 이 실행에서 writer의
clock-report→trigger는 28.163096ms, trigger→진입은 97.583µs였다. 늦은 구간은 trigger 이전에도 있다.
Docker VM이 대조 사이 재시작됐고, 재시작 뒤 일반 스케줄링 64채널 실행도 두 scope를 통과했다.
따라서 통과 실행 하나를 RT 개선의 인과 증거로 쓰지 않으며 단순한 RT 권한 부족만으로 손실을 설명하지 않는다.

**공개 소스에서 확인한 계약:** [Linux 7.0.14 eventfd](https://github.com/gregkh/linux/blob/v7.0.14/fs/eventfd.c#L249)는
write 횟수를 lock 안에서 누적하고 읽을 때 반환한다. 알림을 읽는 즉시 해당 thread를 실행한다는 계약은 없다.
[epoll](https://github.com/gregkh/linux/blob/v7.0.14/fs/eventpoll.c#L1267)은 ready list와 waitqueue를 깨우고
[스케줄러](https://github.com/gregkh/linux/blob/v7.0.14/kernel/sched/core.c#L4092)가 실행을 배정한다.
일반 fair thread의 [wakeup preemption](https://github.com/gregkh/linux/blob/v7.0.14/kernel/sched/fair.c#L8803)은
우선순위/eligibility를 판단하며 오디오의 21.33ms 마감 시간을 보장하지 않는다.
PipeWire의 [trigger_target_v1](https://github.com/PipeWire/pipewire/blob/1.4.2/src/pipewire/private.h#L659)은
activation CAS와 eventfd write 성공을 반환한다. downstream PCM 소비의 완료 응답이 아니다.
[node_on_fd_events](https://github.com/PipeWire/pipewire/blob/1.4.2/src/pipewire/impl-node.c#L1547)는
합쳐진 알림 `cmd > 1`을 xrun으로 기록하고 process를 한 번 호출한다.
[다음 driver 주기](https://github.com/PipeWire/pipewire/blob/1.4.2/src/pipewire/impl-node.c#L2135)는 미완료 follower의
activation을 초기화하고 이전 TRIGGERED/AWAKE를 xrun으로 다룬다.
[SPA IO](https://github.com/PipeWire/pipewire/blob/1.4.2/spa/include/spa/node/io.h#L75)는 현재 buffer id/status의 교환이고
async도 두 cycle slot이므로 늦은 모든 주기의 PCM을 다시 재생하는 무손실 메시지 큐가 아니다.
더 구체적으로 [tee_process](https://github.com/PipeWire/pipewire/blob/1.4.2/src/pipewire/impl-port.c#L262)는
`*mix->io[cycle] = *io`로 downstream의 현재 IO descriptor를 대입하고 producer IO를 NEED_DATA로 바꾼다.
이 경로는 이전 descriptor의 HAVE_DATA 소비 완료를 검사하고 FIFO에 추가하는 방식이 아니다.
따라서 downstream의 한 주기 처리가 건너뛰어진 뒤 다음 output tee가 실행되면 이전 buffer id를
교체할 수 있는 구체적 소스 경로가 있다. 실패 실행의 정확한 port/descriptor 교체 여부는 이 내부 경계의 추가 trace로 확인해야 한다.
관측은 deadline 초과와 주기 복구 중 손실에 부합하지만, 특정 buffer를 교체한 내부 명령이나 kernel 결함을 확정한 trace는 아직 없다.

콜백 경계에 `CLOCK_THREAD_CPUTIME_ID`를 추가했다. 같은 native thread ID의 진입/종료만 짝지으며
실패한 시각 조회는 0 대신 null로 남긴다. 32채널 실패의 최대 capture 벽시각 46.6895ms와
guest CPU 46.687499ms가 거의 같았지만 이를 순수 PCM 계산 시간으로 판정하지 않는다.
실제 `/proc/config.gz`는 PARAVIRT_TIME_ACCOUNTING과 IRQ_TIME_ACCOUNTING이 모두 꺼져 있다.
[update_rq_clock_task](https://github.com/gregkh/linux/blob/v7.0.14/kernel/sched/core.c#L786)의 steal/IRQ 차감은
해당 설정에 의존하므로 이 guest CPU 시각만으로 host vCPU 중단·IRQ·실제 계산을 분리할 수 없다.
안정 커널 태그의 위 소스 분석은 LinuxKit의 모든 패치가 바이너리와 동일함을 증명하지 않는다.

**2026-10-08 PipeWire 소스 수정·실행 대조:** 공식 1.4.2 소스를 원본과 수정본 모두 같은
Meson release/O3·GCC·Ninja 설정으로 컴파일했다. 별도 prefix를 쓰고 native 실행의 실제 loaded mapping과
라이브러리 SHA-256을 보존했다. 배포판 라이브러리·호스트 설치본·MixRelay PCM FIFO 용량은 바꾸지 않았다.
기존 isolated native fixture의 bit-exact 판정과 두 client scope를 그대로 사용했다.
Docker Linux ARM64·48kHz·native quantum 1,024이며 서버 FIFO 88·클라이언트 FIFO 83 환경이다.

소비자 dispatch 지연을 통제하려고 ordinary reader의 100번째 eventfd wake에서 읽기 직전 한 번 50ms를
지연시켰다. 원본과 수정본에 같은 진단 훅을 적용하고 각 reader 종료 때 실제 지연 시각을 출력했다.
두 번째 scope는 새 reader와 반전된 indexed 신호를 쓰므로 지연도 별도로 발생한다. 이 훅은 최종 패치에 없다.

| 소스 대조 | 원본 | 최종 수정본 |
| --- | --- | --- |
| 64채널 PCM16, reader 지연·observer 있음 | 실제 지연 50.829ms, 첫 누락 99,328→101,376: 2,048프레임 | 실제 지연 53.232/54.964ms, 두 scope 각 480,255프레임·누락/반복/오샘플/부분 입력 0 |
| 32채널 PCM24, reader 지연·observer 없음 | 실제 지연 55.065ms, 첫 누락 98,304→100,352: 2,048프레임 | 실제 지연 53.863/53.100ms, 두 scope 각 480,255프레임·누락/반복/오샘플/부분 입력 0 |
| 32채널 PCM24 및 64채널 PCM16, 지연 훅·observer 제거 | 위 통제 실행과 별도 조건 | 각 형식 두 scope 각 480,255프레임·누락/반복/오샘플/부분 입력 0 |

수정은 `private.h`, `impl-port.c`, `impl-node.c` 세 파일이다. 수동 trigger 생산자의 동기 출력 링크에
유효한 HAVE_DATA 버퍼가 남아 있으면 다음 input/process/output tee를 미루고 기존 버퍼를 유지한다.
activation 완료와 downstream 깨우기는 계속 수행하므로 소비자는 기존 버퍼를 받을 수 있다.
driving 노드·자동 capture/monitor의 입력 처리·async cycle slot은 기존 정책을 유지한다.
실험 중 모든 non-driving 노드에 적용한 첫 수정은 FIFO 쓰기 거절과 scope 2 손실을 만들어 폐기했다.
초기 통과 한 번이나 필터된 PipeWire 경고 로그를 수정의 실행 증거로 사용하지 않는다.

최종 지연 대조에서 client capture 생산자가 실제 5주기(64채널)/4주기(32채널)를 미뤘으며 다른 노드는 0이다.
이 수는 client 종료 진단 `output.log`에서 확인한다. 서버 proxy 노드의 `pipewire.log`와 구분한다.
두 수정 대조와 훅 제거 실행 모두 PCM FIFO 쓰기 거절·consumer discard는 0이다.
시작 시 한 번 발생하는 read starvation은 별도로 남으며 전체 read 거절 0을 주장하지 않는다.
64채널 observer 기록 19,254개는 기록 누락·미완성 0, 두 신호 phase의 버퍼 시작 표식 불연속 0이다.
모든 채널의 PCM 검증은 observer 표식 대신 기존 native assertion이 수행했다.
최종 훅 제거 빌드의 PipeWire 검사 47개가 통과했다.

검토할 패치는 [PipeWire 1.4.2 보존 패치](../native/linux/pipewire/tests/patches/pipewire-1.4.2-retain-manual-pending.patch)로
버전 관리에 보존하며 자동 배포 적용은 하지 않는다. 전체 수정 소스와 적용 설명은
`.lab/docker-audio/patches/manual-pending-source/` 및 `README.md`에 있다. 원본 archive, 빌드 옵션·로그·core 바이너리는
`.lab/docker-audio/source/`와 `evidence/pw-build/`, 대조 로그는 `evidence/pw-control-reader-delay50-stats/`,
`pw-control-32-reader-delay50/`, `pw-manual-pending-reader-delay50/`, `pw-manual-pending-32-delay50/`,
`pw-manual-clean-32-final/`, `pw-manual-clean-64-final/`와 연결된 native evidence에 보존했다.
`evidence/source-experiment-summary.json`은 위 여섯 실행의 PCM·transfer 결과, 실제 지연·보존 횟수,
loaded mapping 확인 및 보존 core 바이너리의 해시 일치를 묶는다.
통제한 consumer dispatch 지연의 손실 경로는 수정으로 막았지만 기존의 모든 자연 발생 손실이나
kernel 결함을 해결·확정한 결과는 아니다. 오래 지연된 PCM 보존은 latency를 늘릴 수 있으며
fanout·feedback·지속 과부하·물리 Linux·설치 제품은 이 패치로 아직 검증하지 않았다.

**2026-10-08 kernel trace·upstream 제출 검토:** Docker Linux VM의 별도 tracefs instance에서
`sched_waking`, `sched_wakeup`, `sched_switch`, fork/exec, IRQ/softIRQ를 guest `mono` 시각으로 수집했다.
trace 컨테이너에만 SYS_ADMIN/SYS_PTRACE/PERFMON을 허용했고 제품/native 테스트는 기존 UID 10001과
FIFO 우선순위를 유지했다. 전역 tracing 설정·커널 이미지·호스트 오디오를 변경하지 않았다.
64채널 PCM16 원본은 통과했고, 별도 32채널 PCM24 원본은 자연 발생 393,216→394,240 누락을 재현했다.
후자는 reader 지연 주입이나 SIGSTOP을 넣지 않은 실행이다. observer 기록 14,114개와 각 CPU trace에
기록 누락·overrun·commit overrun·dropped event는 0이다.

첫 손실의 reader kernel TID 15813/native TID 571, 대상 CPU 8을 같은 시각 축에 연결했다.
`sched_waking`은 2662.092657s, IPI IRQ 진입은 2662.099905s, `sched_wakeup`은 2662.099910s,
첫 switch-in은 2662.099915s였다. 깨우기 요청→runnable은 **7.253ms**, runnable→실행은 **5µs**다.
그 사이 다음 driver 주기가 2662.098678s에 이미 시작했다. 첫 switch-in 구간에는 native process callback이
없고 다음 reader callback/dequeue가 2662.102903709/2662.102911001s에 손실 뒤 표식을 받았다.
이 trace만으로 내부 activation CAS가 실패한 정확한 명령까지 관측한 것으로 쓰지는 않는다.

CPU 8은 앞서 idle로 전환됐고 마지막 trace 2662.090234s부터 위 IPI까지 9.671ms 동안 기록이 없었다.
해당 구간에 다른 guest 태스크가 그 CPU를 점유하거나 길게 열린 IRQ handler는 관측되지 않았다.
Linux의 [remote wake-list 경로](https://github.com/gregkh/linux/blob/v7.0.14/kernel/sched/core.c#L3787)는
대상 CPU가 IPI를 받아 task를 enqueue한 뒤 runnable로 바꾼다. 따라서 이번 손실의 큰 지연은
FIFO runnable 태스크의 일반 runqueue 대기보다 **remote CPU의 wake/IPI 처리 이전**에 있다.
vCPU의 host 실행/virtual interrupt 전달 지연에 부합하지만 guest idle/interrupt-disabled 구간과
host vCPU 중단은 guest trace만으로 구분할 수 없으므로 Linux scheduler 결함으로 단정하지 않는다.
컨테이너 `cpu.max=max 100000`, CPU throttling 0, memory high/max/OOM 0을 확인했다.
VM/호스트 CPU는 모두 12개이고 Docker 4.94.0이다. 이 개수만으로 host CPU 포화가 증명되지는 않는다.
원본 trace·stats·시각 대조·분석기는 `.lab/docker-audio/evidence/kernel-review/`와
`.lab/docker-audio/analyze-sched.py`에 보존했다. guest CPU accounting의 기존 한계도 유지한다.

같은 장치 피드백의 기존 native 검사를 source-built 원본/수정본으로 대조했다.

| 순서/소스 | 32채널 Float32 | 64채널 Float32 |
| --- | --- | --- |
| 1 원본 | fresh 48,128·wrong 0 통과 | fresh 48,128·wrong 0 통과 |
| 2 훅 없는 보존 패치 | fresh 48,128·wrong 0 통과 | fresh 37,888·wrong 12,288 실패 |
| 3 보존 패치+종료 카운터, 지연 주입 0 | fresh 48,128·wrong 0 통과 | fresh 48,128·wrong 0 통과, capture 보존 1주기 |
| 4 원본 | fresh 48,128·wrong 0 통과 | fresh 48,128·wrong 0 통과 |

이 검사는 PCM24/PCM16의 indexed 연속성 검사와 다른 causal feedback 판정이며 기준을 낮추지 않았다.
기존에도 피드백의 간헐 실패가 있었고 수정본 반복은 통과했으므로 패치 유발 회귀라고 확정하지 않는다.
하지만 수정본 실패가 남아 있어 **regression 없음으로 승인할 근거도 없다**. 실행은
`pw-original-feedback-review`, `pw-pending-feedback-review`, `pw-pending-feedback-repeat`,
`pw-original-feedback-repeat`와 연결된 native evidence에 보존했다.

최신 공식 mirror의 master `b37700cb2083d68477cef9002d1f676dff796f3c`와 이력을 확인했다.
이미 [2025-10-14 upstream 변경](https://github.com/PipeWire/pipewire/commit/20d2a331be58a68abead4b99a95bf783c9797033)에
`node.reliable`/`port.reliable` opt-in과 `tee_process_reliable`, stream dequeue의 미소비 버퍼 검사,
driver→async 입력 링크의 IO 순서 처리가 있다. key의 도입 버전은 1.6.0이다. 1.4.2에는 없다.
이는 현재 실험 패치와 동일하지 않다. reliable tee는 사용 가능한 첫 링크 하나에 전달하고 멈추므로
다중 reader multicast를 보장하지 않으며 audio fanout에 그대로 켜는 해결책으로 권고하지 않는다.
현재 확인은 소스 검토이며 최신 reliable 빌드의 MixRelay PCM 통과 결과로 쓰지 않는다.
공식 제출 경로는 [GitLab merge request/issue](https://github.com/PipeWire/pipewire/blob/b37700cb2083d68477cef9002d1f676dff796f3c/README.md#contributing)다.

실험 패치는 3파일·35줄 추가로 작지만 중앙 scheduler에 영향을 준다. `node.trigger`는 추가 trigger 의존성이지
reliable 전달 요청이 아니다. 임의의 수동 trigger 생산자에 보존 정책을 강제하고, 어느 동기 output 링크 하나가
미소비 상태이면 생산자의 모든 input/process/output을 미루므로 다른 빠른 reader나 async 출력도 함께 늦을 수 있다.
Mixed sync/async·fanout·drain 처리의 회귀 검증도 없다. 따라서 이 패치를 그대로 새 upstream MR로 제출하지 않는다.
기존 reliable 기능의 audio 적용 가능성/한계를 먼저 재현하고, 남는 결함이 있으면 해당 경계의 opt-in 수정과
공개 PipeWire만으로 실행되는 최소 회귀 검사를 준비한다. private MixRelay 소스·이미지·원시 lab 데이터는 제출하지 않는다.

**2026-10-08 MixRelay API 사용 수정과 reliable 실행 대조:** `MRPipeWire.c`의 출력 콜백은
`spa_chunk.size` 바이트 수만 설정하고 `pw_buffer.size` 프레임 수를 설정하지 않았다.
[공개 stream 계약](https://github.com/PipeWire/pipewire/blob/1.4.2/src/pipewire/stream.h#L264)에 맞게
실제 출력 프레임 수(무음 포함)를 기록하고, 반환할 재사용 출력 버퍼는 먼저 0으로 초기화하도록 수정했다.
FIFO·클럭·trigger·PipeWire 소스 정책은 바꾸지 않았다. 새 C 회귀 검사는 실제 PCM ring과 제품 콜백을 사용하며
원본 코드에서 프레임 수 assertion으로 실패하고 수정본에서 F32·PCM16·packed PCM24·PCM32를 통과했다.
이 검사는 버퍼 용량 상한·무음·잘못된 형식/geometry·반환 횟수를 함께 확인한다.

공식 1.6.9 소스를 같은 격리 컨테이너에서 수정 없이 빌드하고 기존 native PCM 검사로 대조했다.
원본 MixRelay의 64채널 PCM16은 두 scope 각 480,255프레임·연속성 오류 0이었다.
반면 수동 출력에 `node.reliable=true`를 추가한 실험은 1,024→2,048에서 1,024프레임 누락으로 실패했다.
위 `pw_buffer.size` 수정까지 적용한 reliable 실험도 같은 첫 누락으로 실패했다.
따라서 reliable은 제품 코드에 적용하지 않았으며 이 오디오 경로의 해결책으로 제시하지 않는다.
개발 브랜치 1.7.0에서는 reliable 설정 유무 모두 PCM 검사 전에 capture 형식 협상에 실패했으므로
해당 실행은 reliable 전달 효과를 판정하는 근거가 아니다. audio fanout 보장도 아직 확인되지 않았다.

프레임 수 수정본을 원본 PipeWire 1.4.2로 실행한 64채널 PCM16은 두 scope 각 480,255프레임·연속성 오류 0이었다.
지연 훅 없이 실행한 32채널 PCM24는 첫 scope를 통과했지만 두 번째 scope에서
439,296→441,344의 2,048프레임 누락으로 실패했다. 기존 비결정적 손실도 남아 있다.
그러나 같은 수정본에 기존 reader 지연 진단 훅을 사용한 32채널 PCM24는 실제 52.722333ms 지연 뒤
98,304→100,352에서 2,048프레임 누락으로 실패했다. 프레임 수 수정은 확인된 API 메타데이터 문제를
해결하지만 앞서 관측한 지연 손실을 해결하지 않는다. 일반 실행의 통과를 지연 손실 해결로 해석하지 않는다.
"PipeWire 수정은 필요 없고 MixRelay의 API 오용만 고치면 해결된다"는 결론은 실행 근거로 뒷받침되지 않는다.
동일 장치 feedback 검사는 프레임 수 수정본 첫 실행에서 32채널 fresh=0으로 실패했다.
동일 조건의 수정 전 원본은 32채널을 통과했지만 64채널 fresh=32,768로 목표 48,000 미달이었다.
수정본 재실행은 32·64채널 모두 fresh=48,128·wrong=0으로 통과했다. 세 실행의 불안정한 결과로
프레임 수 변경의 회귀를 확정하거나 전체 회귀가 없다고 주장하지 않는다. 실패 판정 조건은 완화하지 않았다.
새 원본 로그·loaded mapping·해시·진단 소스는 `.lab/docker-audio/evidence/pw-*`와
`buffer-metadata/`에 보존한다. upstream 패치는 계속 실험 자료이며 제품에 적용하거나 PR을 보내지 않았다.

**2026-10-08 시작 전 버퍼 준비:** native PCM ring은 이미 두 quantum 분량의 prefill 후 읽으며,
부족해지면 무음과 재충전 상태로 돌아간다. 초기 prefill이 없는 구현은 아니었다.
다만 익명 mmap PCM 페이지와 Linux Driver의 큰 zeroed scratch는 할당만 하고 첫 접근을 RT에 남겨 두었다.
공유 ring 생성과 Driver 준비 단계에서 모든 페이지를 제어 스레드에서 미리 접근하도록 변경했다.
샘플 비트·FIFO 커서·용량·두 quantum prefill·클록·SRC는 그대로다. 메모리 준비는 callback 공개 전에 끝나며
페이지의 이후 상주를 영구 보장하거나 `mlock` 권한을 요구하지 않는다.
Loopback의 `prepare_inputs`도 기존 render/input dependency 준비 후 capture/output 형식 협상을 확인한 뒤
DSP를 공개하도록 변경했다. 대기는 제어 스레드에서 최대 3초이며, 미협상 상태는 시작 실패로 반환한다.

macOS ARM64 ring 검사 17개, Docker Linux ARM64 ring 검사 18개, Linux Driver unit 검사 11개와
변경 경계 strict Clippy·Rust fmt·C 버퍼 메타데이터 검사를 통과했다.
공유 ring 생성 변경의 macOS 호출부도 확인했고 macOS transport unit 검사 176개를 통과했다(별도 native 검사 1개 제외).
Linux 전체 mapping residency 검사는 기존 지연 할당 경로에서 실패하고 사전 준비 경로에서 통과했다.
32·64채널 네 가지 PCM 형식의 실제 협상/설정 교체/제거 검사도 통과했다. 이 빠른 native 실행은
1.19초에 끝나 별도 2초 후 scheduler sampler가 `/proc`에서 이미 종료된 PID를 읽는 데 실패했다.
native 검사 exit=0과 별도 monitor 실패를 구분하며, 해당 짧은 실행의 loaded mapping은 확인했다고 쓰지 않는다.

반면 변경본의 일반 32채널 PCM24 실행은 첫 scope의 100,352→101,376에서 1,024프레임 누락,
wrong=2,048로 실패했고 64채널 PCM16은 두 scope 각 480,255프레임·연속성 오류 0이었다.
서버/클라이언트 전체를 같은 guest CPU 2에 제한한 진단에서도 32채널은 첫 scope를 통과했지만
둘째 scope의 269,312→270,336에서 1,024프레임 누락으로 실패했다. native thread affinity와
서버 FIFO 88·클라이언트 FIFO 83을 실행 중 확인했다. affinity 제한은 격리 진단에만 사용하고 제품에 적용하지 않았다.
따라서 시작 전 메모리 준비와 형식 확인은 보완했지만, 초기 준비 부족이나 CPU 간 이동만으로
실행 중 손실을 설명하지 않는다. PCM 안정화/무손실 해결 완료로 표시하지 않는다.
새 원본 자료는 `.lab/docker-audio/evidence/pw-startup-*`와 `startup-preparation/`에 보존한다.

**2026-10-08 별도 Proxmox VM 대조:** 기존 호스트에 임시 Debian 13 x86_64 VM을 만들었다.
2 vCPU·RAM 4GiB·디스크 32GiB이며, guest 커널은 `6.12.111+deb13-amd64`,
Debian PipeWire `1.4.2-1`·WirePlumber `0.5.8-2`다. 격리 daemon의 native quantum 1,024와
DSP 128은 그대로이며 실행 중 서버 FIFO 88·클라이언트 FIFO 83을 확인했다.
CPU affinity나 커널 설정을 추가로 바꾸지 않았다. upstream 보존 패치와 reliable 설정도 적용하지 않았다.

현재 수정본은 32채널 PCM24·64채널 PCM16을 각각 3회 실행해 모두 통과했다.
각 실행의 두 독립 client scope는 각각 fresh=480,255·누락·반복·wrong·short=0이었다.
PipeWire 기본 `pw-loopback`의 32채널 F32P도 두 scope에서 같은 판정을 통과했다.
32·64채널 네 PCM 형식의 협상/설정 교체/제거, ring unit 18개·Driver unit 11개와
C buffer 메타데이터 검사도 통과했다. 빠른 형식 검사는 0.28초에 끝났으므로
native exit=0과 실행 중 scheduler snapshot 부재를 구분한다.

수정 전 HEAD `a2d2160`은 별도의 빈 target에서 다시 빌드하여 32채널 PCM24·64채널 PCM16을
각 1회 실행했고 모두 두 scope 각 480,255프레임·연속성 오류 0이었다.
수정본/원본 native test 바이너리 SHA-256은 각각 `1520621348c76d3aafe982d543f939fdc96fc11eb2445c639a9cd451efa93066` /
`4bcb6a7766588a4a255a377d055774588408c4384f5b665beadd778725095d07`이다.
처음 shared Cargo target을 사용한 원본 비교는 archive의 오래된 mtime 때문에 현재 바이너리가
재사용되어 제외했다. 해당 두 실행은 제외 이유와 함께 보존했으며 원본 통과 횟수에 넣지 않는다.

이 유한 실행에서 누락이 재현되지 않았다는 환경 대조이며, startup 수정의 해결 효과나 전체 안정화 증명이 아니다.
Docker ARM64/LinuxKit와 별도 x86_64/KVM은 CPU·아키텍처·guest 커널·hypervisor·PipeWire 빌드가
함께 달라졌다. 특정 커널 결함으로 원인을 확정하지 않는다. 이 VM의 50ms reader 지연 복구·동일 장치
feedback·장시간 운용은 아직 검증하지 않았다. source archive/manifest·VM 구성·원본 로그·loaded mapping·
스레드 우선순위·native artifacts·비교 요약은 `.lab/remote-audio/`에 보존한다.

**2026-10-08 최신 Linux Driver 패키지 검증:** 같은 Debian 13 VM의 정상 사용자 PipeWire
세션에서 현재 수정본을 표준 `pnpm desktop:build --driver`로 빌드해 x86_64 DEB/RPM을 생성했다.
DEB payload·ELF·문서·서비스/hook 검사를 통과하고 실제 APT 설치·사용자 서비스 Ready를 확인했다.
설치된 서비스의 SHA-256은 빌드한 실제 sidecar와 일치한다. 2채널 Float32와 64채널 Float32·PCM16·
PCM24·PCM32는 각각 독립 client scope 두 구간에서 fresh=480,255·누락·반복·wrong·short=0이었다.
이 수치는 측정 구간의 PCM 판정이며 시작/종료 시 전체 transfer counter가 모두 0이라는 뜻은 아니다.
당시 정책의 실제 D-Bus Quit 이벤트 후 관리자는 종료되고 같은 서비스 PID·장치 Ready가 유지됐다. 이 과거 검증은 이후 변경된 명시적 Driver 종료 동작의 검증으로 사용하지 않는다.
관리자의 트레이 아이콘 시각 표시는 이번 최소 X11 세션에서 확인하지 못했다.

같은 0.1.0 패키지 재설치 후 장치 ID·이름·64채널 PCM32와 사용자/권한은 보존됐다.
서비스 재기동의 새 native owner 게시로 revision 8→9가 되어 파일 해시는 달라졌지만 설정 값은 동일하다.
이는 같은 버전 교체 검증이며 서로 다른 릴리스 버전 사이의 업그레이드 검증으로 표시하지 않는다.
정상 Polkit 제거·선택 삭제 결과는 아래 Driver 독립 수명 항목에 기록한다.
당시 RPM은 빌드·payload 추출·실행 파일·hook·의존성 검사까지만 확인했다. 이후 실제 검증은 아래 2026-10-10 기록을 따른다.
실제 관리 프로그램의 glibc 2.39 심볼 요구에 맞춰 Driver DEB/RPM의 최소 의존성을 2.39로 보완했다.
패키지 검사 33개 통과와 수정 후 DEB/RPM 재빌드·실제 의존성 확인을 완료했다.
최종 DEB의 관리자 메뉴에서 64채널·PCM32 선택을 실제 D-Bus 이벤트로 실행해 native readback을
확인했고 당시 Quit 동작 뒤 같은 서비스 PID를 유지한 PCM 재검증 두 구간도 각각 fresh=480,255·연속성 오류 0이었다. 명시적 Driver 종료로 장치·루프백을 중지하는 새 동작은 별도 검증한다.
macOS는 Notion의 기존 서명 백업을 복원하여 최신 서비스·HAL·관리 앱·설치/제거 도구의
ARM64/Intel 통합 release 빌드와 실제 Developer ID Application 서명 검증을 완료했다.
설치 payload의 아키텍처·게시자·launchd 설정 검증을 통과했고 Apple 공증은 Accepted였다
(제출 ID `85c92976-8fa5-4580-9215-18afd6df2e3e`, issues 없음, ticket 18개).
관리 앱과 독립 제거 앱은 staple/validate 및 Gatekeeper `Notarized Developer ID` 판정을 통과했다.
기존 Installer 개인 키를 찾지 못해 사용자 지시에 따라 새 Developer ID Installer 인증서
`G27855PS32`를 발급했다. 새 개인 키와 일치함을 확인하고 사용자 지시에 따라 암호 없는
인증서·개인 키 PEM·중간 인증서·복원 설명서 ZIP을 Notion 서명 자료 페이지와 저장소 밖 개발 볼륨에
각각 하나씩 보관했다. 중복 첨부·암호화 P12·임시 사본을 정리했다. Notion에서 내려받은 키와 인증서를
별도 키체인에 복원해 유효 signing identity와 실제 PKG 서명을 확인했으며 최종 ZIP의 인증 자료와
바이트가 일치했다(`macos-notion-backup-verification.json`).
새 인증서로 실제 `MixRelay-Driver-Installer-0.1.0.pkg`를 생성했고 게시자·정확한 인증서 지문·timestamp를
확인했다. PKG 공증 `0e8a8628-706d-4538-b097-3a79b0442a44`는 Accepted였다(issues 없음, ticket 21개).
PKG와 독립 제거 앱의 staple/validate 및 Gatekeeper `Notarized Developer ID` 검증을 통과했다.
최신 PKG의 실제 설치와 설치 후 runtime 검증은 아직 수행하지 않았다.
이전 Installer 인증서 `FWXT2Q4U59`는 폐기 대상으로 지정하고 임시 키체인에서 공개 인증서를 제거했다.
Developer ID는 계정에서 직접 revoke할 수 없어 Apple 보안팀에 보낼 폐기 요청 메일 초안을 준비했다.
사용자 지시에 따라 메일은 발송하지 않으며 Apple 측 폐기는 미완료다. 앱 서명 인증서와 새 Installer 인증서는 폐기 대상이 아니다.
과거 다른 설치본의 성공을 최신 수정본의 설치 성공으로 대체하지 않는다. 당시 Arch 패키지는 없었다.
원본 로그·source manifest·설치 산출물은 `.lab/driver-release-20261008/`에 보존한다.

**2026-10-10 Linux Driver 배포 패키지 검증:** 현재 0.1.0 x86_64 산출물은
Debian 13의 DEB, Fedora 44의 RPM, Arch rolling의 pacman 패키지다. 각 KVM VM에서
실제 설치·일반 사용자 서비스 Ready·관리자 메뉴 종료/재실행·실행 중 관리자에 대한 제거 차단·
같은 버전 교체·설정과 명시적 종료 상태 보존·제거 후 프로세스/API/노드 해제·재설치를 확인했다.
revision은 새 native owner 게시에 따라 증가할 수 있으며 장치 설정과 파일 소유자/권한을 보존했다.
최종 설치 서비스의 바이트는 세 패키지에서 일치하고 root 소유 0755로 일반 사용자가 실행한다.

Fedora와 Arch의 설치 서비스는 2채널 Float32 및 64채널 Float32·PCM16·PCM24·PCM32를
각각 독립 client scope 두 구간에서 검사했다. 각 구간은 fresh=480,255, 누락·반복·wrong·short=0이다.
이는 유한 구간의 PCM 판정이며 시작/종료 transfer counter 전체가 0이라는 의미는 아니다.
Fedora의 최종 수명 검사는 동일 RPM 해시로 먼저 통과한 PCM 결과를 유지하고 실행했다.
Debian의 기존 PCM 실측은 위의 별도 기록을 유지하며 이번에 다시 실행했다고 표시하지 않는다.
최소 X11 세션의 시험용 StatusNotifier 호스트에 실제 관리자 메뉴의 D-Bus 이벤트를 전달했다.
일반 데스크톱 패널의 트레이 아이콘 시각 배치는 이번 검증 범위에 포함하지 않았다.

RPM의 glibc 버전 표현식이 bundler에서 하나의 의존성 이름으로 저장되는 문제는 실제 제공되는
`libc.so.6(GLIBC_2.39)(64bit)` 요구로 수정했다. 빌드 umask로 sidecar가 0700이 되어 사용자
서비스 실행이 실패하는 문제는 패키징 전 0755 설정으로 수정했다. 의존성 우회 없이 DNF 설치와
서비스 실행을 확인했다. Arch는 검사한 DEB 공통 payload를 SHA-256으로 고정한 PKGBUILD로
일반 사용자 `makepkg`에서 빌드하며, 패키지 전용 PreTransaction/AbortOnFail hook을 제공한다.
제거 도구는 pacman의 고정 패키지 제거 명령을 지원한다.

Fedora와 Arch의 실제 Zenity 제거 창에서 설정 삭제를 선택하고 Polkit을 통한 정상 제거 뒤
정확한 Driver 설정 파일과 종료 마커 삭제를 확인했다. 시험 VM의 관리자 계정에만 고정 패키지
관리 프로그램을 허용하는 임시 Polkit 규칙을 사용했으며 검사 뒤 제거했다. 테스트 설정은
백업에서 복원하고 최종 패키지를 재설치해 Ready를 확인했다. 사용자 인증 창의 암호 입력 자체를
검증했다는 뜻은 아니다. 기존 Debian의 승인/취소 검사는 아래 별도 기록을 유지한다.

패키징·번역·빌드 경계 회귀 검사 69개가 통과했다. 배포 파일과 Arch 빌드 recipe는
`artifacts/linux/`, 실제 패키지 해시·빌드 소스 해시·PCM 원본 로그·수명/제거 결과는
`.lab/linux-release-20261010/`에 보관한다. 직접 다운로드용 파일 준비까지 완료했으며
공개 게시·저장소 서명·AUR 등록은 수행하지 않았다. 서로 다른 버전 사이의 업그레이드,
ARM64 설치 실행, 모든 파생 배포판과 실제 장비의 장시간 운용까지 검증한 것으로 확대하지 않는다.

**Known issue — 일부 실행 환경에서 재생 중 PCM 프레임 누락:** 현재 재현 자료는
Docker Desktop ARM64/LinuxKit 등의 가상화·스케줄링 지연 조건이며, 48kHz에서
1,024~2,048프레임(약 21~43ms)의 간헐 누락을 확인했다. 원인은 미확정이고 해결 완료로 표시하지 않는다.
별도 Debian 13 x86_64/KVM 대조에서는 수정 전·후 모두 위의 유한 연속성 검사를 통과했다.
Arch Linux의 유한 PCM 검사는 위의 2026-10-10 기록에서 확인했다. 실제 사용자 장비의 장시간 운용은 아직 검증하지 않았다.
사용자 결정에 따라 현재는 환경과 재현 조건을 명시한 known issue로 추적하며,
이 결과만으로 Linux 전체의 출시 차단으로 분류하지 않는다. Debian 기반 전체나 Arch 전체의
무결성을 보장한다는 뜻은 아니다. 지원 대상의 실제 환경에서 정상 사용 중 재현되면
출시 판단을 다시 검토한다. 기존 PCM 검사와 실패 판정은 완화하지 않는다.

**재개 지점:** 정상 callback 메타데이터·시작 전 버퍼 준비와 지연 시 전달 보장을 분리한다. 기존 reliable 설정만으로는 해결되지 않았으므로
RAW follower·audio adapter·downstream IO의 미소비 경계를 대조하고, 다중 reader/feedback을 보존하는
최소 전달 정책 변경이 필요한지 판단한다. 50ms 지연 PCM 검사를 통과하기 전에는 손실을 해결했다고 표시하지 않는다.
실험 보존 패치는 regression 미확인으로 제출/배포 대기이며 indexed 지연 대조의 통과 결과는 유지한다.
지연의 더 낮은 원인은 host vCPU/virtual IRQ trace 또는 물리 Linux 대조로 구분한다. 확정한 경계만 수정·재검증하며
임의 FIFO 확대·Clock 재작성·판정 완화·새 runner·무가설 반복 검사를 하지 않는다.
최신 CPU 시각 계측의 포맷·strict Clippy·Linux release 검사 9개·경고 금지 문서 빌드는 통과했지만
제품 연속성 전체는 검증 대기다. 일반 MixRelay 빌드에는 lab 훅이 없고 제품 오디오 동작·FIFO 용량도 바꾸지 않았다.
기존 isolated lab의 project-managed `.lab/mixrelay-integration-current` 소스 루트에서 정상 사용자로 실행한다.

```sh
CARGO_BUILD_JOBS=1 MIXRELAY_LAB_TEST_NAME=native_driver_32_64_pcm_continues_after_graph_client_exit MIXRELAY_LAB_TEST_PROFILE=release MIXRELAY_LAB_BUFFER_OBSERVER=1 MIXRELAY_LAB_DRIVER_CHANNELS=32 MIXRELAY_LAB_DRIVER_FORMAT=s24 bash native/linux/pipewire/tests/run-application-retracking.sh "$PWD"
```

## 설치와 판매

웹사이트에서 직접 배포한다. 사용자가 SDK·개발 도구·별도 런타임을 설치하거나 시스템 서비스를 직접 설정하게 하지 않는다.
드라이버 설치는 한 번이면 되며 기본 무료 기능을 이용하는 데 구매 인증을 요구하지 않는다.
현재 드라이버 등록에는 설치가 필요하므로 완전 무설치형으로 광고하지 않는다.

- 업데이트는 장치·그래프·설정·녹음·인증을 보존한다. 현재 경로는 새 설치 패키지를 실행하는 업데이트이며 자동 업데이트 완료를 주장하지 않는다.
- 앱은 시작 시·24시간마다 업데이트 서버를 조회하며 수동 확인도 제공한다. 앱과 Driver의 설치·게시 버전을 구분한다. 미게시·실패를 최신으로 표시하지 않고 다운로드를 누르면 현재 목록을 다시 확인한 뒤 링크만 연다. 설치·오디오 중단은 자동 실행하지 않는다.
- 앱 제거는 로컬 유료 인증과 복구 저널을 자동 삭제한다. 기본 미선택인 체크박스로 앱의 그래프·캐시 삭제를 선택한다. 외부 프로젝트·녹음 파일은 보존한다.
- Driver 제거는 Driver 장치·루프백을 내린다. 별도 기본 미선택 체크박스로 Driver 설정 삭제를 선택하며 앱의 구매 인증·그래프·녹음은 건드리지 않는다. 앱 제거만으로 Driver를 제거하지 않는다.
- macOS 설치·업데이트용 `MixRelay Driver` PKG는 Driver와 전용 제거 앱 `/Applications/Uninstall MixRelay Driver.app`을 함께 설치한다. 설치 화면에 제거 선택을 넣지 않는다. 제거 앱에서 기본 미선택인 Driver 설정 삭제 여부를 선택하고 제거를 실행하며 약관 절차는 없다. PKG가 설치한 on-demand 도우미는 제거 전용 관리자 인증과 동일 게시자의 서명된 앱 요청을 확인한 후 고정 제거 작업만 수행한다. 일반 앱의 광범위한 관리자 실행 권한·Installer 플러그인·별도 제거 PKG·트레이 제거 메뉴를 사용하지 않는다. 인증 취소 전에는 Driver 상태를 변경하지 않고, 성공 응답·실제 파일 부재·도우미 서비스 정리까지 확인한다. 새 구조의 실제 설치 및 제거 검증은 사용자 테스트가 완료된 뒤 기록한다.
- Linux 설치·업데이트·제거는 APT·DNF 패키지 관리 경로를 유지한다.
- 제거·업데이트 실패나 재부팅 뒤에도 재실행·재제거할 수 있어야 한다. 실패·취소한 제거는 인증을 지우지 않는다.
- macOS는 Developer ID 서명·PKG 공증을 확인한다.
- 모든 다운로드 패키지는 `CHANGELOG.md`와 해당 제품 약관을 포함한다. 앱은 `LICENSE.txt`, Driver는 `LICENSE-Driver.txt`, 통합 배포는 두 약관을 모두 포함한다. 릴리즈 페이지 첨부만으로 대체하지 않으며 제3자 고지도 유지한다.
- R2 설치 파일은 제품별 `x.y`마다 최신 패치 `z`만 보관한다. 새 패치 게시·검증이 성공한 뒤 이전 패치 파일을 삭제하고 다운로드 목록에서도 제외한다. 다른 `x.y`와 GitHub 릴리즈·태그·CHANGELOG는 보존한다.

순서는 드라이버 → 앱의 무료·유료 정책과 UI → 설치 앱 검증·매뉴얼·사이트 → 스토어·실결제 → 보안·최종 배포다.
App Store 전환·AudioDriverKit 재작성·BPM·MIDI·새 JACK/ALSA 백엔드는 이번 범위가 아니다.
다른 배포판·앱·플러그인·클록 조합의 전수 검사나 특정 상용 장비 확보를 새 출시 조건으로 추가하지 않는다.

플랫폼별 빌드·설치 명령은 [macOS](../packaging/macos/README.md), [Linux](../packaging/linux/README.md),
[개발 lab](../infra/lab/README.md)에 둔다.
[성능 기록](performance/audio-engine-render-baseline.md)은 계측 조건과 수치만 보존하며 별도 작업 목록을 만들지 않는다.

## 구현 상태

완료 19개 · 검증 대기 16개 · 미구현 1개.
완료는 실제 검증 통과, 검증 대기는 구현 완료, 미구현은 누락 또는 미해결 실패다.
독립 Driver 재작성으로 바뀌는 장치 참조·입출력·Linux 원래 출력 차단은 검증 대기로 옮겼다.
기존 결과는 유지하되 새 산출물의 통과 결과로 사용하지 않는다. 무관한 기능 검증은 반복하지 않는다.

드라이버의 형식·수명·입출력·복구부터 처리한다. 상태가 바뀌면 이 체크리스트와 개수를 바로 갱신한다.
유효한 기존 검증은 반복하지 않고 실패한 경계만 재검증한다.
단위 테스트·빌드·native fixture·VM·설치 앱·물리 장치 결과는 구분하며 다른 아키텍처의 완료로 바꾸어 표시하지 않는다.
연속성은 시간·위치가 구분되는 PCM으로 확인한다. 진단 계측·수동 처리·sleep pacing을 정상 실시간 성능으로 표시하지 않는다.
성능 개선은 동일 조건의 변경 전후 수치를 기록하고, 측정할 수 없으면 이유와 대체 근거를 적는다.

### 완료

- [x] 그래프 생성·전환·이름 변경·저장
- [x] 노드·케이블 추가·삭제·재연결
- [x] 다중 선택·일괄 연결·그래프 이동
- [x] Undo/Redo와 레벨 변경 묶음 처리
- [x] 입력 게인 제어
- [x] 1–64채널 미터
- [x] 포트 표시·채널 툴팁·Options 배치
- [x] 창·헤더·버튼 정리
- [x] 노드 추가 메뉴
- [x] 앱·장치 목록 갱신과 재시도
- [x] 앱·장치 이름·아이콘·안정적인 선택
- [x] 플러그인 파라미터 전달
- [x] 플러그인 지연 보상
- [x] 플러그인 장애 격리·복구
- [x] 빈 그래프·서버 재시작 상태 처리
- [x] 노드 오류·끊긴 미터 상태 표시
- [x] OS별 단축키
- [x] 오디오 스케줄링: Linux VM native 1,024/DSP 128프레임에서 480,000프레임 누락·반복·오샘플 0개, 출력 부족 0회. native 128프레임 강제 실행의 DSP 이전 손실 해결이나 동일 조건 성능 개선을 뜻하지 않는다.
- [x] macOS 설치 활성화: SIP를 유지하고 Mac 재부팅 없이 실제 PKG 설치 성공. HAL 장치 인식·서비스 Ready/버전/PID 일치 확인. 2채널 48kHz 기본 루프백은 비교 192,512개 모두 일치·최대 오차 0. 정상 설치의 임시 폴더는 정리됐다.

### 검증 대기

- [ ] 원래 출력 음소거: macOS 기존 통과 유지. Linux native 장치 소유자가 Driver로 바뀌므로 PID·장치·스트림 소유 검증과 경로 차단·원복 재확인.
- [ ] 장치 참조 수명: 그래프 참조는 Driver 장치를 생성·제거하지 않는다. 설치본의 참조 삭제·그래프 종료·재시작 뒤 장치와 PCM 유지 확인.
  - [ ] 사용 중 제거 안전성:
    - [x] macOS HAL release 검사 67개: 활성 클라이언트 제거·콜백 배출·오디오 자원 즉시 회수·늦은 StopIO·100회 재생성. client I/O 검사 45개: 제거 알림 전후·렌더 도중 무음과 할당 0. 포맷·strict Clippy·문서·독립 리뷰·Universal 2 HAL 빌드/서명/factory 검사 통과. 수정 HAL은 아직 설치하지 않았다.
    - [x] Linux native 제거: 열린 입출력 클라이언트를 유지한 장치 제거·서비스 해제·재생성 4회 통과. 콜백 종료·native owner 회수·초기 캐시 준비 후 FD·스레드 증가 없음.
    - [x] macOS 앱 캡처 자원 회수: 128개 캡처 노드 순차 생성·종료, 활성/pause 64개 보존, stale lease·epoch 무효화·복사 후 종료 무음, 실제 FD 고갈 실패·reader 배출 후 매핑/FD 회수 통과. transport 10개·registry 12개 release 검사, 포맷·strict Clippy·문서·macOS 서비스 빌드 통과. 슬롯·매핑 회수는 다음 제어 수집에서 수행하며 RT에서 해제하지 않는다. 수정 앱/엔진은 아직 설치하지 않았다.
    - [x] Linux 단절 복구: 실제 native 노드 소실 후 같은 설정 복원·정상 다른 장치의 native ID 보존·명시 제거 후 재생성 없음 통과. GUI 요청 없는 maintenance·정상/pause 보존·재시도·종료 회귀와 제어 33개·Linux 11개·PipeWire 17개 release 검사, 포맷·strict Clippy·문서·서비스 빌드 통과. 정상 maintenance의 추가 readback·설정 저장 0회. 설치 서비스의 장시간 장애 복구는 별도 확인한다.
    - [x] Linux 콜백 관측: 검증 빌드 전용 진입/종료·스레드·버퍼 시각/대기 수. 포맷·strict Clippy·관측기 7개·release·runner 경계/정리 검사 통과. 진단 한 회의 기록 11,300개·콜백 쌍 2,823개 누락 0. 일반 빌드에는 훅 참조 없음.
    - [ ] Linux 최적화본 연속성: 기존 32채널 Float32·PCM16 및 64채널 Float32 두 구간은 각 480,255프레임 오류 0. PipeWire 1.4.2 소스 보존 패치는 reader 지연 대조의 32채널 PCM24·64채널 PCM16 손실을 막았고, 훅 제거 실행도 두 구간 각 480,255프레임 오류 0이다. 기존 source queue→Driver render 손실·PCM32·fanout/feedback·설치 제품 전체 해결은 미확인이다. [누락 조사](#linux-1024프레임-누락-조사)에 빌드와 원본/수정 대조를 구분해 기록했다.
    - [ ] Linux 같은 장치 되돌림: 32채널 native 실행에서 기대 PCM에 도달했지만 유지하지 못했다(정상 20,480·이후 불일치 29,696프레임). PipeWire 전달 지연과 정상상태 신호 복구를 확인한다. 진단은 실제 수렴값·정상/오류 프레임을 표시하며 임계값은 유지한다. 진단 변경의 포맷·strict Clippy·단위 8개 통과.
    - [ ] 수정 설치본의 실제 DAW 및 장시간 운용 확인. 위의 유한 검사를 장기 현장 무결성·절대 누수 없음으로 확대해 주장하지 않는다.
- [ ] 드라이버 독립 수명:
  - [x] Linux 과거 설치본(이전 종료 정책): 관리 앱 Ctrl+Q·클라이언트 종료 뒤 서비스와 장치 유지. 2채널 Float32 및 64채널 네 PCM 형식은 각 두 구간에서 480,255프레임·오류 0개.
  - [x] Linux 패키지 관리자 제거: Driver 프로세스·노드·API·실행 파일 해제, 설정·App 패키지 보존. 업데이트·재설치 뒤 같은 설정과 일반 사용자 서비스 복원. 최종 설치본의 64채널 PCM32도 두 구간 각각 480,255프레임·오류 0개.
  - [x] Linux GUI 제거 취소. 인증 취소·실패 시 설정 보존과 한영 안내 회귀 검사 통과. 부분 설치 실패 상태에서도 패키지 제거를 재시도하도록 수정했다.
  - [x] macOS 수정 PKG 설치: 관리자 승인 후 성공. 서비스 Ready·버전 0.1.0, 설치된 관리 앱·제거 앱과 최신 패키지의 일치 및 서명을 확인했다. Mac 재부팅은 하지 않았다.
  - [x] macOS 설정 보존 제거: 설치된 별도 제거 앱에서 관리자 승인 후 관리 앱·중첩 제거 앱·HAL·서비스·LaunchDaemon·Driver 캐시와 환경설정이 제거됐다. Driver 설정·그래프·오디오 설정의 해시는 제거 전과 같다. 제거 취소 시 관리 앱·서비스·장치 유지도 통과했다.
  - [x] 과거 설치 실패 복구 폴더 영구 삭제: 폴더 부재와 Driver 설정·그래프·오디오 설정의 해시 보존을 확인했다. 오래된 패키지 사본은 휴지통으로 옮겼고 현재 패키지는 보존했다.
  - [x] macOS 최종 패키지 재설치: 표시·소유권 수정본 설치 성공. 재부팅 없이 서비스 Ready·장치 ID·이름·2채널·48kHz Float32 유지, 설치 앱의 payload·서명 일치 확인. 서비스 게시 revision만 증가했다. 그래프·오디오 설정은 변경되지 않았다.
  - [ ] macOS 제거 잔여: 인증 취소 시 관리 앱 복원·선택 삭제 확인 남음.
  - [ ] macOS 다른 계정 재설치: 제거 후 남은 lease 소유권을 잠금 아래에서 새 계정에 재결합하도록 수정했다. 설치 도구의 release 검사 115개·포맷·strict Clippy·독립 리뷰·ARM64/Intel 빌드·문서 검사는 통과했다. 수정한 Universal 2 패키지의 서명·payload·공증·ticket·Gatekeeper 검사를 통과했다. 실제 다른 UID 변경·재설치 확인은 남았다.
  - [x] Linux GUI 정상 승인·설정 삭제 선택: 2026-10-08 Debian 13 x86_64 VM의 설치된 제거 도구를 일반 사용자 터미널에서 실행했다. 실제 Zenity 확인창 취소는 exit 0·패키지/설정 보존, Polkit 암호 입력 EOF는 exit 127·인증 실패 안내·패키지/설정 보존이었다. 정상 Polkit 승인 후 기본 제거는 설정 해시 보존·서비스/API/실행 파일 해제, 재설치는 같은 64채널 PCM32 설정 복원, 삭제 선택 후 정상 승인은 정확한 Driver 설정 파일 삭제를 확인했다. 테스트 중 VM 계정에만 일회성 암호를 설정했고 원래 잠금 상태를 복원했다. GUI 인증 agent의 취소 exit 126은 이번 실제 실행에 포함하지 않았으며 기존 회귀 검사 결과로 구분한다.
- [ ] 장치 입출력: 일반 CoreAudio·PipeWire 클라이언트로 전환. 입력=capture·출력=render의 UI·엔진 라이브러리와 macOS callback·변환 버퍼 검사 통과. Linux native 32/64채널 같은 장치 피드백과 설치된 64채널 네 PCM 형식 연속성 통과. 설치 장치가 필요한 6개 검사·다중 그래프 합산·회수 확인 남음.
- [ ] 가상 장치 설정: macOS·Linux의 1–64채널 설정 경로 유지. Linux 설치 관리 앱의 64채널 PCM24 선택과 native readback 일치. 실제 DEB 업데이트·서비스 재시작 뒤 장치 ID·이름·64채널 PCM32·설정 파일의 소유자/권한 보존 확인. revision만 새 native 소유자 게시에 따라 증가한다. macOS 설치본 확인 남음.
  - [x] Driver 메뉴 단순화: 장치별 공통 샘플레이트·비트 깊이·채널 수만 표시. 앱 열기·장치 중간 메뉴·업데이트 팝업 제거. release 검사·Universal 2·서명·공증 통과. 설치본 화면에서 제품명·버전, 48kHz·24bit·4채널, PCM 선택지와 현재 값 체크 표시 확인. 업데이트 확인은 팝업 없이 메뉴에 ‘배포된 업데이트 없음’을 표시했다.
    - [x] macOS Dock 미표시: 새 서명 Universal 2 관리 앱의 시작·다시 열기·재실행에서 Dock 부재와 상태 표시줄 유지 확인. 서비스·장치 설정은 보존된다. 설치본 교체는 남았다.
    - [ ] 채널 메뉴 개선: 2·4·8·16·32·64만 남기는 구현·서명 Universal 2 빌드 완료, 설치본 교체 대기. 기타·범위 메뉴와 비표준 현재값의 추가 선택지·구현은 제거했다. 동일한 전체 1–64 지원·현재 4채널 조건에서 첫 메뉴는 64→6행이다. 사용자 요청으로 추가 테스트·화면 검증은 생략했다. 기존 설치·설정 보존 결과는 유지한다.
    - [x] macOS 업데이트 설치: 기본 Installer로 2026-10-06 19:34 성공. 설치된 관리 앱이 최신 payload와 일치하고 서명 유효. 서비스 Ready·같은 장치 ID·2채널·48kHz Float32 유지, 재부팅 없이 완료. 이전 메뉴 앱 잠금 실패 수정과 패키지 검사 56개·공증·Gatekeeper 통과 결과 유지.
    - [x] 상단 제품명·실제 설치 버전 표시: 설치본 화면에서 `MixRelay Driver 0.1.0`만 표시함을 확인. 메뉴 검사 7개·포맷·strict Clippy·문서·Universal 2·서명·공증·Gatekeeper 통과. 2026-10-06 23:22 업데이트 설치 성공, 새 payload 일치·서명·실행 확인. 재부팅 없이 서비스 Ready·장치 ID·4채널·48kHz PCM24 보존.
    - [x] 공통 비트 깊이 명령: 양방향을 한 번에 적용하고 미지원·오래된 요청·실패 시 설정을 보존하는 release 검사 통과.
    - [x] 폐기된 이름 변경 UI·이벤트·번역 제거: 앱의 이름 편집은 유지. headless 시작·공통 API 회귀·타입·UI 경계 검사 통과.
- [ ] 장치 형식 선택: UI·정수 PCM 변환·macOS Universal 2 HAL 빌드·서명·factory/COM 통과. Linux native 32/64채널 네 형식 readback·제거 통과. 이전 PCM 꼬리에 새 검사가 잠기는 측정 결함을 실행별 식별자로 수정했다. 실제 DEB의 64채널 Float32·PCM16·PCM24·PCM32는 각 두 구간에서 480,255프레임·누락·반복·오샘플·부분 입력 0개다. 설정 변경·업데이트·재설치 복원도 통과했다. macOS 설치본 실행 확인 남음.
- [ ] 앱별 캡처: Linux 48k/128-frame 480,000프레임 연속성 통과. 앱 분리·권한 거부·무신호·rate 차이·타 앱 혼입 방지 확인 남음.
- [ ] 앱 재추적: 새 프로세스·스트림에서 같은 앱 PCM 복원 확인.
- [ ] 클록 보정: 44.1k→48k duration 오차 0.259프레임 관측. observer의 SRC 초기화 수정 후 재검증과 실제 drift 확인 남음.
- [ ] 플러그인 검색: 설치 효과의 검색·재검색·오류·복구 확인.
- [ ] 플러그인 DSP: 내장·외부 PCM·bypass·reset·재구성·제거 확인.
- [ ] 플러그인 다채널: 버스·sidechain·64→16 매핑·미지원 거부 확인.
- [ ] 플러그인 상태: 저장·재시작·Undo·포맷 변경 복원 확인.
- [ ] 플러그인 에디터: 창·resize·worker 종료 후 회수 확인. 임의 파라미터가 PCM을 반드시 바꾼다고 요구하지 않음.
- [ ] 배포:
  - 완료: 패키지 문서·설정 보존·선택 삭제, 업데이트 API·R2 게시·패치 보관·목록 생성 검사. 앱의 업데이트 조회·링크 재확인과 기존 코드·UI 검사를 통과했다. Linux Driver ARM64 DEB 제품 빌드·실제 payload 검사 통과. macOS 설치·서비스 Ready, 제거 확인창·취소·미설치 안내와 설정 보존 제거・최종 패키지 재설치를 확인했다. 설치본 업데이트 확인은 남았다. 앱과 Driver 약관은 각각 지원 11개 언어의 전문을 제공한다. 설치 안내·약관의 RTF 표시 수정은 실제 다크 모드 화면·원문 보존·독립 리뷰·변경 경계 검사를 통과했다. 수정 패키지의 서명·새 공증·ticket·Gatekeeper 허용을 확인하고 현재 배포 산출물로 교체했다. 설치 유형에서 제거를 선택하던 PKG 후보는 사용자 UX 검토에서 거부됐다. 현재는 설치·업데이트 PKG에 전용 제거 앱을 함께 설치하는 구조로 변경 중이며 새 구성의 실측 완료를 주장하지 않는다.
  - 남음: 실제 제품 게시·설치/업데이트/제거, 자동 인증 정리 변경·설치 약관 표시/동의·제3자 고지와 법무 검토 기록의 구현·운영 요건. 실제 제품 파일은 아직 게시하지 않음.
- [ ] 자원·성능: 정상·실패 시 회수, RT 무할당·무차단과 동일 조건 계측 확인.

### 미구현

- [ ] 무료·유료 정책 적용: 기존 14일 체험 코드를 무기한 제한 체험으로 교체. 드라이버·장치 설정은 무료로 유지하고 그래프·매핑 저장·불러오기·자동 복원 제한을 같은 제어 경로에 적용. 무료 앱의 수치 제한은 확정 후 반영.


2026-10-09 제거 흐름 검토: `installer-handoff` 후보는 일반 사용자 제거 앱에서
서명된 scripts-only PKG를 macOS Installer로 넘겨 사용자 앱의 광범위한 관리자 접근 요청을
없애도록 만들었다. 두 제거 PKG와 최종 설치 PKG의 서명·공증·Gatekeeper, 11언어 문구,
Rust 9개·패키징/번역/가드 120개·strict Clippy를 통과했다. 실제 수정본 설치 후 네 실행 파일의
payload 일치와 정상 종료·활성 클라이언트 배출·동일 장치 재시작을 확인했다. 인증 전 Installer
취소는 관리 앱 PID·장치·설정 해시를 보존했다.
그러나 제거를 Installer로 넘기면 시스템 UI가 다시 ‘설치’로 표시된다. 사용자가 이 혼란을
지적하여 제거 실측을 중단하고 테스트가 연 Installer와 제거 앱을 모두 종료했다. 실제 제거는
실행하지 않았고 Driver와 설정은 유지됐다. 이 후보는 배포 승인하지 않는다. 제거 전용 화면과
권한 요청 흐름은 추가 수정이 필요하다. Debian 새 패키지의 실제 제거·재설치·설정 보존·상태
메뉴 종료/재시작은 통과했고 VM은 원래의 종료 상태로 돌렸다.
