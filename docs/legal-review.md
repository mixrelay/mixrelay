# MixRelay 계약 검토 기록 — 2026-10-08

이 문서는 개발·출시 검토 기록이다. 설치 약관, 앱의 계약 원문 또는 고객용 개인정보 처리방침으로 배포하지 않는다.
검토 대상은 App·Driver의 11개 언어 계약 전문과 실제 인증·업데이트·패키징 구현이다.
문구 검토와 제품 출시 요건의 충족을 구분하며, 확인하지 않은 외부 운영 상태를 완료로 표시하지 않는다.

## 계약에 반영한 변경

| 항목 | 판단 및 반영 | 근거 |
| --- | --- | --- |
| 제목·버전 | 제품명과 Software License Agreement를 구분하고 버전을 앞에 표시했다. 전문 검토 미완료, 배포 초안, 출시 전 확정 지시는 제거했다. | 두 계약 원문; Installer RTF 생성 및 원문 보존 검사 |
| 적용 범위 | Driver 무료 허가와 App 구매·활성화를 분리한다. App 환불·종료로 Driver 무료 사용권이 자동 소멸하지 않는다. | `docs/product.md`; 독립 Driver 실행 모드 |
| 체험·구매 | 무기한 기능 제한 체험, 구매 메이저 버전, 동시 3기기, 최대 30일 오프라인 인증을 구분했다. 알리지 않은 제한 또는 구매 기능 축소를 약관으로 허용하지 않는다. | 제품 정책; `crates/licensing/src/entitlement.rs`; 아래 구현 불일치 |
| 기기 교체 | 서버가 확인한 비활성화만 좌석을 해제한다. 삭제·재설치와 구매 취소를 혼동하지 않고 접근할 수 없는 기기는 지원 경로를 안내한다. | `crates/licensing/src/client.rs`, `credentials.rs` |
| 결제 책임 | Lemon Squeezy의 거래상 판매자 역할을 개발자 책임과 구분하고 법정 책임을 회피하지 않는다. 지원 이메일과 영수증의 주문 지원 경로를 제공한다. | Lemon Squeezy 판매자·환불 문서; 전자상거래법 제18조 |
| 청약철회 | 한국 소비자법 적용 시 일반 7일, 디지털 공급 예외의 요건, 미공급 가분 부분, 표시·계약 불일치에 대한 3개월 및 인지 후 30일 기준을 명시했다. 무료 체험·약관 동의 자체를 포기로 취급하지 않는다. | 전자상거래법 제17조 |
| 환급 | 디지털콘텐츠 법정 청약철회 시 3영업일 환급, 결제 취소 조치, 지연배상 및 위약금 금지를 설명한다. 더 유리한 권리를 축소하지 않는다. | 전자상거래법 제18조 |
| 인증 정보 | HTTPS로 Lemon Squeezy에 전송하는 키·활성화 이름·식별자와 OS 자격 증명 저장소를 명시한다. 결제정보 입력 장소를 구분한다. | `crates/licensing/src/client.rs`, `credentials.rs`; 앱 checkout 구성 |
| 업데이트 | 제품·OS·아키텍처 및 접속 정보 수신을 설명한다. App은 시작·24시간 간격·수동 확인, Driver는 수동 확인이다. 해당 요청에 오디오·프로젝트·그래프·오디오 장치 이름은 들어가지 않는다. | `apps/desktop/src-tauri/src/updates.rs`; `useUpdatesSession.ts`; `driver` 메뉴 |
| 개인정보 권리 | 문의·열람·정정·삭제·처리 제한 연락처를 제공한다. 사용권 동의를 선택적 마케팅·분석 또는 국외 이전 동의로 취급하지 않는다. 알 수 없는 보존기간은 만들지 않았다. | 개인정보 보호법 제28조의8·제30조; 아래 운영 확인 사항 |
| 작업물·제3자 권리 | 녹음·프로젝트 권리는 사용자에게 남고 학습·마케팅 권리를 얻지 않는다. 독점 코드 제한과 Apple/MIT/Apache/MPL 구성요소 권리를 구분한다. | 제품 정책; `LICENSE.apple-null-audio.txt`; 번들 제3자 고지 |
| 면책·종료·변경·분쟁 | 고의·중과실 면책, 일괄 배상 상한, 임의 유료권 박탈, 묵시적 불이익 동의, 불리한 전속 관할을 부여하지 않는다. 개별 약정과 강행 소비자 권리를 보존한다. | 약관법 제6–14조 |

