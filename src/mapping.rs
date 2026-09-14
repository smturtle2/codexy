use crate::config::Config;
use serde_json::{Value, json};

pub struct Prepared {
    pub body: Value,
    pub cache_key: Option<String>,
    pub ignored: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestErrorKind {
    Invalid,
    Unsupported,
}

#[derive(Debug)]
pub struct RequestError {
    pub kind: RequestErrorKind,
    pub message: String,
    pub param: Option<String>,
}
impl RequestError {
    fn invalid(message: impl Into<String>, param: Option<&str>) -> Self {
        Self {
            kind: RequestErrorKind::Invalid,
            message: message.into(),
            param: param.map(str::to_owned),
        }
    }
}
impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RequestError {}
fn error(name: &str) -> RequestError {
    RequestError {
        kind: RequestErrorKind::Unsupported,
        message: format!("unsupported_parameter: {name}"),
        param: Some(name.into()),
    }
}
pub fn prepare(
    mut v: Value,
    chat: bool,
    session: Option<&str>,
    config: &Config,
) -> Result<Prepared, RequestError> {
    let obj = v
        .as_object_mut()
        .ok_or_else(|| RequestError::invalid("request must be an object", None))?;
    for key in ["stream", "store"] {
        if obj.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(RequestError::invalid(
                format!("{key} must be a boolean"),
                Some(key),
            ));
        }
    }
    if obj.get("model").is_some_and(|v| !v.is_string()) {
        return Err(RequestError::invalid(
            "model must be a string",
            Some("model"),
        ));
    }
    for key in ["previous_response_id", "conversation"] {
        if obj.contains_key(key) {
            return Err(error(key));
        }
    }
    if obj.get("store") == Some(&json!(true)) {
        return Err(error("store"));
    }
    if let Some(n) = obj.get("n") {
        let n = n
            .as_u64()
            .ok_or_else(|| RequestError::invalid("n must be a positive integer", Some("n")))?;
        if n > 1 {
            return Err(error("n"));
        }
        if n == 0 {
            return Err(RequestError::invalid("n must be positive", Some("n")));
        }
    }
    let mut ignored = Vec::new();
    for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        if obj.remove(key).is_some() {
            ignored.push(key.into());
        }
    }
    obj.remove("n");
    obj.remove("stream_options");
    if chat {
        let messages = obj
            .remove("messages")
            .ok_or_else(|| RequestError::invalid("messages is required", Some("messages")))?;
        let mut input = Vec::new();
        for message in messages
            .as_array()
            .ok_or_else(|| RequestError::invalid("messages must be an array", Some("messages")))?
        {
            let role = message["role"].as_str().ok_or_else(|| {
                RequestError::invalid("message role is required", Some("messages"))
            })?;
            if role == "tool" {
                input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"].as_str().ok_or_else(|| RequestError::invalid("tool_call_id required", Some("messages")))?,"output":message["content"]}));
                continue;
            }
            if !matches!(role, "system" | "developer" | "user" | "assistant") {
                return Err(error("message role"));
            }
            if let Some(content) = message.get("content").filter(|v| !v.is_null()) {
                let kind = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                let content = if let Some(text) = content.as_str() {
                    json!([{"type":kind,"text":text}])
                } else {
                    let mut parts = content
                        .as_array()
                        .ok_or_else(|| RequestError::invalid("invalid content", Some("messages")))?
                        .clone();
                    for part in &mut parts {
                        if part["type"] != "text" {
                            return Err(error("content type (only text supported)"));
                        }
                        part["type"] = json!(kind);
                    }
                    json!(parts)
                };
                input.push(json!({"role":role,"content":content}));
            }
            if let Some(calls) = message.get("tool_calls") {
                for call in calls
                    .as_array()
                    .ok_or_else(|| RequestError::invalid("invalid tool_calls", Some("messages")))?
                {
                    input.push(json!({"type":"function_call","call_id":call["id"].as_str().ok_or_else(|| RequestError::invalid("call id required", Some("messages")))?,"name":call["function"]["name"].as_str().ok_or_else(|| RequestError::invalid("function name required", Some("messages")))?,"arguments":call["function"]["arguments"].as_str().ok_or_else(|| RequestError::invalid("arguments must be a string", Some("messages")))?}));
                }
            }
        }
        obj.insert("input".into(), json!(input));
        if let Some(tools) = obj.get_mut("tools") {
            for tool in tools
                .as_array_mut()
                .ok_or_else(|| RequestError::invalid("tools must be an array", Some("tools")))?
            {
                if tool["type"] != "function" {
                    return Err(error("tool type"));
                }
                let mut f = tool["function"]
                    .as_object()
                    .ok_or_else(|| RequestError::invalid("function required", Some("tools")))?
                    .clone();
                f.insert("type".into(), json!("function"));
                f.entry("strict").or_insert(json!(false));
                *tool = json!(f);
            }
        }
        if let Some(choice) = obj.get_mut("tool_choice").filter(|v| v.is_object()) {
            *choice = json!({"type":"function", "name":choice["function"]["name"].as_str().ok_or_else(|| RequestError::invalid("tool_choice name required", Some("tool_choice")))?});
        }
        if let Some(effort) = obj.remove("reasoning_effort") {
            obj.insert("reasoning".into(), json!({"effort":effort}));
        }
        if let Some(format) = obj.remove("response_format") {
            let format = if format["type"] == "json_schema" {
                let mut schema = format["json_schema"]
                    .as_object()
                    .ok_or_else(|| {
                        RequestError::invalid("json_schema required", Some("response_format"))
                    })?
                    .clone();
                schema.insert("type".into(), json!("json_schema"));
                json!(schema)
            } else {
                format
            };
            obj.insert("text".into(), json!({"format":format}));
        }
    }
    if let Some(input) = obj.get_mut("input") {
        if let Some(text) = input.as_str() {
            *input = json!([{"role":"user","content":[{"type":"input_text","text":text}]}]);
        }
        let input = input.as_array_mut().ok_or_else(|| {
            RequestError::invalid("input must be a string or an array", Some("input"))
        })?;
        for item in input {
            if item["type"] == "item_reference" {
                return Err(error("item_reference"));
            }
            if let Some(item) = item.as_object_mut() {
                item.remove("id");
            }
        }
    }
    let model = obj
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&config.default_model);
    let model = config
        .models
        .get(model)
        .map(String::as_str)
        .unwrap_or(model)
        .to_owned();
    obj.insert("model".into(), json!(model));
    obj.insert("store".into(), json!(false));
    obj.insert("stream".into(), json!(true));
    obj.entry("instructions").or_insert(json!(""));
    let include = obj
        .entry("include")
        .or_insert(json!([]))
        .as_array_mut()
        .ok_or_else(|| RequestError::invalid("include must be an array", Some("include")))?;
    if !include.contains(&json!("reasoning.encrypted_content")) {
        include.push(json!("reasoning.encrypted_content"));
    }
    let cache_key = match obj.get("prompt_cache_key") {
        Some(v) => Some(
            v.as_str()
                .ok_or_else(|| {
                    RequestError::invalid(
                        "prompt_cache_key must be a string",
                        Some("prompt_cache_key"),
                    )
                })?
                .to_owned(),
        ),
        None => session.map(str::to_owned),
    };
    if let Some(key) = &cache_key {
        obj.insert("prompt_cache_key".into(), json!(key));
    }
    Ok(Prepared {
        body: v,
        cache_key,
        ignored,
    })
}
pub fn usage(v: &Value) -> Value {
    json!({"prompt_tokens":v["input_tokens"],"completion_tokens":v["output_tokens"],"total_tokens":v["total_tokens"],"prompt_tokens_details":v["input_tokens_details"],"completion_tokens_details":v["output_tokens_details"]})
}
pub fn completion(v: &Value) -> Value {
    let mut text = String::new();
    let mut calls = Vec::new();
    for item in v["output"].as_array().into_iter().flatten() {
        if item["type"] == "function_call" {
            calls.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}}));
        }
        if item["type"] == "message" {
            for part in item["content"].as_array().into_iter().flatten() {
                if part["type"] == "output_text" {
                    text.push_str(part["text"].as_str().unwrap_or(""));
                }
            }
        }
    }
    let reason = if v["status"] == "incomplete" {
        "length"
    } else if !calls.is_empty() {
        "tool_calls"
    } else {
        "stop"
    };
    let mut message = json!({"role":"assistant", "content": if text.is_empty() && !calls.is_empty() { Value::Null } else { json!(text) }});
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    json!({"id":v["id"],"object":"chat.completion","created":v["created_at"],"model":v["model"],"choices":[{"index":0,"message":message,"finish_reason":reason}],"usage":usage(&v["usage"])})
}
