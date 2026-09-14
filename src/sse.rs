use serde_json::{Value, json};
use std::collections::HashMap;

/// Some Codex backends omit output from the terminal response. Completed items
/// carry the authoritative content and are ordered by their output index.
#[derive(Default)]
pub struct ResponseAssembler {
    items: std::collections::BTreeMap<u64, Value>,
}
impl ResponseAssembler {
    pub fn observe(&mut self, event: &Value) {
        if event["type"] == "response.output_item.done"
            && let Some(index) = event["output_index"].as_u64()
            && event["item"].is_object()
        {
            self.items.insert(index, event["item"].clone());
        }
    }
    pub fn finish(self, mut response: Value) -> Value {
        if response
            .get("output")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
            && !self.items.is_empty()
        {
            response["output"] = json!(self.items.into_values().collect::<Vec<_>>());
        }
        response
    }
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}
impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<(String, Value)>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut result = Vec::new();
        loop {
            let end = self
                .buffer
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|n| (n, 2));
            let crlf = self
                .buffer
                .windows(4)
                .position(|v| v == b"\r\n\r\n")
                .map(|n| (n, 4));
            let Some((n, len)) = end.into_iter().chain(crlf).min_by_key(|v| v.0) else {
                break;
            };
            let frame: Vec<_> = self.buffer.drain(..n + len).collect();
            let raw = String::from_utf8(frame).map_err(|_| "invalid SSE UTF-8")?;
            let data = raw
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|s| s.strip_prefix(' ').unwrap_or(s))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let v = serde_json::from_str(&data).map_err(|_| "invalid SSE JSON")?;
            result.push((raw, v));
        }
        if self.buffer.len() > 8 * 1024 * 1024 {
            return Err("SSE event exceeds 8 MiB".into());
        }
        Ok(result)
    }
}
pub fn encode(v: &Value) -> String {
    format!("data: {v}\n\n")
}
#[derive(Default)]
pub struct ChatStream {
    id: Value,
    model: Value,
    created: Value,
    calls: HashMap<u64, usize>,
    role_sent: bool,
}
impl ChatStream {
    fn chunk(&self, delta: Value, reason: Value) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason}]})
    }
    pub fn event(&mut self, event: &Value, include_usage: bool) -> Vec<String> {
        let mut chunks = Vec::new();
        if let Some(response) = event.get("response") {
            self.id = response["id"].clone();
            self.model = response["model"].clone();
            self.created = response["created_at"].clone();
        }
        if !self.role_sent {
            self.role_sent = true;
            chunks.push(encode(
                &self.chunk(json!({"role":"assistant"}), Value::Null),
            ));
        }
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => chunks.push(encode(&self.chunk(json!({"content":event["delta"]}), Value::Null))),
            "response.output_item.added" if event["item"]["type"] == "function_call" => {
                let index = self.calls.len(); self.calls.insert(event["output_index"].as_u64().unwrap_or(0), index);
                chunks.push(encode(&self.chunk(json!({"tool_calls":[{"index":index,"id":event["item"]["call_id"],"type":"function","function":{"name":event["item"]["name"],"arguments":event["item"]["arguments"].as_str().unwrap_or("")}}]}), Value::Null)));
            }
            "response.function_call_arguments.delta" => {
                if let Some(index) = self.calls.get(&event["output_index"].as_u64().unwrap_or(0)) {
                    chunks.push(encode(&self.chunk(json!({"tool_calls":[{"index":index,"function":{"arguments":event["delta"]}}]}), Value::Null)));
                }
            }
            "response.completed" | "response.incomplete" => {
                let final_response = crate::mapping::completion(&event["response"]);
                let reason = if event["type"] == "response.completed" && !self.calls.is_empty() { json!("tool_calls") } else { final_response["choices"][0]["finish_reason"].clone() };
                chunks.push(encode(&self.chunk(json!({}), reason)));
                if include_usage { let mut usage = self.chunk(json!({}), Value::Null); usage["choices"] = json!([]); usage["usage"] = crate::mapping::usage(&event["response"]["usage"]); chunks.push(encode(&usage)); }
                chunks.push("data: [DONE]\n\n".into());
            }
            "response.failed" | "error" => chunks.push(encode(&json!({"error":event.get("error").or_else(||event["response"].get("error")).cloned().unwrap_or(json!({"message":"backend failed"}))}))),
            _ => ()
        }
        chunks
    }
}
