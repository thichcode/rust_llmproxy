use crate::models::{
    AnthropicMessage, AnthropicRequest, AnthropicResponse, ChatMessage, ChatRequest, ChatResponse,
    Choice, Usage,
};

pub fn to_anthropic_request(req: ChatRequest, model: String) -> AnthropicRequest {
    let mut system: Option<String> = None;
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in req.messages {
        if msg.role == "system" {
            system = msg.content;
        } else {
            messages.push(AnthropicMessage {
                role: msg.role.clone(),
                content: msg.content.unwrap_or_default(),
            });
        }
    }

    AnthropicRequest {
        model,
        messages,
        system,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        extra: req.extra,
    }
}

pub fn to_openai_response(anthropic: &AnthropicResponse, model: &str) -> ChatResponse {
    let content = anthropic
        .content
        .iter()
        .map(|b| b.text.clone())
        .collect::<Vec<_>>()
        .join("");

    let usage = anthropic.usage.as_ref().map(|u| Usage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: u.input_tokens + u.output_tokens,
    });

    ChatResponse {
        id: anthropic.id.clone(),
        object: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: Some(content),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: anthropic.stop_reason.clone(),
        }],
        usage,
    }
}
