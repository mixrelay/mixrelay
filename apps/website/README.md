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
