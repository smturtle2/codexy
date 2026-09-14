use codexy::{
    config::Config,
    mapping::{completion, prepare},
};
use serde_json::{Value, json};

#[test]
fn prepare_cache_key_precedence_and_limits() {
    let config = Config::default();

    let prepared = prepare(
        json!({
            "model": "gpt-test",
            "prompt_cache_key": "body-key",
            "max_tokens": 10,
            "max_completion_tokens": 20,
            "max_output_tokens": 30,
            "n": 1
        }),
        false,
        Some("session-key"),
        &config,
    )
    .unwrap();
    assert_eq!(prepared.cache_key.as_deref(), Some("body-key"));
    assert_eq!(prepared.body["prompt_cache_key"], "body-key");
    assert_eq!(
        prepared.ignored,
        vec!["max_tokens", "max_completion_tokens", "max_output_tokens"]
    );
    assert!(prepared.body.get("n").is_none());
    assert!(prepared.body.get("max_tokens").is_none());

    let prepared = prepare(
        json!({"model": "gpt-test"}),
        false,
        Some("session-key"),
        &config,
    )
    .unwrap();
    assert_eq!(prepared.cache_key.as_deref(), Some("session-key"));
    assert_eq!(prepared.body["prompt_cache_key"], "session-key");

    let prepared = prepare(json!({"model": "gpt-test"}), false, None, &config).unwrap();
    assert_eq!(prepared.cache_key, None);
    assert!(prepared.body.get("prompt_cache_key").is_none());

    for request in [
        json!({"previous_response_id": "resp_1"}),
        json!({"conversation": "conv_1"}),
        json!({"store": true}),
        json!({"n": 2}),
    ] {
        assert!(prepare(request, false, None, &config).is_err());
    }
}

#[test]
fn prepare_normalizes_response_input_and_rejects_invalid_shapes() {
    let config = Config::default();
    let text = "  안녕\n世界  ";
    let from_string = prepare(json!({"input": text}), false, None, &config)
        .unwrap()
        .body;
    assert_eq!(
        from_string["input"],
        json!([{"role":"user","content":[{"type":"input_text","text":text}]}])
    );

    let from_array = prepare(
        json!({"input":[{"role":"user","content":[{"type":"input_text","text":text}]}]}),
        false,
        None,
        &config,
    )
    .unwrap()
    .body;
    assert_eq!(from_array, from_string);
    assert_eq!(
        prepare(json!({"input":text}), false, None, &config)
            .unwrap()
            .body,
        from_string
    );

    let with_body_key = prepare(
        json!({"input":text,"prompt_cache_key":"body-key"}),
        false,
        Some("session-key"),
        &config,
    )
    .unwrap();
    assert_eq!(with_body_key.cache_key.as_deref(), Some("body-key"));
    assert_eq!(with_body_key.body["prompt_cache_key"], "body-key");

    for input in [Value::Null, json!(42), json!({"role":"user"})] {
        let error = prepare(json!({"input":input}), false, None, &config)
            .err()
            .unwrap();
        assert_eq!(error.kind, codexy::mapping::RequestErrorKind::Invalid);
        assert_eq!(error.param.as_deref(), Some("input"));
    }
}

#[test]
fn prepare_chat_tools_and_message_order() {
    let request = json!({
        "model": "gpt-test",
        "messages": [
            {"role": "user", "content": "Find a result"},
            {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}]},
            {"role": "tool", "tool_call_id": "call_1", "content": "result"}
        ],
        "tools": [{"type": "function", "function": {"name": "lookup", "description": "Find it", "parameters": {"type": "object"}}}],
        "tool_choice": {"type": "function", "function": {"name": "lookup"}}
    });
    let body = prepare(request, true, None, &Config::default())
        .unwrap()
        .body;

    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "lookup");
    assert_eq!(body["tools"][0]["strict"], false);
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": "lookup"})
    );
    assert_eq!(
        body["input"][0],
        json!({"role": "user", "content": [{"type": "input_text", "text": "Find a result"}]})
    );
    assert_eq!(
        body["input"][1],
        json!({"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"q\":\"x\"}"})
    );
    assert_eq!(
        body["input"][2],
        json!({"type": "function_call_output", "call_id": "call_1", "output": "result"})
    );
}

#[test]
fn prepare_strips_input_ids_and_preserves_response_metadata() {
    let body = prepare(
        json!({
            "model": "gpt-test",
            "input": [
                {"type": "message", "id": "item_1", "call_id": "call_1", "role": "user", "phase": "final", "summary": ["kept"], "encrypted_content": "cipher"}
            ]
        }),
        false,
        None,
        &Config::default(),
    )
    .unwrap()
    .body;
    let item = &body["input"][0];
    assert!(item.get("id").is_none());
    assert_eq!(item["call_id"], "call_1");
    assert_eq!(item["phase"], "final");
    assert_eq!(item["summary"], json!(["kept"]));
    assert_eq!(item["encrypted_content"], "cipher");
}

#[test]
fn completion_maps_usage_and_hides_reasoning() {
    let result = completion(&json!({
        "id": "resp_1",
        "created_at": 123,
        "model": "gpt-test",
        "status": "completed",
        "output": [
            {"type": "reasoning", "summary": [{"type": "summary_text", "text": "secret"}]},
            {"type": "message", "content": [{"type": "output_text", "text": "hello"}]}
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 4,
            "total_tokens": 14,
            "input_tokens_details": {"cached_tokens": 7},
            "output_tokens_details": {"reasoning_tokens": 2}
        }
    }));
    assert_eq!(result["choices"][0]["message"]["content"], "hello");
    assert!(result["choices"][0]["message"].get("reasoning").is_none());
    assert_eq!(result["usage"]["prompt_tokens_details"]["cached_tokens"], 7);
    assert_eq!(
        result["usage"]["completion_tokens_details"]["reasoning_tokens"],
        2
    );
}