## 구현·운영에서 확인할 사항

### App 출시를 막는 구현 불일치

`docs/product.md`는 14일 체험을 폐지하고 기간 제한 없는 기능 제한 체험을 요구한다.
그러나 `crates/licensing/src/entitlement.rs`에는 `TRIAL_SECONDS = 14 * 86_400`이 있고,
현재 App 접근 UI도 만료된 체험을 차단한다. 계약서의 체험 정책을 코드에 맞춰 몰래 바꾸지 않았다.
기간 제한 폐지와 무료 기능 제한 구현이 끝나기 전에는 App을 이 계약에 맞게 구현했다고 판정하지 않는다.
무료 그래프·노드·처리 채널·녹음 제한 수치도 제품 정책에서 미확정이다.
이 불일치는 구매 인증을 시작하지 않는 독립 Driver의 무료 사용과 별개다.

### 판매 운영

Lemon Squeezy store/product/variant/checkout 설정이 없는 빌드는 구매·활성화를 제공하지 않는다.
실제 판매 설정, 거래 판매자 표시, 가격·세금, 결제 전 계약·환불 고지, 지원 및 좌석 복구 절차를
확인해야 유료 판매를 출시할 수 있다. 통신판매 신고/면제와 과세유형은 현재 원문 사업자 정보만으로
확정할 수 없다. 설정 값이나 신고번호를 추측해서 작성하지 않았다.

### 개인정보 처리방침과 국외 이전

클라이언트가 보내는 정보와 목적은 코드에서 확인했다. 별도 배포 저장소에는
`appserver.dsub.io`의 HAProxy ingress 및 R2 배포 구성이 있다. ingress 설정은 IP·URL 경로·
사용자 에이전트·Cloudflare 접속 관련 정보를 stdout으로 기록한다. Appserver의 `main` 소스와
README도 확인했으며 업데이트 경로에는 데이터베이스나 구매 인증 처리가 없다. 다만 소스와 설정만으로
현재 서버·CDN 접속 로그와 지원 메일의 실제 수령자, 국가, 보존·삭제 주기를 확정할 수 없다.
Notion의 MixRelay 검색에서도 해당 운영 정책을 찾지 못했다.
계약의 개인정보 조항은 확인된 데이터 흐름과 권리를 설명하며 완전한 처리방침을 대신하지 않는다.

개인정보 보호법 제30조에 따른 목적·보유기간·파기·수탁자·권리 행사·책임자 고지와
제28조의8에 따른 국외 이전 요건은 실제 운영에 맞는 별도 고지 및 절차가 필요하다.
Lemon Squeezy 방침 링크나 EULA 동의만으로 이를 충족했다고 판정하지 않는다.
외부 운영에 대한 접근·설정 확인 없이 보존기간이나 삭제 완료를 보장하는 문장을 만들지 않는다.

### 국내 이용자에 대한 언어·설명

