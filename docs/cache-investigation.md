# 캐시 미적중 원인 조사

2026-09-14 17:07 KST, gpt-5.6-luna. 프록시 코드는 변경하지 않았습니다.

## 확인한 결론

**프록시 없이 Codex Backend에 직접 호출해도 같은 미적중이 발생합니다.**
따라서 이 현상이 codexy의 요청 변환 때문에만 발생한다고 볼 수 없습니다.
백엔드에서 보고되는 일시적인 캐시 미적중으로 범위를 좁혔습니다.

다만 최초 프록시 미적중 한 건의 내부 원인을 특정한 것은 아닙니다.
캐시를 가진 머신으로 배정되지 않았는지, 캐시 가용 상태가 바뀌었는지 등
서버 내부 사유는 관측할 수 없었습니다. 이 구분 없이 원인을 확정해서는 안 됩니다.

## 통제 실험

동일한 4,574 입력 토큰, 요청 본문, session-id, prompt_cache_key를 유지했습니다.
직접 호출과 프록시 호출을 번갈아 실행했습니다.

| 조건 | 직접 호출 | 프록시 호출 |
| --- | --- | --- |
| 6초 간격, 총 6회 | 3/3 적중 | 3/3 적중 |
| 연속 호출, 총 12회 | 5/6 적중, 1회 미적중 | 6/6 적중 |
| 합계 | 8/9 적중 | 9/9 적중 |

모든 적중은 cached_tokens=3840이었습니다. 연속 호출 구간에서만 한 번
미적중했지만 표본이 작고 배정 머신을 관측할 수 없으므로 호출 빈도가 원인이라고
확정하지 않습니다.

모든 요청의 SHA-256:
`c8d5db6d234aeb49585bd3c4cb9a47b7d4b7613fa4b74a02006d1661c2ee5cc3`

동일하게 확인한 반환값:
- model: gpt-5.6-luna
- service_tier: default
- prompt_cache_key: codexy-live-cache-validation-20260914
- reasoning: medium / all_turns / standard
- text: text format / medium verbosity
- prompt_cache_retention: 24h

별도 로컬 백엔드 캡처에서 직접 호출용 본문과 설치된 프록시가 송신하는 본문이
바이트 단위로 같은 것도 확인했습니다. 키 우선순위·헤더 fallback·키 없음 유지도
검증했습니다.

## 직접 호출 미적중 증거

- 시각: 2026-09-14T08:07:40Z (17:07:40 KST)
- HTTP 200, input_tokens=4574, cached_tokens=0
- response ID: resp_0312bd7a6e312fab016aa7ab4cb0a887d09b939b8920017e1d
- CF-Ray: a3ade63bfe4fea13-ICN
- 응답 시간: 1.602초
- 전후 동일 요청은 모두 cached_tokens=3840

전체 진단 메타데이터는 `cache-investigation-results.json`에 보존했습니다.
OAuth 토큰이나 응답 텍스트는 포함하지 않습니다.

## 진단 한계와 공식 근거

OpenAI의 [캐시 문서](https://developers.openai.com/api/docs/guides/prompt-caching#cache-location)는
캐시가 개별 머신에 있고 배정이 머신 부하·가용 용량 등의 영향을 받는다고 설명합니다.
이는 관측 결과와 일치하는 **가능한 설명**이며, 이 건의 확정된 내부 원인은 아닙니다.
공식 API 문서가 Codex 구독 백엔드의 내부 구현까지 보장하지도 않습니다.

[캐시 진단 옵션](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics)도
시도했습니다. 하지만 현재 구독 백엔드는
`prompt_cache_options.comparison_response_id`에 HTTP 400,
`Unsupported parameter: prompt_cache_options`를 반환했습니다.
따라서 서버가 분류한 미적중 이유를 받을 수 없었습니다.

## 제안

현재 증거로 캐시 변환 코드를 수정할 이유는 없습니다. 정확한 서버 내부 원인까지
필요하다면 위 시각·response ID·CF-Ray를 기준으로 백엔드 운영 측 추적이 필요합니다.
추가 로컬 반복 실험만으로 최초 사건의 내부 원인을 증명할 수는 없습니다.
