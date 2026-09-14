# codexy 운영·API·개발 가이드

[영문 README](../README.md) · [한국어 README](../README.ko.md) · [검증 기록](validation.md)

이 문서는 codexy를 설치하고 로그인한 뒤 로컬 OpenAI 호환 API 프록시로 운영하는 방법과, 요청이 백엔드로 매핑되는 규칙을 설명합니다. codexy는 브라우저 OAuth 로그인으로 ChatGPT/Codex 구독 백엔드에 연결하며 Codex CLI를 별도로 설치하지 않습니다.

Codex Backend는 공식 제3자 API가 아니므로 동작과 응답 형식이 변경될 수 있습니다. 현재 실제 계정에서 텍스트·함수 호출·스트리밍·캐시 적중을 확인했지만, 실제 토큰 갱신은 아직 검증하지 않았습니다.

## 빠른 시작

소스 체크아웃에서 빌드하는 경우에는 [개발](#개발) 절차를 따르세요. 릴리스 바이너리를 설치한 뒤에는 다음 순서로 실행합니다.

```sh
codexy login
codexy serve
```

`serve`는 포그라운드 프로세스로 실행됩니다. 다른 터미널에서 API를 호출하고, 종료할 때는 서버 터미널에서 `Ctrl+C`를 누릅니다.

로그인은 브라우저를 열고 `http://localhost:1455/auth/callback`에서 최대 5분 동안 콜백을 기다립니다. 이 포트가 사용 중이면 기존 로그인 작업을 먼저 종료하세요. 로그인 URL은 터미널에도 출력됩니다. 토큰, 요청 본문, 응답 본문은 로그에 기록하지 않습니다.

상태와 로그아웃은 다음 명령으로 확인합니다.

```sh
codexy status
codexy logout
```

인증되지 않은 상태에서 API를 호출하면 `401 authentication_error`가 반환됩니다.

## 인증과 파일 위치

설정·인증 디렉터리는 다음 우선순위로 선택합니다.

1. `$CODEXY_HOME`
2. `$XDG_CONFIG_HOME/codexy`
3. `$HOME/.config/codexy`

이 디렉터리의 `auth.json`에 OAuth 자격 증명이 저장됩니다. Unix에서는 디렉터리를 0700, 파일을 0600 권한으로 만들고 임시 파일을 원자적으로 교체합니다. 토큰 파일 자체는 암호화하지 않으므로 운영체제 계정과 파일 권한을 보호하세요.

서버가 실행 중이어도 이후 요청마다 인증 파일을 다시 확인하므로 `logout`을 감지합니다. 이미 진행 중인 요청은 로그아웃으로 취소되지 않습니다. 토큰 갱신 결과 병합은 하나의 `serve` 프로세스 안에서만 조정되므로 같은 인증 디렉터리를 사용하는 `serve` 프로세스는 하나만 실행해야 합니다.

## 설정

`config.example.toml`을 설정 디렉터리의 `config.toml`로 복사하거나 전역 `--config` 옵션으로 파일을 지정합니다.

```sh
codexy --config /경로/config.toml serve
```

예시 설정은 다음과 같습니다.

```toml
listen = "127.0.0.1:8787"
default_model = "gpt-5.6-luna"
timeout_seconds = 300
max_concurrent_requests = 8

[models]
# 로컬 별칭 = 백엔드 모델 ID
codex = "gpt-5.6-luna"
```

주요 항목은 다음과 같습니다.

- `listen`: 수신 주소입니다. 기본값은 `127.0.0.1:8787`이며 로컬 클라이언트 인증은 제공하지 않습니다.
- `default_model`: 요청에 모델이 없을 때 사용할 모델 ID입니다. 이 값에도 `[models]` 별칭 매핑이 적용됩니다.
- `[models]`: 클라이언트가 보낼 로컬 모델 이름과 백엔드 모델 ID의 매핑입니다. 모델을 자동 탐색하지 않으므로 계정에서 실제로 사용할 수 있는 모델을 직접 설정해야 합니다.
- `timeout_seconds`: 백엔드 요청 전체 제한 시간입니다.
- `max_concurrent_requests`: 동시에 처리할 요청 수입니다. 한도를 넘은 요청은 즉시 `429 rate_limit_exceeded`를 받습니다.
- `backend_url`: 백엔드 URL입니다. 기본값은 ChatGPT Codex responses 엔드포인트이며 HTTPS 또는 로컬호스트 HTTP만 허용됩니다.

`GET /v1/models`가 반환하는 목록은 `[models]`의 별칭과 `default_model`로 구성됩니다. 예제 모델의 계정별 사용 가능 여부는 보장하지 않습니다. 요청 본문은 8 MiB, 개별 SSE 이벤트는 8 MiB, 비스트리밍으로 조립하는 백엔드 응답은 64 MiB로 제한됩니다.

기본 주소 외부에 바인딩하려면 TLS와 접근 제어를 제공하는 역방향 프록시 뒤에 배치하세요. codexy 자체에는 로컬 API 클라이언트 인증이 없습니다.

## HTTP API

모든 로컬 엔드포인트는 별도 클라이언트 인증 없이 사용할 수 있습니다.

### 상태 확인

```sh
curl http://127.0.0.1:8787/health
curl http://127.0.0.1:8787/v1/models
```

`/health`는 `{"status":"ok"}`를 반환합니다. `/v1/models`는 OpenAI 형식의 모델 목록을 반환하며, 모델 ID는 설정한 별칭과 기본 모델입니다.

### Responses

모델을 생략하면 설정의 `default_model`을 사용합니다.

```sh
curl http://127.0.0.1:8787/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"input":"안녕","prompt_cache_key":"my-session"}'
```

스트리밍 요청은 `stream: true`를 사용합니다.

```sh
curl -N http://127.0.0.1:8787/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"input":"안녕","stream":true}'
```

### Chat Completions

```sh
curl -N http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"안녕"}],"stream":true,"stream_options":{"include_usage":true}}'
```

Responses와 Chat Completions 모두 텍스트, 함수 호출, 스트리밍과 비스트리밍을 지원합니다. Chat의 이미지·오디오 콘텐츠는 `400`으로 거절합니다. Responses 입력 항목은 서버 `id`를 제외하고 보존합니다. Chat의 시스템 메시지는 합치거나 임의의 시스템 지침을 추가하지 않습니다. 함수 호출의 인자 문자열과 call ID도 유지합니다.

## 요청 매핑과 캐시

백엔드로 전송할 때는 OAuth access token과 `ChatGPT-Account-Id`만 인증에 사용합니다. 로컬 요청에 포함된 인증 정보는 백엔드로 전달하지 않습니다. 모든 요청에는 `store:false`, `stream:true`, `include:["reasoning.encrypted_content"]`를 적용하며, 기존 `include` 항목은 보존합니다. 암호화된 reasoning은 Responses 객체에 보존하지만 Chat 답변 텍스트로 노출하지 않습니다.

캐시 키는 다음 순서로 선택합니다.

1. JSON 본문의 `prompt_cache_key`
2. 수신 요청의 `session-id` 헤더
3. 둘 다 없으면 캐시 키를 만들지 않음

선택한 값은 백엔드 요청의 `session-id` 헤더와 JSON `prompt_cache_key`에 동일하게 넣습니다. Chat 캐시 사용량은 `usage.prompt_tokens_details.cached_tokens`로 매핑됩니다. 캐시 적중 여부 자체는 백엔드 정책에 따릅니다.

`max_tokens`, `max_completion_tokens`, `max_output_tokens`는 백엔드로 보내지 않고 정상 응답의 `x-codexy-ignored-params` 헤더에 이름을 나열합니다. `n > 1`, `store:true`, `previous_response_id`, `conversation`, `item_reference`는 `400 unsupported_parameter`입니다.

## 오류와 재시도 동작

로컬 요청 오류는 `error.message`, `error.type`, `error.code`, `error.param` 필드를 가진 JSON 객체입니다.

- 잘못된 JSON: `400`
- JSON Content-Type이 없거나 지원하지 않는 형식: `415` (`application/json`과 `application/*+json` 허용)
- 요청 본문이 8 MiB 초과: `413`
- 미지원 파라미터: `type=invalid_request_error`, `code=unsupported_parameter`
- 동시 처리 한도 초과: `429 rate_limit_exceeded`
- 인증 파일이 없거나 토큰 갱신 실패: `401 authentication_error`
- 백엔드 연결·타임아웃 실패: `502 upstream_error`

백엔드 HTTP 오류는 백엔드 상태 코드와 본문을 전달합니다. 백엔드가 `401`을 반환하면 토큰을 갱신한 뒤 한 번만 재요청합니다. 스트리밍이 시작된 뒤에는 재시도하지 않습니다. 완료 이벤트 없이 연결이 끊기면 비스트리밍 요청은 `502`를 반환하고, 스트리밍 요청은 이미 전송된 본문에서 오류가 발생할 수 있습니다. 클라이언트가 연결을 취소하면 백엔드 연결도 중단됩니다.

Responses의 문자열 `input`은 원문을 보존하는 `user`/`input_text` 배열로 변환됩니다. 백엔드 응답의 `response.completed` 또는 `response.incomplete`에서 비스트리밍 응답을 완성하며, Chat 요청은 이를 Chat Completions 형식으로 변환합니다.

## 설치와 배포

최신 GitHub Release는 다음 스크립트로 설치하거나 업데이트할 수 있습니다.

```sh
curl -fsSL https://raw.githubusercontent.com/smturtle2/codexy/main/scripts/install.sh | sh
```

스크립트는 최신 릴리스의 플랫폼별 바이너리와 SHA-256 파일을 내려받습니다. `CODEXY_INSTALL_DIR`의 기본값은 `$HOME/.local/bin`이므로 해당 경로를 `PATH`에 추가하세요. 설정·인증 파일은 변경하지 않으며 검증에 실패하면 기존 실행 파일을 보존합니다. 릴리스는 Linux/macOS의 x86_64/aarch64를 대상으로 합니다. Linux 바이너리는 GNU libc를 사용하므로 CI 환경보다 오래된 libc와의 호환성은 검증되지 않았습니다.

## 개발

소스에서 로컬 설치하려면 Rust 도구체인과 Cargo가 필요합니다.

```sh
cargo install --path . --locked
```

개발 중 직접 빌드하고 실행할 때는 다음과 같이 합니다.

```sh
cargo build --release --locked
./target/release/codexy login
./target/release/codexy serve
```

변경 전후 검증 명령은 다음과 같습니다.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
sh tests/install.sh
```

범위별 결과와 남은 실계정 검증 항목은 [검증 기록](validation.md)에서 확인할 수 있습니다. 구현은 [OpenAI Responses 마이그레이션 문서](https://developers.openai.com/api/docs/guides/migrate-to-responses), [Codex 로그인 구현](https://github.com/openai/codex/blob/main/codex-rs/login/src/server.rs), [Codex 캐시 라우팅](https://github.com/openai/codex/blob/main/codex-rs/core/src/client.rs)을 참고했습니다. 외부 프로젝트의 소스 코드를 제품에 복사하지 않았습니다.

## 라이선스

라이선스 전문은 [LICENSE](../LICENSE)에서 확인할 수 있습니다. 추가 고지는 [NOTICE](../NOTICE)를 참고하세요.
