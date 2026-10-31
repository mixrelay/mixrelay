# Polar 판매 설정

소유자가 [Polar](https://polar.sh) 대시보드에서 아래 설정을 완료한다. API 토큰이나 자체 결제
서버는 앱에 필요하지 않다. API 계약과 제한은 [조사 문서](research/polar-licensing.md), 판매·사용권
정책은 [제품 명세](product.md#판매지원-정책)를 따른다.

## 대시보드

1. Echo Vision Lab의 판매 조직을 만들고 실제 사업자 정보·지원 연락처·웹사이트·판매조건과
   개인정보 방침을 등록한다. Polar의 조직 검토와 판매 승인 상태를 확인한다.
2. 정산 설정에서 소유자의 지급 계좌를 연결하고 요구되는 본인·사업자 확인을 완료한다.
   계좌·정산 인증정보는 저장소나 앱 빌드에 전달하지 않는다.
3. MixRelay 소프트웨어 상품을 만든다. 일회성 고정 가격을 USD 19.00으로 설정한다.
   구독·사용량·기기 좌석 가격을 사용하지 않는다. 세금은 가격에 포함하지 않고 결제 전에
   적용 세금과 총액을 표시한다. 할인 코드와 checkout의 할인 코드 입력을 끈다.
4. License Keys benefit를 만들고 이 상품에 연결한다. 활성화 제한·만료·사용량 제한을 모두 끈다.
   발급 키에서 `limit_activations: null`, `expires_at: null`인지 확인한다. 이 benefit는 MixRelay의
   현재 사용권에만 쓴다. 같은 조직의 다른 상품이나 차기 유료 메이저용 benefit와 공유하지 않는다.
   Polar는 무제한 키의 원격 기기 활성화를 허용하지 않는다. MixRelay는 기기 이름과 해제를
   로컬에서 관리하며, 컴퓨터 수 무제한을 유한한 큰 상한으로 대신하지 않는다.
5. Checkout Links에서 이 상품 하나만 선택한 영구 링크를 만든다. 할인·고객 이메일·추적 쿼리를
   넣지 않는다. 생성된 일시적 checkout session URL을 복사하지 않는다. 본 서비스 링크는
   `https://buy.polar.sh/polar_cl_…` 형식이다. 소유자의 실제 링크는 저장소에 하드코딩하지 않는다.
6. 공개 판매조건에 구매 후 30달력일 내 무조건 전액 환불 정책과 지원 주소를 표시한다.
   주문의 Refund order에서 전액 환불할 때 **Revoke benefits** 기본 선택을 유지하고,
   환불 완료와 키의 `revoked` 상태를 함께 확인한다. 부분 환불 때는 전체 benefit 회수를 끈다.
   환불 버튼만 눌렀다고 키 회수가 완료됐다고 판단하지 않는다.
7. 1.0 출시 때 상품의 일회성 세금 별도 가격을 USD 29.00으로 변경하고 checkout link가
   새 가격을 사용하는지 확인한다. 가격 수정이 지원되지 않는 경우 기존 가격을 보관 처리하고
   같은 상품에 새 가격을 만든다. 웹·앱·결제 화면을 함께 확인한다. 1.0 전 구매자의 0.x·1.x
   업데이트 권리는 유지한다. 2.0 등 다음 유료 메이저는 별도 benefit로 구분한다.

## 전달할 공개 값

| 빌드 값 | 소유자가 복사할 위치 |
| --- | --- |
| `MIXRELAY_POLAR_ORGANIZATION_ID` | 조직 Settings의 조직 UUID |
| `MIXRELAY_POLAR_BENEFIT_ID` | MixRelay License Keys benefit의 UUID |
| `MIXRELAY_POLAR_CHECKOUT_URL` | Checkout Links에서 복사한 본 서비스 영구 URL |

세 값은 함께 설정하거나 개발 빌드에서만 모두 생략한다. 비밀이 아닌 공개 상품 설정이다.
조직·상품 이름, product UUID, API access token, 고객 키는 빌드 값으로 대신 넣지 않는다.

GitHub 저장소의 **Settings → Secrets and variables → Actions → Variables → Repository variables**에
위 이름 그대로 세 값을 등록한다. Linux 후보 빌드는 이를 실제 Docker 빌드 컨테이너에 넘긴다.
후보 선택 workflow도 같은 값으로 후보 명세를 만든다. 값이 없거나 잘못됐으면 패키지·후보 생성을 거부한다.

로컬 macOS 릴리스와 `pnpm release:candidate`는 기존 비공개 서명 설정의 `polar` 객체에서 읽는다.
기본 파일은 `/Users/state303/Library/Application Support/MixRelay/Signing/background-stop-20261011/config.json`이며,
다른 파일은 기존 `MIXRELAY_SIGNING_CONFIG`로 선택한다. 기존 서명·공증 설정은 보존하고 다음 필드만 추가한다.

```json
{
  "polar": {
    "organizationId": "대시보드에서 복사한 조직 UUID",
    "benefitId": "MixRelay 라이선스 benefit UUID",
    "checkoutUrl": "대시보드에서 복사한 본 서비스 영구 checkout 링크"
  }
}
```

위 예시의 설명 문자열은 실제 값으로 바꾼다. 기존 owner-only 파일 권한을 유지한다.
환경 변수도 설정돼 있으면 비공개 파일과 같은 값이어야 한다. 충돌·누락·sandbox는 키체인을
열기 전에 거부한다. `--package-only`도 최종 앱의 내장 설정과 현재 릴리스 값을 대조한다.
후보 설치 시험은 설치된 앱 바이너리에서 후보 명세와 같은 본 서비스 설정을 다시 확인한다.
설정이 없는 기존 후보 파일은 새 설정을 가진 앱으로 다시 빌드해야 한다.

## 판매 전 확인

별도 sandbox 계정·조직·상품·benefit로 구매·키 확인·환불을 시험한다. 계정은 소유자가 만든다.
개발 빌드에만 `MIXRELAY_POLAR_SANDBOX=1`을 함께 설정하고 sandbox의 세 값을 사용한다.
영구 checkout 링크는 `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_…/redirect`를 쓴다.
본 서비스와 sandbox의 조직·키는 서로 통하지 않는다. 릴리스는 sandbox를 거부한다.

소유자는 성공 구매의 발급 키, 다른 본인 컴퓨터의 같은 키 사용, 로컬 기기 해제,
회수·비활성·전액 환불 뒤 온라인 확인의 무료 전환, 30일 오프라인 만료를 확인한다.
진행 중 오디오·녹음이 유지되고 새 유료 작업만 제한되는지도 확인한다.
계정 없는 API 연결 확인은 이 구매·환불 수락 검증을 대신하지 않는다.