Driver는 영어·일본어·한국어·중국어 간체·번체·프랑스어·독일어·스페인어·포르투갈어·이탈리아어·러시아어의 전문을 제공한다.
macOS Installer의 11개 `.lproj`에 전체 약관과 설치 안내를 제공한다.
모든 언어는 8개 조항과 사업자 정보를 포함하며, 언어 변경으로 새 사용권 제한을 추가하지 않는다.
11개 Driver 원문을 관리 앱과 제거 앱에도 보관한다. 파일 대응은 `packaging/localization/languages.json`을 따른다.
Installer의 기본 언어 선택과 인쇄·저장 기능을 이용한다.
App도 `LICENSE.txt`와 `LICENSE.ko.txt`, `LICENSE.ja.txt`, `LICENSE.zh-Hans.txt`,
`LICENSE.zh-Hant.txt`, `LICENSE.fr.txt`, `LICENSE.de.txt`의 영어·한국어·일본어·중국어
간체·번체·프랑스어·독일어 계약 전문과 `LICENSE.es.txt`, `LICENSE.pt.txt`,
`LICENSE.it.txt`, `LICENSE.ru.txt`의 스페인어·포르투갈어·이탈리아어·러시아어 전문을 제공한다. 앱에서 선택한 언어로 계약 원문 전체를 표시한다.
언어 리소스 제공만으로 모든 중요 조건의 설명이나 별도 개인정보 처리방침까지 완료됐다고 판정하지 않는다.

### 실제 설치와 운영 증거

원문과 RTF의 문자 일치, 앱·제거 앱 리소스 일치, 서명·공증은 문서 전달 검증이다.
사용자의 계약 동의나 관리자 설치 인증, 설치된 macOS Driver의 오디오 실행 검증을 대신하지 않는다.
외부 전문가가 검토했다거나 모든 국가의 법적 요건을 충족했다고 표시하지 않는다.

## 개정 문서 전달 검증 — 언어 선택 추가 전

- 패키징 검사 56개, App 원문 표시 검사 22개 통과.
- 최종 Universal 2 Driver 관리 앱을 다시 빌드하여 App에 내장된 원문도 갱신했다.
- 관리 앱의 App·Driver 원문, 내장 및 독립 제거 앱의 Driver 원문이 현재 저장소 파일과 바이트 단위로 일치한다.
- 최종 PKG에서 추출한 RTF를 macOS `textutil`로 읽어 약관 및 설치 안내의 모든 문자가 원문과 일치함을 확인했다.
- 실제 Installer에서 최종 패키지의 제목·버전·본문·사업자 정보와 내부 메모 제거를 확인했다. 약관 동의나 설치를 실행하지 않았다.
- 최종 PKG 공증 Accepted: `904f0b5d-57c8-4010-aeea-7e08c482edca`. 패키지·관리 앱·제거 앱의 ticket 및 Gatekeeper 검증 통과.
- PKG SHA-256: `6dd27041565c606d5a0051ea8ec8b125a68b8027914100aa47411af4ec940c85`.
- 상세 근거는 `.lab/driver-release-20261008/legal-review-final/document-verification.json` 및 `release-verification.json`에 보관한다.

## Driver 종료 동작과 설치 안내의 별도 검토

기능 리뷰와 문구 리뷰를 별도로 수행했다. 이전 Driver 메뉴의 `app.exit(0)`는 관리 UI만
종료하므로 "Driver가 계속 실행된다"는 안내가 당시 구현에는 맞았지만, 새 요구사항에는
맞지 않았다. 현재 명시적 종료는 기존 네이티브 장치 제어 경로로 모든 Driver 장치와
기본 루프백을 중지한 후 관리 앱을 종료한다. 장치 설정은 보존하고 관리 앱 재실행 시
장치와 루프백을 복원한다. 유료 MixRelay 앱 종료와 Driver 메뉴 종료는 구분한다.

종료 상태는 `drivers.json.stopped`에 보관하여 현재 서비스가 재시작되어도 장치가
자동 복원되지 않게 한다. 기존 `drivers.json` 형식은 유지한다. 구버전으로 롤백하면
중지 표시를 무시하고 이전 정책대로 실행할 수 있지만, 새 필드 때문에 기존 설정을
읽지 못하는 문제는 만들지 않는다. 선택적 설정 삭제는 중지 표시도 삭제한다.

