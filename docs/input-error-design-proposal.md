# 문자열 입력·JSON 오류 처리 설계 제안

상태: 사용자 승인 후 구현 완료. 최종 검증은 [수정 검증 결과](input-error-fix-validation.md) 참조.
아래는 구현 전 조사·제안 기록입니다.

## 권장안

입력의 의미를 바꾸는 작업은 mapping에, HTTP 파싱과 오류 응답은 작은 공통 API
모듈에 둡니다. 새 의존성이나 프레임워크 교체 없이 현재 Axum 0.8.9로 해결합니다.

| 책임 | 담당 위치 | 변경 |
| --- | --- | --- |
| 입력 형태 정규화 | src/mapping.rs | 문자열 input을 user/input_text 배열로 변환 |
| 요청 검증 실패 표현 | src/mapping.rs의 RequestError | invalid와 unsupported를 값으로 구분 |
| JSON 요청 추출 | 신규 src/api.rs의 ApiJson | Axum Json<Value>에 위임하고 파싱 실패를 ApiError로 변환 |
| 로컬 API 오류 직렬화 | src/api.rs의 ApiError | 공통 JSON error 객체 생성 |
| 라우팅·백엔드 호출 | src/proxy.rs | ApiJson 사용, RequestError→ApiError 연결 |

## 1. 입력 정규화

