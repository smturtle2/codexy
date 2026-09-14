# codexy

Rust로 작성한 로컬 ChatGPT/Codex 구독 인증 프록시. EUPL 1.2.
Codex CLI 설치 없이 브라우저 OAuth 로그인을 사용합니다.

현재 상태: 초기 구현 및 모의 백엔드 검증 완료. 로그인된 계정에서 gpt-5.6-luna의
두 API 텍스트·함수 호출·스트리밍과 실제 캐시 적중 비교도 통과했습니다.
실제 토큰 갱신은 아직 검증하지 않았습니다.
Codex Backend는 공식 제3자 API가 아니며 변경될 수 있습니다.

## 실행

```sh
cargo build --release --locked
./target/release/codexy login
./target/release/codexy serve
```

`serve`는 포그라운드에서 실행합니다. 별도 터미널에서 `curl`로 확인하고
종료할 때는 서버 터미널에서 `Ctrl+C`를 누르세요.

OAuth 로그인은 브라우저를 열고 `http://localhost:1455/auth/callback`에서
최대 5분 기다립니다. 해당 포트가 사용 중이면 먼저 기존 로그인 작업을 종료하세요.
로그인 URL을 터미널에도 표시합니다. 토큰·요청·응답 본문은 로그에 기록하지 않습니다.

```sh
./target/release/codexy status
./target/release/codexy logout
```

설정·인증 디렉터리는 `$CODEXY_HOME`, `$XDG_CONFIG_HOME/codexy`,
`$HOME/.config/codexy` 순서로 선택합니다. `auth.json`은 Unix에서 디렉터리 0700,
파일 0600으로 저장하고 임시 파일을 원자적으로 교체합니다. 토큰 파일 자체는
암호화하지 않습니다. 로그인 상태의 서버도 이후 요청부터 logout을 감지합니다.
이미 진행 중인 요청은 logout으로 취소되지 않습니다.
갱신 병합은 한 serve 프로세스 안에서 적용되므로 같은 인증 디렉터리를 사용하는
serve 프로세스는 하나만 실행하세요.

## 설정

`config.example.toml`을 설정 디렉터리의 `config.toml`로 복사하거나
`codexy --config /경로/config.toml serve`로 지정합니다.
기본 주소는 `127.0.0.1:8787`이며 로컬 클라이언트 인증을 사용하지 않습니다.
외부 주소에 바인딩한다면 TLS·접근 제어를 제공하는 역방향 프록시 뒤에서 운영하세요.

모델 이름은 자동 탐색하지 않습니다. `default_model`과 `[models]` 별칭을
자신의 계정에서 사용할 수 있는 모델로 설정하세요. GET `/v1/models`가 반환하는
목록도 이 설정에 따릅니다. 예제 모델의 계정별 사용 가능 여부는 미검증입니다.
`timeout_seconds`는 백엔드 요청 전체 제한, `max_concurrent_requests`는
동시에 처리하는 요청 수입니다. 한도 초과 요청은 즉시 429로 반환합니다.
요청 본문은 8 MiB, 개별 SSE 이벤트는 8 MiB, 비스트리밍 수집은 64 MiB로 제한합니다.

## API

모든 로컬 API 엔드포인트는 별도 클라이언트 인증 없이 사용할 수 있습니다.

```sh
curl http://127.0.0.1:8787/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"model":"codex","input":"안녕","prompt_cache_key":"my-session"}'

curl -N http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"codex","messages":[{"role":"user","content":"안녕"}],"stream":true,"stream_options":{"include_usage":true}}'
```

`codex` 별칭은 예제 설정을 설치했을 때 사용할 수 있습니다.
Responses와 Chat Completions 모두 텍스트·함수 호출 및 스트리밍/비스트리밍을
처리합니다. Chat의 이미지·오디오 콘텐츠는 400으로 거절합니다.
Responses 입력 항목은 서버 `id`를 제외하고 보존합니다.
Chat의 시스템 메시지를 합치거나 임의의 시스템 지침을 추가하지 않습니다.
함수 호출의 인자 문자열과 call ID는 유지합니다.

