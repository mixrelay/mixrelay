# Polar 결제·라이선스 조사

확인일: 2026-10-31. 공식 API 계약은 `2026-10`으로 고정한다.

## 키와 기기

[라이선스 키 benefit](https://polar.sh/docs/features/benefits/license-keys)은 구매 후 키를 발급한다.
상태는 `granted`, `revoked`, `disabled`이며 만료는 별도 `expires_at`이다.
일회성 구매의 만료 설정은 구매일 기준이고, 만료를 끄면 `expires_at: null`이다.
조직뿐 아니라 `benefit_id`도 대조해야 같은 조직의 다른 상품 키를 받지 않는다.

**제약:** 활성화 제한을 끄면 `limit_activations: null`이고 원격 활성화를 만들 수 없다.
공식 [서버 구현](https://github.com/polarsource/polar/blob/main/server/polar/license_key/service.py)의
`activate`는 이 경우 403을 반환한다. 따라서 무제한 컴퓨터와 원격 기기별 활성화를 동시에
설정할 수 없다. 유한한 큰 수를 무제한으로 표시하지 않는다. 무제한 정책을 유지할 때는
기기 이름·해제를 로컬에서 관리하고 키만 온라인 확인해야 한다. 제공자 포털의 기기별 원격
해제는 이 구성에서 제공할 수 없다. 이 차이는 제품 명세와 고객 고지에도 반영한다.

## 공개 API

아래 세 경로는 모두 POST·JSON이며 인증 토큰 없이 공개 데스크톱 클라이언트에서 호출할 수 있다.
키와 고객 정보가 들어 있는 원시 응답은 로그·UI로 내보내지 않는다.

| 동작·공식 계약 | 요청 | 성공 응답 |
| --- | --- | --- |
| [activate](https://polar.sh/docs/api-reference/2026-10/customer_portal/activate-license-key) `/v1/customer-portal/license-keys/activate` | `key`, `organization_id`, `label`; 선택 `conditions`, `meta` | 200: 활성화 `id`, `license_key_id`, `label`, `meta`, 생성·수정 시각과 중첩 `license_key` |
| [validate](https://polar.sh/docs/api-reference/2026-10/customer_portal/validate-license-key) `/v1/customer-portal/license-keys/validate` | `key`, `organization_id`; 선택 `activation_id`, `benefit_id`, `customer_id`, `conditions`, `increment_usage` | 200: 키 `id`, `organization_id`, `benefit_id`, `status: granted`, `limit_activations`, `expires_at`, 사용량·검증 횟수·시각, 선택 `activation` |
| [deactivate](https://polar.sh/docs/api-reference/2026-10/customer_portal/deactivate-license-key) `/v1/customer-portal/license-keys/deactivate` | `key`, `organization_id`, `activation_id` | 204, 본문 없음 |

식별자는 UUID다. validate는 회수·비활성·만료·키/활성화 없음에 404 `ResourceNotFound`를 반환한다.
활성화 ID 없는 키 확인과 구분하면 이미 사라진 기기를 판별할 수 있다. 403 활성화 거부,
400 잘못된 요청, 422 요청 필드 오류, 429 제한과 5xx·네트워크 오류를 권한 회수와 구별한다.
불확실한 활성화 결과 뒤에는 재활성화를 반복하지 않는다.

[API 개요](https://polar.sh/docs/api-reference/2026-10/introduction)와
[sandbox](https://polar.sh/docs/integrate/sandbox)에 따르면 본 서비스는 `https://api.polar.sh/v1`,
시험 서비스는 `https://sandbox-api.polar.sh/v1`이다. 일반 제한은 본 서비스 500회/분,
sandbox 100회/분이며 공개 라이선스 세 경로는 두 환경 모두 초당 3회다. 429의 `Retry-After`를
받아도 자동 재시도 루프를 만들지 않는다. [버전 정책](https://polar.sh/docs/api-reference/2026-10/versioning)의
`Polar-Version` 헤더를 쓰며 지원 종료 시 API 버전을 갱신해야 한다.

## 판매·환불·법적 고지

[Checkout Links](https://polar.sh/docs/features/checkout/links)는 방문마다 결제 세션을 만드는
영구 링크다. 일시적인 세션 URL을 배포하지 않는다. 대시보드에서 복사한
`https://buy.polar.sh/polar_cl_…` 링크를 사용한다(sandbox: `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_…/redirect`).
앱에는 고객 이메일·추적 쿼리·토큰을 붙이지 않는다.

[환불 문서](https://polar.sh/docs/features/refunds)에 따르면 일회성 구매 환불에서 benefit 회수는
기본 선택이지만 끌 수 있다. 전액 환불 시 반드시 회수를 선택하고 키의 `revoked` 상태까지 확인한다.
부분 환불에서는 전체 benefit 회수를 끈다. 환불 사실만으로 키가 반드시 회수됐다고 가정하지 않는다.
Polar는 차지백 예방을 위해 구매 후 60일 안에 자체 환불·benefit 회수를 할 수 있다.

[구매자 약관](https://polar.sh/legal/checkout-buyer-terms)의 계약 주체는 **Polar Software, Inc.**다.
Polar는 merchant of record이자 공인 재판매자로 결제·세금·영수증·결제 분쟁을 처리한다.
제품 사용권은 Echo Vision Lab의 계약으로 정한다. 구매자 약관 URL은 위 링크,
개인정보 방침은 [Polar Privacy Policy](https://polar.sh/legal/privacy-policy)다.

기존 제공자와 달리 form·숫자 store/product/variant·boolean 성공 필드가 없고,
JSON·조직/benefit UUID·상태/HTTP 결과를 사용한다. 해제 성공은 JSON이 아닌 204다.
30일 오프라인 권한과 24시간 시도 제한은 앱의 정책이며 Polar가 발급하는 오프라인 토큰이 아니다.

계정 없이 sandbox validate를 합성 키·nil 조직 UUID로 호출해 404 `ResourceNotFound`와
`Polar-Version: 2026-10` 응답을 확인했다. 계정 생성·구매·실제 키 발급·환불은 수행하지 않았다.
