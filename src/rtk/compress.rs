use crate::config::RtkConfig;
use crate::models::ChatRequest;

pub fn compress_chat_request(req: &mut ChatRequest, options: &RtkConfig) {
    if !options.enabled {
        return;
    }

    for msg in &mut req.messages {
        if let Some(ref content) = msg.content {
            if content.len() > options.max_message_chars {
                let head = &content[..options.preserve_head_chars.min(content.len())];
                let tail = &content[content.len().saturating_sub(options.preserve_tail_chars)..];
                let original_chars = content.len();

                let marker = format!(
                    "\n\n[RTK_COMPRESSED: original_chars={}]\n\n",
                    original_chars
                );

                let compressed = format!("{}{}{}", head, marker, tail);
                msg.content = Some(compressed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatMessage;

    #[test]
    fn test_compress_short_message() {
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 100,
            preserve_head_chars: 20,
            preserve_tail_chars: 20,
        };
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("short".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: std::collections::HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        assert_eq!(req.messages[0].content.as_deref(), Some("short"));
    }

    #[test]
    fn test_compress_long_message() {
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 50,
            preserve_head_chars: 20,
            preserve_tail_chars: 10,
        };
        let long_content = "A".repeat(100);
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(long_content),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: std::collections::HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        let compressed = req.messages[0].content.as_ref().unwrap();
        assert!(compressed.contains("[RTK_COMPRESSED: original_chars=100]"));
        assert!(compressed.starts_with("AAAAAAAAAAAAAAAAAAAA"));
        assert!(compressed.ends_with("AAAAAAAAAA"));
    }

    #[test]
    fn test_compress_disabled() {
        let config = RtkConfig {
            enabled: false,
            max_message_chars: 50,
            preserve_head_chars: 20,
            preserve_tail_chars: 10,
        };
        let long_content = "A".repeat(100);
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(long_content.clone()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: std::collections::HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        assert_eq!(req.messages[0].content.as_ref().unwrap(), &long_content);
    }
}
