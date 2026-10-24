# MixRelay 웹사이트 초안

정적 HTML과 CSS로 작성했습니다. 빌드나 패키지 설치 없이 저장소 루트에서 구조 검사를 실행할 수 있습니다.

```sh
node apps/website/check-site.mjs
```

로컬에서 페이지를 열려면 저장소 루트에서 다음처럼 정적 파일 서버를 실행하세요. Python 표준 라이브러리만 사용합니다.

```sh
cd apps/website
python3 -m http.server 4173 --bind 127.0.0.1
```

브라우저에서 `http://localhost:4173/`를 엽니다. 종료하려면 서버 터미널에서 `Ctrl+C`를 누릅니다.

페이지는 사전 공개 초안입니다. 실제 제품 공개 전에 제품·OS 검증 상태와 설치 안내를 확인하고, 확정된 구매 링크와 가격만 추가해야 합니다.

서체는 Core의 IBM Plex 원본을 동봉합니다. Core 서체 계약을 바꾸면 `node scripts/sync-website-fonts.mjs`로 웹 자산을 생성한 뒤 구조 검사를 실행합니다. `assets/fonts/OFL.txt`가 원본 라이선스입니다.

전체 사용 안내서는 [언어 선택](manual/index.html), [English](manual/en/index.html),
[한국어](manual/ko/index.html)에서 읽을 수 있습니다. 기존 `guide.html`은 이전 절 링크를
유지하는 안내 페이지로 바꿨습니다. 새 언어 추가 방법·번역 용어표·근거와 공개 전 확인
범위는 [안내서 관리 문서](manual/README.md)에 있습니다. 모든 페이지는 공통 `styles.css`를
사용하며 별도 빌드는 필요하지 않습니다.

구조 검사는 전체 HTML 페이지의 내부 링크·절 ID·문서 언어·제목 계층·랜드마크·이미지
대체 텍스트와 언어별 장 구성을 확인합니다. 한국어 페이지의 푸터는 한국어 정책으로,
나머지 언어는 영어 정책으로 연결합니다. 외부 링크는 지원 메일·Apple 공식 도움말과
정책 페이지에 명시한 법령·Lemon Squeezy 자료로 제한합니다. 실제 설치·권한·오디오
검증과 스크린리더 검증을 대신하지 않습니다.

판매조건과 개인정보 처리방침 초안은 [한국어](legal/ko/terms.html)와
[영어](legal/en/terms.html)로 제공합니다. 미확정 운영 정보는 본문의 대괄호 표시로
남겨 두었습니다. `legal/{ko,en}/LICENSE*.txt`는 저장소 루트의 해당 언어 사용권
전문 사본입니다. 사용권 원문을 변경하면 사본도 갱신해야 하며, 구조 검사가 일치를
확인합니다. 이 초안과 사업자 정보를 검토하기 전에는 공개·판매하지 않습니다.