시작 직후 종료와 지연된 서비스 시작, 중지·저장 실패 및 재시도, 미설치 상태 종료를
검토했다. 중지 또는 저장 실패 시 관리 앱을 닫지 않고 재시도할 수 있도록 안내한다.
한영 설치 안내와 README의 이전 종료 유지 문구도 새 동작에 맞춰 수정했다.
제어 41개, 메뉴 23개, macOS 사용자 데이터 정리 9개, Linux 제거 5개,
Installer 패키징 58개 검사와 변경 패키지의 strict Clippy가 통과했다.
이 검사는 저장소·제어 API fixture 검증이며 실제 설치된 Driver의 종료 및 PCM 재시작
검증을 대신하지 않는다. 상세 기록은
`.lab/driver-release-20261008/quit-reviewed-installer/quit-behavior-review.md`에 보관한다.

Apple Installer의 기본 언어 메뉴에서 `English`와 `한국어`를 실제로 선택하여 각
전체 약관 본문이 전환되고 한국어 설치 안내가 자동 표시되는 것을 확인했다.
공용 영문 RTF가 언어별 리소스를 덮어쓰는 문제는 공용 파일을 제거하여 해결했다.
이 UI 검증은 `native-language-installer-final` 패키지에서 수행했으며,
이후 종료 안내 및 대표자 표기 수정본에서는 사용자 요청에 따라 Installer를 다시
열지 않는다. 이후 패키지는 동일한 언어별 구조와 원문·RTF·번들 일치를 검사한다.
열려 있던 Installer는 모두 종료했으며 계약 동의나 설치를 실행하지 않았다.

최종 수정본은 `quit-reviewed-installer`에 보관한다. 관리 앱·제어 서비스·제거 도구를
모두 Universal 2로 재빌드했으며 HAL만 변경 없이 재사용했다. PKG 전체를 추출하여
재컴파일한 서비스·제거 도구 및 세 계약 원문이 실제 설치 페이로드에 포함됨을
확인했다. 한영 RTF는 각각 원문과 일치하고, 공용 RTF는 없으며, 대표자 성명과
내부 검토 메모는 계약 원문에서 제거된 상태다.

- PKG 공증 Accepted: `91fdd52d-90a1-48aa-8c45-2cfc1a975749`. PKG·관리 앱·제거 앱의 ticket 및 Gatekeeper 검증 통과.
- PKG SHA-256: `84c0b4a79d5c49808177c4aee1e6039471f153de18f46686a9c547e75194e2b2`.
- 상세 근거: `.lab/driver-release-20261008/quit-reviewed-installer/final-review-verification.json`, `localization-verification.json`, `package-payload-verification.json`, `release-verification.json`.
- 새 설치본은 아직 설치하지 않았으며 실제 장치 종료·PCM 재시작은 미검증이다.

## 사용권 계약의 상호와 대표자 표시 — 2026-10-08 추가 검토

### App 계약 전문 번역과 표시 검토

영어 원문 SHA-256 `3aa4e89eb32436095148868093aa9c3a2553bb8ad83358ee913f9023b313a6ab`을
기준으로 6개 언어의 전문을 작성했다. 10개 조항과 조항별 본문 문단 수
`3/4/3/4/2/4/4/2/2/2`, 버전·사업자등록번호·연락처·업데이트 및 개인정보 URL,
제3자 라이선스 식별자를 대조했다. 별도 리뷰어가 전체 조항을 대조하여 주요 권한과
기간·환불·개인정보 및 제3자 권리의 의미 누락이나 새 권리 추가가 없음을 확인했다.
중국어의 불일치 인지 표현, 프랑스어의 그래프 저장 용어, 독일어의 법정 권리 보존 표현은
리뷰에 따라 정제했다. 언어 우선권이나 새로운 법원·준거법을 추가하지 않았다.

