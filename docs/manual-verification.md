# 설치 후 직접 검증

설치 위치: `/home/smturtle2/.local/bin/codexy` (0.1.0)
아래 명령은 두 터미널에서 실행합니다.

## 1. 로그인

```sh
codexy --version
codexy login
codexy status
```

브라우저에서 로그인 후 터미널의 `Logged in.`을 확인합니다.

## 2. 서버 시작

```sh
codexy serve
```

`codexy listening on 127.0.0.1:8787`이 출력된 다음 진행합니다.
서버는 포그라운드에서 실행하고, 검증이 끝나면 서버 터미널에서 `Ctrl+C`로 종료합니다.

## 3. 상태·모델

```sh
curl -fsS http://127.0.0.1:8787/health
curl -fsS http://127.0.0.1:8787/v1/models
```

예상: `{"status":"ok"}` 및 설정된 모델 목록.

## 4. Responses 비스트리밍

```sh
curl -sS -i http://127.0.0.1:8787/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"input":[{"role":"user","content":[{"type":"input_text","text":"한 문장으로 인사해."}]}],"prompt_cache_key":"codexy-manual-check","max_output_tokens":64}'
```

예상: HTTP 200, Responses 객체와 usage.
`x-codexy-ignored-params: max_output_tokens`도 확인합니다.

## 5. Chat 스트리밍

```sh
curl -sS -N http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"한 문장으로 인사해."}],"stream":true,"stream_options":{"include_usage":true}}'
```

예상: `chat.completion.chunk` 텍스트 델타, usage 청크, 마지막 `data: [DONE]`.

## 6. 함수 호출

```sh
curl -sS -i http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"서울 날씨를 조회해."}],"tools":[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],"tool_choice":{"type":"function","function":{"name":"get_weather"}}}'
```

예상: `message.tool_calls`에 `get_weather`와 인자 문자열,
`finish_reason: "tool_calls"`. 프록시 자체는 함수를 실행하지 않습니다.

기본 모델은 gpt-5.6-luna입니다. 계정에서 거절하면 요청 본문에 사용할 수 있는
`model`을 명시하거나 설정 파일의 default_model을 변경하세요.
짧은 인사 프롬프트는 캐시 적중 검증용이 아닙니다.

## 7. 종료

서버를 실행한 터미널에서 `Ctrl+C`를 누릅니다.

인증 파일까지 삭제하려면 추가로 `codexy logout`을 실행합니다.
오류 제보에는 HTTP 상태와 오류 메시지만 포함하고 OAuth 토큰은 제외하세요.
