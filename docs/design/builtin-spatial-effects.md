# 내장 공간 효과 구현 근거

제품 범위와 진행 상태는 [product.md](../product.md)가 기준이다.

## 출처와 배포 판단

- Jezar at Dreampoint의 2000년 Freeverb 배포 원문은 public domain을 선언하고 상용 제품에 사용해도 된다고 명시한다. 제품 사본 전달은 부탁이며 강제 조건으로 적혀 있지 않다. 원래 Dreampoint 주소 대신 원문 보존본의 [readme.txt](https://github.com/sinshu/freeverb/blob/cfcea55553fb59ac57ebf2a237f72cad4296f2b0/readme.txt)를 확인했다.
- Julius O. Smith의 저서 [Freeverb 분석](https://www.dsprelated.com/freebooks/pasp/Freeverb.html)은 8개 병렬 lowpass-feedback comb와 4개 직렬 diffusion section, 스테레오 입력 합산과 좌우 지연 차이를 설명한다. 특히 원형 Freeverb의 diffusion section이 일반적인 Schroeder allpass의 근사임을 구별한다.
- 공개 topology를 참고하며 외부 C++/Rust/Faust 소스, Freeverb `tuning.h` 상수, 프리셋 자산, IR, 바이너리 또는 패키지를 가져오지 않았다. 이 구현에 추가되는 제3자 코드·전이 의존성·런타임은 없다. 자체 코드에 다른 구현의 라이선스 이름을 붙이거나 Freeverb 원형과 동일한 소리라고 표시하지 않는다. 이 결정은 기존 제3자 의존성의 고지 의무를 바꾸지 않는다.

## 자체 DSP 계약

`audio-graph/src/effects.rs`가 저장·검증 계약, `audio-engine/src/render/effects.rs`가 준비·처리를 소유한다.

리버브 알고리즘 0 **기본형**은 기존의 4 comb/2 allpass 구조이고, 1 **조밀형**은 8 comb/4 allpass 구조다. 두 모드는 실제 다른 응답을 만든다. 각 comb는 한 극 lowpass가 포함된 피드백을 쓰고, 각 diffusion section은 `(-g + z^-N)/(1 - g z^-N)` 형태의 실제 Schroeder allpass(g=0.5)다. 피드백은 지연 길이와 명목 잔향 시간에서 `exp(-ln(1000) × delaySeconds / decaySeconds)`로 계산한다. 댐핑과 diffusion 때문에 표시 시간이 전체 스펙트럼의 정확한 측정 T60이라는 뜻은 아니다.

`roomSize`는 피드백만 바꾸는 원형 Freeverb의 동명 파라미터와 달리 자체 지연 길이를 0.5–2배 변경한다. `preDelayMs`는 wet 경로 앞의 0–200ms 지연이다. 조밀형은 각 스테레오 쌍의 평균 입력으로 좌우에 다른 길이의 잔향을 만들고 기본형은 기존 채널별 excitation을 유지한다. mono와 마지막 홀수 채널은 독립 mono로 처리하며 다른 스테레오 쌍끼리는 섞지 않는다.

알고리즘 전환은 5ms 계수 스무딩으로 comb 합산과 추가 allpass 출력, stereo excitation을 이어준다. 모든 지연을 계속 처리하므로 비활성 경로의 오래된 오디오가 다시 나타나는 일을 피한다. 최대 공간 크기·선행 지연의 ring을 control 준비 단계에서 한 번 할당하고 실시간 콜백에서는 할당·해제를 하지 않는다. Off/reset은 ring의 유효 history와 lowpass 상태를 무효화한다. Bypass는 dry 출력과 별개로 history를 유지한다.

딜레이 0 **스테레오 에코**는 채널별 지연이고, 1 **핑퐁**은 각 스테레오 쌍의 평균을 왼쪽 지연에 넣고 좌우를 교차 피드백한다. 따라서 중앙 impulse의 wet 반복이 L→R→L로 이동한다. 양쪽 과거 값을 먼저 읽고 나서 쓰므로 처리 순서가 결과를 바꾸지 않는다. mono/홀수 마지막 채널은 일반 echo로 처리한다. feedback 범위는 0–0.9이고 0ms는 즉시 피드백을 만들지 않는다. 모드·값·Bypass 전환은 기존 스무딩 계약을 사용한다.

저장값은 `bypass`와 효과의 모든 파라미터(리버브 `algorithm`·`roomSize`·`preDelayMs`, delay `mode` 포함)를 명시해야 하며 빠진 필드는 거부한다. 공간 프리셋은 조밀형을 명시한다.

## 프리셋과 화면

리버브: 작은 방·스튜디오·홀·가라오케·터널·어항. 공간 크기·선행 지연·잔향 시간·댐핑·wet 비율을 모두 저장한다. 작은 방은 빠르고 짧고, 홀은 더 넓고 밝게, 터널은 늦게 시작하는 긴 어두운 tail, 어항은 작고 강하게 damped된 잔향을 만든다. 가라오케 단일 리버브와 에코+리버브 가라오케 체인은 구별한다. 어항 전체 체인은 별도의 필터/코러스도 포함한다.

딜레이: 짧은 에코·슬랩백(피드백 없음)·보컬 에코·가라오케·긴 에코·핑퐁. 핑퐁 프리셋은 실제 교차 피드백 모드를 저장한다. 시간은 ms이며 호스트 tempo sync를 제공한다고 표시하지 않는다.

Feature `builtinEffects.ts`는 값·범위·선택·section 메타데이터만 제공한다. 리버브 기본 면의 3노브(잔향 시간·댐핑·효과 비율)를 유지하고 알고리즘·공간 크기·선행 지연은 Core 공통 Options에 둔다. 프리셋 선택은 전체 설정을 한 번에 적용하며 이후 값 변경은 사용자 설정으로 표시한다.

검증은 impulse의 에코 위치·교차 감쇄, 선행 지연의 샘플 위치, 기본형/조밀형의 tail 밀도 차이, 방/터널의 시작·tail 에너지 차이, mono/stereo, 블록 분할 불변성, 여러 샘플레이트, bypass/reset, preset activation 중 history 보존과 callback heap operation 0을 확인한다. 설치본 청취·장시간 검증을 자동 테스트 결과로 대신하지 않는다.