앱의 계약 펼침 영역은 언어별 실제 파일을 그대로 표시하며 언어 변경에 따라 갱신된다.
7개 언어 전문 표시와 열린 상태에서 언어 변경을 검사했고, 기존 사용권 UI 검사와 합쳐
30개 통과했다. 타입·코드 형식·UI 경계 검사도 통과했다. 상세 SHA와 검증 기록은
`.lab/driver-release-20261008/app-contract-localization-review.json`에 보관한다.
이 검토는 기존 계약의 번역 및 전달 대조이며 각 국가의 판매 운영이나 법적 요건을 새로
확정한 기록이 아니다. 외부 전문가의 검토를 받았다고 표시하지 않는다.

독립 무료 Driver의 설치 사용권 계약에 대표자 성명을 반드시 별도 줄로 반복해야 한다는
일반적인 의무는 아래 확인한 조문에서 찾지 못했다. 약관법 제3조는 한글 작성, 명시 및 중요
내용 설명을 요구한다. 사용권 허가자의 상호와 사업자등록번호·주소·연락처를 유지하는 조건으로
`LICENSE.txt`, `LICENSE-Driver.txt`, `LICENSE-Driver.ko.txt`에서 대표자 성명 줄만 제거했다.
영어 원문의 `Echo Vision Lab (에코 비전 랩)`과 한국어 원문의 `에코 비전 랩 (Echo Vision Lab)`은
동일한 상호를 뜻한다. 개인사업자라는 기존 표시를 법인으로 바꾸거나 저작권자를 변경하지 않았다.

이 판단은 소프트웨어 사용권 원문에 관한 것이다. 전자상거래법 제10조제1항제1호와
시행규칙 제7조는 전자상거래 사이버몰의 사업자 정보 표시를 요구한다. 전자상거래법
제13조제1항제1호는 청약을 받을 목적으로 하는 표시·광고에 대표자 성명을 포함하도록 하고,
제13조제2항제1호는 구매 전 거래조건 고지 및 공급 시까지 교부하는 계약내용 서면에도
공급자·판매자의 대표자 성명 등을 포함하도록 한다. 개인사업자에게 대표자 표시를 모두
면제하는 규정으로 해석하지 않았다.

따라서 유료 App 판매 사이트·결제 및 구매 계약내용 서면의 필수 신원정보까지 삭제한 것은
아니다. App 사용권 원문만으로 구매 계약내용 서면을 대신해서는 안 되며, 해당 유료 판매
경로에서는 실제 공급자·판매자와 대표자 정보를 별도로 충족해야 한다. Lemon Squeezy의
거래상 판매자 역할만으로 Echo Vision Lab의 모든 고지 의무가 면제된다고 단정하지 않는다.

