# 문자열 입력·JSON 오류 수정 검증

2026-09-14, 사용자 승인 후 구현 및 검증 완료.

## 변경

- `src/mapping.rs`: 문자열 input을 user/input_text 배열로 변환. 원문을 보존하고,
  기존 배열은 종전 처리 경로를 유지. 잘못된 input 타입은 구조화된 요청 오류로 반환.
- `RequestError`: invalid/unsupported 종류와 message/param을 보유.
  proxy의 문자열 접두사 기반 분류 제거.
- `src/api.rs`: Axum Json<Value>에 위임하는 ApiJson과 공통 ApiError.
  JSON 구문 오류 400, Content-Type 오류 415, 크기 초과 413 및 기존 8 MiB 제한 유지.
- 두 POST 핸들러가 같은 추출기를 사용. 로컬 오류는 error.message/type/code/param으로
  직렬화. 미지원 파라미터의 code는 unsupported_parameter, type은 invalid_request_error.
- 백엔드 HTTP 오류 전달, OAuth, 캐시 키 선택, SSE 알고리즘은 변경하지 않음.

## 검증

- `cargo test --locked --quiet`: 전체 13개 통과.
- `cargo clippy --locked --all-targets -- -D warnings`: 통과.
- cargo fmt 적용. release 빌드 및 cargo install 성공.
- 문자열 공백·개행·Unicode 보존, 동등한 배열 및 반복 변환 결과 일치,
  캐시 키 우선순위, 잘못된 input 타입과 param 검증.
- 두 API에서 깨진 JSON, Content-Type 누락·불일치, 8 MiB 초과의 상태 및 JSON 오류 확인.
  거절된 요청이 모의 백엔드에 전달되지 않음을 확인.
- application/*+json 정상 요청이 모의 백엔드에 도달함을 확인.
- 기존 툴 호출·메타데이터·사용량·SSE 회귀 검사 통과.
- 별도 포트 18787의 수정 서버에서 실제 계정으로 Responses 호출 4개 통과:
  문자열/배열 input × 일반/스트리밍. model 생략, 기본 gpt-5.6-luna 사용.
  모두 OK 응답과 정상 완료 확인. 시험 서버 종료.

설치본 `/home/smturtle2/.local/bin/codexy` 갱신 완료.
기존 실행 중 서버는 재시작하지 않았으므로, 그 프로세스에 적용하려면 종료 후
`codexy serve`로 다시 시작해야 합니다.
실제 토큰 갱신·재로그인과 캐시 100회 검사는 이번 범위에서 반복하지 않았습니다.