OpenAI [Responses 규격](https://developers.openai.com/api/reference/cli/resources/responses/methods/create)은
input에 문자열과 입력 항목 배열을 허용합니다. 실제 Codex Backend는 문자열을
`Input must be a list`로 거절했습니다. 이 차이는 프록시의 요청 변환 계층이 담당합니다.

```json
{"input":"안녕"}
```

변환 결과의 input:

```json
[{"role":"user","content":[{"type":"input_text","text":"안녕"}]}]
```

- 공통 normalize_input 단계에서 문자열을 한 번 변환합니다.
- Chat messages 변환 뒤 생성되는 input도 같은 배열 처리 경로를 통과합니다.
- 문자열을 trim하거나 Unicode 정규화하지 않습니다. 공백·개행·문자열 내용 보존.
- 기존 배열은 항목 순서·콘텐츠·툴 호출·reasoning 필드를 보존하고 현재의 서버 id 제거 규칙을 적용합니다.
- input이 존재하지만 문자열/배열이 아닌 경우는 param=input인 요청 오류로 처리합니다.
- input 생략 여부는 이번 수정에서 별도 필수 조건으로 추가하지 않습니다.
- 시간·새 ID·임의 지침을 추가하지 않으므로 같은 입력은 같은 송신 본문이 됩니다.

전체 Responses 스키마를 새 Rust DTO로 복제하지 않습니다. 현재 Value 기반 통과 방식은
백엔드 확장 필드와 reasoning 항목을 보존하는 데 적합하고, 이번 수정은 명시적으로
지원하는 변환만 담당하면 됩니다.

## 2. 공통 HTTP JSON 추출기

두 엔드포인트의 마지막 인자 `Json<Value>`를 `ApiJson`으로 변경합니다.
ApiJson은 수동으로 본문을 읽거나 재파싱하지 않고
`Json::<Value>::from_request(req, state)`에 그대로 위임합니다.
실패한 경우만 ApiError로 변환합니다.

[Axum 공식 추출기 문서](https://docs.rs/axum/0.8.9/axum/extract/index.html#customizing-extractor-responses)는
기본 추출기를 감싸서 재사용하는 방식을 지원합니다. 현재 잠긴 버전의 소스에서도
Json → Bytes → into_limited_body 경로를 확인했습니다.
원래 요청을 그대로 전달하면 현재 8 MiB DefaultBodyLimit도 유지됩니다.

| 입력 문제 | 상태 코드 제안 | 응답 |
| --- | ---: | --- |
| 깨진 JSON | 400 | JSON error |
| 잘못되거나 없는 JSON Content-Type | 415 | JSON error |
| 본문 크기 초과 | 413 | JSON error |
| 파싱된 값의 의미·형식 오류 | 400 | JSON error, 가능한 경우 param 포함 |
| 미지원 파라미터 | 400 | JSON error, code=unsupported_parameter |

추출기 오류는 `JsonRejection.status()`를 사용해 상태를 보존합니다.
실제 Json<Value>가 거의 발생시키지 않는 JsonDataError나 향후 변형도 기본 상태를
보존하는 fallback을 둡니다. 오류 메시지는 짧고 안정적인 문구로 정하며,
본문 원문이나 자격 증명을 메시지에 넣지 않습니다.

error 객체는 message/type/code/param 필드를 한 곳에서 생성합니다.
파싱 오류에는 type=invalid_request_error, code/param은 필요 없으면 null로 둡니다.
code=unsupported_parameter는 기존 클라이언트 동작을 유지합니다.

서버가 직접 만드는 오류만 공통화합니다. 백엔드가 반환한 HTTP 오류의 상태·본문과
이미 시작한 SSE 스트림은 기존 전달 규칙을 유지합니다.

## 3. 구조화된 오류 타입

현재 mapping::prepare는 String을 반환하고 proxy.rs가
`starts_with("unsupported_parameter")`로 종류를 판별합니다.
오류 문구를 바꾸면 분류까지 바뀌는 연결을 제거합니다.

RequestError에 종류(Invalid/Unsupported), message, 선택적 param을 둡니다.
mapping은 Axum이나 HTTP 상태에 의존하지 않습니다.
API 모듈에서 RequestError를 ApiError로 변환하고 HTTP 응답을 만듭니다.
기존 로컬 오류 생성도 같은 serializer를 사용하도록 모읍니다.
이 정리는 두 문제의 오류 흐름을 한 곳으로 모으는 데 필요한 범위입니다.

## 대안 검토

| 방식 | 평가 |
| --- | --- |
| Result<Json<Value>, JsonRejection>를 공통 핸들러에 전달 | 가능하고 짧지만 백엔드 요청 처리 함수가 HTTP 추출 실패도 알아야 함 |
| ApiJson + ApiError | 권장. 파싱·오류 변환 책임이 명확하고 두 API가 같은 처리 사용 |
| 전체 응답을 검사하는 미들웨어 | SSE·백엔드 원본 오류까지 구분해야 하므로 이번 문제에 부적합 |
| 수동 JSON 파서 또는 전체 요청 DTO 재작성 | 현재 파서·크기 제한·확장 필드 처리까지 다시 책임져야 하므로 불필요 |

## 조사 중 기술 검증

`/tmp/codexy-design-spike`에서 잠긴 axum 0.8.9로 최소 ApiJson/ApiError 추출기를
컴파일하고 6가지 입력을 하나의 테스트로 검증했습니다.
제품 저장소에는 이 시험 코드를 적용하지 않았습니다.

- 깨진 JSON → 400, JSON error.
- Content-Type 없음 / text/plain → 각각 415, JSON error.
- 축소한 테스트 제한(16바이트) 초과 → 413, JSON error.
- 정상 application/json / application/vendor+json → 각각 200.
- 모든 응답 Content-Type=application/json 확인.

현재 8 MiB 제한 보존은 Axum 소스 경로를 확인했고,
축소한 제한으로 같은 위임 경로의 동작을 실행 검증했습니다.

## 구현 후 검증 제안

1. 입력 변환 묶음: 문자열·공백·개행·Unicode, 기존 배열/툴/reasoning 보존,
   잘못된 input 타입, 문자열과 동등한 배열의 정규화 결과, 반복 본문·캐시 키 동일성.
2. 두 HTTP API 묶음: JSON 파싱/Content-Type/8 MiB 제한/정상 +json,
   error 객체와 상태 코드, 거절한 요청이 백엔드에 도달하지 않음.
3. 기존 회귀 테스트와 fmt/clippy를 한 번에 실행.
4. 실제 계정: 새 문자열 입력의 스트리밍·비스트리밍 및 기존 배열 입력 확인.
   이미 통과한 캐시 100회 검사는 변경 없는 캐시 경로 때문에 자동 반복하지 않습니다.

## 변경 규모

신규 src/api.rs 한 개와 mapping.rs·proxy.rs·lib.rs, 관련 테스트를 수정합니다.
OAuth·캐시·SSE 변환 알고리즘이나 의존성은 변경하지 않습니다.
추출기 뼈대는 시험으로 검증했지만, 제품 통합과 위 회귀 검증은 아직 수행하지 않았습니다.

## OpenCode·pi 비교 조사

두 프로젝트의 Codex 송신 코드를 추가로 대조했습니다. 이번 문제에 직접 참고할
부분은 입력 변환의 위치이며, 클라이언트 내부 오류를 프록시의 HTTP 오류 규격으로
그대로 옮기지는 않습니다.

- **pi**: `buildRequestBody`가 공유 `convertResponsesMessages`를 호출하고,
  사용자 문자열을 `user` 메시지의 `input_text` 배열로 만듭니다.
  `instructions`는 별도로 설정합니다. 이는 codexy의 mapping에서 문자열 입력을
  배열로 정규화한다는 제안을 뒷받침합니다.
  [본문 생성](https://github.com/earendil-works/pi/blob/0c7bb7c5c72118e4c71e4c04dfa2ad4a0a6a62f1/packages/ai/src/api/openai-codex-responses.ts#L533),
  [메시지 변환](https://github.com/earendil-works/pi/blob/0c7bb7c5c72118e4c71e4c04dfa2ad4a0a6a62f1/packages/ai/src/api/openai-responses-shared.ts#L174).
- **OpenCode**: Codex 플러그인의 fetch 래퍼는 OAuth 갱신·헤더·송신 URL을 담당하고
  `init.body`를 그대로 전달합니다. 따라서 이 래퍼에 문자열 input 변환 로직이
  있다는 주장은 할 수 없습니다. provider는 AI SDK를 사용하며, 이번 확인 범위에서는
  SDK 내부 변환 구현까지 추가로 확인했습니다(아래 참조).
  [Codex fetch 래퍼](https://github.com/anomalyco/opencode/blob/228e9095ba3988a02664c3816cb51f98584e86c2/packages/opencode/src/plugin/openai/codex.ts#L350).
- **오류 경계**: pi의 해당 모듈은 송신 클라이언트입니다. HTTP 오류 본문은 가능한 경우
  JSON으로 해석하고, SSE 프로토콜 오류와 API 오류를 구분한 뒤 사용자 이벤트로
  바꿉니다. codexy가 받은 잘못된 HTTP JSON을 어떤 응답으로 돌려줄지는 별도 서버
  책임이므로, Axum 추출기를 감싸는 제안의 근거는 Axum 문서와 실행 검증입니다.
  [pi HTTP 오류](https://github.com/earendil-works/pi/blob/0c7bb7c5c72118e4c71e4c04dfa2ad4a0a6a62f1/packages/ai/src/api/openai-codex-responses.ts#L350),
  [pi SSE 처리](https://github.com/earendil-works/pi/blob/0c7bb7c5c72118e4c71e4c04dfa2ad4a0a6a62f1/packages/ai/src/api/openai-codex-responses.ts#L721).

pi의 대화 복구·식별자 보정·재시도 정책은 자체 대화 모델을 위한 처리입니다.
이번 두 오류를 고치는 데 필요하지 않으므로 그대로 도입하지 않습니다.
이 비교로 권장 구조는 유지하며, 제품 코드는 여전히 변경하지 않았습니다.


### OpenCode SDK 경로까지 추가 확인

고정 커밋의 `packages/opencode/package.json`은 `@ai-sdk/openai` **3.0.88**을
지정합니다. 해당 npm 배포본의 원본 TypeScript를 내려받아 확인했습니다.
OpenCode `getModel` → `sdk.responses(modelID)` → SDK의
`convertToOpenAIResponsesInput` 순서로 연결됩니다. 변환기는 빈 input 배열을
만든 뒤 user의 text 파트를 `{type:"input_text", text:part.text}`로 넣습니다.
즉 OpenCode도 송신 전에 배열을 만들며, 그 책임은 OAuth fetch 래퍼가 아니라
SDK의 입력 변환기에 있습니다.

- [OpenCode SDK 버전](https://github.com/anomalyco/opencode/blob/228e9095ba3988a02664c3816cb51f98584e86c2/packages/opencode/package.json#L70)
- [Responses 모델 선택](https://github.com/anomalyco/opencode/blob/228e9095ba3988a02664c3816cb51f98584e86c2/packages/opencode/src/provider/provider.ts#L208)
- [SDK 3.0.88 입력 변환 소스](https://unpkg.com/@ai-sdk/openai@3.0.88/src/responses/convert-to-openai-responses-input.ts)
- [SDK 3.0.88 오류 처리 소스](https://unpkg.com/@ai-sdk/openai@3.0.88/src/openai-error.ts)

SDK는 백엔드 HTTP 실패 처리에 `openaiFailedResponseHandler`를 연결하고,
`error.message`를 추출하는 JSON 오류 스키마를 사용합니다. 이 역시 송신 결과를
해석하는 클라이언트 처리이며, codexy의 수신 JSON 파싱 오류 처리와는 경계가 다릅니다.

두 프로젝트 모두 범용 Responses HTTP 요청을 그대로 받는 codexy와 인터페이스가
같지는 않습니다. 따라서 문자열 input을 직접 받아 변환하는 동일 사례로 표현하지
않고, **대화 입력을 Codex용 배열로 만드는 책임을 변환 계층에 둔 사례**로 참고합니다.
SDK 소스 조사만 수행했으며 OpenCode·pi 자체 테스트나 실제 모델 호출은 실행하지 않았습니다.