백엔드 송신에는 OAuth 토큰과 `ChatGPT-Account-Id`만 인증에 사용하며,
수신한 로컬 요청 인증 정보는 전달하지 않습니다. `store:false`, `stream:true`,
`include:["reasoning.encrypted_content"]`를 적용하며 기존 include도 보존합니다.
암호화 reasoning은 Responses 객체에 보존하지만 Chat 답변 텍스트로 노출하지 않습니다.

캐시 키는 본문 `prompt_cache_key`를 우선하고 없으면 수신 `session-id`를 사용합니다.
선택한 값을 백엔드 헤더와 본문에 동일하게 넣으며 둘 다 없으면 생성하지 않습니다.
Chat 캐시 사용량은 `usage.prompt_tokens_details.cached_tokens`로 매핑합니다.
캐시 적중 자체는 백엔드 정책에 달려 있습니다.

토큰 상한 `max_tokens`, `max_completion_tokens`, `max_output_tokens`는 제거하고
정상 응답의 `x-codexy-ignored-params`에 나열합니다. `n > 1`, `store:true`,
`previous_response_id`, `conversation`, `item_reference`는 400
`unsupported_parameter`입니다. 백엔드 HTTP 오류는 상태와 본문을 전달합니다.
Responses의 문자열 `input`은 원문을 보존한 `user/input_text` 배열로 변환합니다.
로컬 요청 오류는 `error.message/type/code/param` JSON 객체로 반환합니다.
잘못된 JSON은 400, JSON Content-Type 누락·불일치는 415, 8 MiB 초과는 413입니다.
미지원 파라미터는 `type=invalid_request_error`, `code=unsupported_parameter`입니다.
백엔드 401은 토큰 갱신 후 한 번만 재요청합니다. 스트리밍 시작 후 재시도하지 않습니다.
완료 이벤트 없는 단절은 비스트리밍에서 502, 스트리밍에서는 본문 전송 오류입니다.
클라이언트 취소는 백엔드 연결도 중단합니다.

## 설치·배포

현재 소스에서 로컬 설치:

```sh
cargo install --path . --locked
```

`v*` 태그용 GitHub Actions가 Linux/macOS × x86_64/aarch64 바이너리와
SHA-256 파일을 생성합니다. 최신 릴리스는 아래 명령으로 설치합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/smturtle2/codexy/main/scripts/install.sh | sh
```

설치 스크립트는 최신 GitHub Release를 다운로드하며 같은 명령으로 업데이트합니다.
`CODEXY_INSTALL_DIR`의 기본값은 `$HOME/.local/bin`입니다. 해당 경로를 PATH에
추가하세요. 설정·인증 파일은 변경하지 않고 검증 실패 시 기존 실행 파일을 보존합니다.
Linux 배포본은 GNU libc를 사용합니다. CI 환경보다 오래된 libc 호환성은 미검증입니다.

## 검증과 구현 근거

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
sh tests/install.sh
```

범위별 결과와 남은 실계정 검증은 [docs/validation.md](docs/validation.md)를 참고하세요.
요구사항은 첨부 개발요청서를 참고해 구현했고, API 매핑은
[OpenAI Responses 마이그레이션 문서](https://developers.openai.com/api/docs/guides/migrate-to-responses),
OAuth 흐름은 [Codex 로그인 구현](https://github.com/openai/codex/blob/main/codex-rs/login/src/server.rs),
캐시 라우팅은 [Codex 클라이언트](https://github.com/openai/codex/blob/main/codex-rs/core/src/client.rs)를
참조했습니다. 외부 프로젝트의 소스 코드를 제품에 복사하지 않았습니다.

라이선스 전문은 [LICENSE](LICENSE)에 있습니다.
