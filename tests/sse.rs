use codexy::sse::{ChatStream, Decoder};
use serde_json::{Value, json};
#[test]
fn function_stream_preserves_ids_arguments_and_finish_reason() {
    let mut converter = ChatStream::default();
    let events = [
        json!({"type":"response.created","response":{"id":"r","model":"m","created_at":5}}),
        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","call_id":"call_abc","name":"lookup","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"delta":"{\"x\":"}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"delta":"1}"}),
        json!({"type":"response.completed","response":{"id":"r","model":"m","created_at":5,"status":"completed","output":[],"usage":{"input_tokens":20,"output_tokens":3,"total_tokens":23,"input_tokens_details":{"cached_tokens":10}}}}),
    ];
    let text = events
        .iter()
        .flat_map(|e| converter.event(e, true))
        .collect::<String>();
    let chunks: Vec<Value> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();
    assert_eq!(
        chunks[1]["choices"][0]["delta"]["tool_calls"][0]["id"],
        "call_abc"
    );
    let args = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str())
        .collect::<String>();
    assert_eq!(args, "{\"x\":1}");
    assert_eq!(chunks[4]["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        chunks[5]["usage"]["prompt_tokens_details"]["cached_tokens"],
        10
    );
    assert!(text.ends_with("data: [DONE]\n\n"));
    let mut decoder = Decoder::default();
    let mut result = Vec::new();
    for b in "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\r\ndata: \"delta\":\"한\"}\r\n\r\n".as_bytes() { result.extend(decoder.push(&[*b]).unwrap()); }
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1["delta"], "한");
}