- [전자상거래법 제13조: 신원 및 거래조건 정보](https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1031805891) — 시행 2026-07-21.
- [전자상거래법 시행규칙 제7조: 사이버몰 표시방법](https://www.law.go.kr/LSW/lsLawLinkInfo.do?chrClsCd=010202&lsJoLnkSeq=900616963) — 시행 2026-07-21. 제10조의 표시를 초기 화면에 요구하며, 출력 제한 기기에 대한 별도 방법을 규정한다.

## 확인한 원문 자료

2026-10-08 기준 시행 중인 법령을 사용했다. 향후 시행 개정본을 현행으로 적용하지 않았다.

- [약관법 제3조: 작성·명시·설명](https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1021562635) — 시행 2024-08-07.
- [약관법 제6–14조: 불공정 조항](https://law.go.kr/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1025032405) — 시행 2024-08-07.
- [전자상거래법 제17조: 청약철회](https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1031805939) — 시행 2026-07-21.
- [전자상거래법 제18조: 환급](https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1031805883) — 시행 2026-07-21.
- [개인정보 보호법 제30조: 처리방침](https://law.go.kr/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1029332499) — 시행 2026-09-11.
- [개인정보 보호법 제28조의8: 국외 이전](https://law.go.kr/lsLinkCommonInfo.do?lsJoLnkSeq=1020398611) — 시행 2026-09-11.
- [Lemon Squeezy: Merchant of Record](https://docs.lemonsqueezy.com/help/payments/merchant-of-record).
- [Lemon Squeezy: Refunds and Chargebacks](https://docs.lemonsqueezy.com/help/payments/refunds-chargebacks).
- [Lemon Squeezy: Privacy](https://www.lemonsqueezy.com/privacy).
- [Appserver README](https://github.com/echovisionlab/appserver/blob/main/README.md) 및 [실행 코드](https://github.com/echovisionlab/appserver/blob/main/cmd/appserver/main.go) — 업데이트 서버의 역할 확인. 실제 ingress·로그 운영 상태를 대체하지 않는다.


## 11개 언어 지원 및 최종 패키지 검증 — 2026-10-08

앱과 Driver의 필수 언어를 영어·일본어·한국어·중국어 간체·번체·프랑스어·독일어·
스페인어·포르투갈어·이탈리아어·러시아어로 확정했다. 계약 22개의 조항과 문단,
권리·기간·환불·개인정보·제3자 라이선스·사업자 정보를 전문 대조했고,
동일 조항에 새 영어 우선 규칙이나 대표자 성명을 추가하지 않았다.
언어/원문 대응은 `packaging/localization/languages.json`으로 관리한다.

앱의 422개 문구, 네이티브 메뉴와 대화상자, macOS 제거 UI, Linux 설치/제거 안내를
11개 언어로 연결했다. 문서의 접근성 언어도 UI 선택을 따른다. macOS 번들에는
CFBundleLocalizations와 권한 요청 설명의 InfoPlist.strings를 포함한다.
Apple의 [번들 현지화 키 설명](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/CoreFoundationKeys.html)을 기준으로 언어를 선언한다.
앱의 실제 사용권 컴포넌트를 390×620 창에서 11개 언어로 렌더링하여 전체 10개 조항,
언어 태그와 가로 잘림 없음을 확인했다. 이 검사는 실제 구매 인증이나 오디오 실행 검사가 아니다.

새 Universal 2 관리 앱·서비스·제거 앱·설치 도구를 빌드하고 기존 변경 없는 HAL과 패키징했다.
최종 PKG SHA-256: `e582a2d56650b0e20c9a202c9baf7b6c8675ef5aee294caf5e0cc80724464c01`.
공증 `7cb42c74-7d3d-4523-a982-b56431045b77` Accepted, stapling과 Gatekeeper 검사 통과.
실제 PKG를 풀어 11개 언어 안내·약관의 Cocoa Unicode 파싱과 원문 일치,
번들 약관 77개 사본, 실행 파일의 빌드 결과·서명·양 아키텍처와 생산 소스 101개를 검증했다.
이번에는 Installer 창을 다시 열거나 새 패키지를 설치하지 않았다.
이 빌드의 검증·한계는 `.lab/driver-release-20261008/multilingual-installer/final-verification.json`에 기록한다.
중간 패키지는 정리했고, 현재 설치·제거를 검증한 패키지는
`artifacts/macos-pkg/MixRelay-Driver-Installer-0.1.0.pkg`에 보관한다.
최종 실기기 검증 기록은 `.lab/driver-release-20261009/always-cleanup-final/`에 있다.

desktop 138개, UI 집중 126개, 전체 UI 1,047개, 패키지/현지화 169개,
Linux 패키징 33개 테스트 통과. TypeScript·Biome·strict Clippy 통과.
i18n 모듈은 커버리지 전 항목 100%이나 전체 UI의 100% 게이트는 미달이다.
전체 debug installer lib에서는 미수정 실행 파일 삭제 fixture가 SIGKILL로 실패했다
(106개 통과, 단독 debug/release 실행 통과). 관련 제거 UI 9개와 strict Clippy는 통과했다.
이 한계와 기존 App 체험/판매/개인정보 운영 요건을 언어 지원 완료나 서명·공증으로 해소됐다고 보지 않는다.
