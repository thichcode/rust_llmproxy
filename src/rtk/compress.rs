use crate::config::RtkConfig;
use crate::models::ChatRequest;
use crate::rtk::detect::{self, Detector, ToolOutput};
use crate::rtk::scoring::{score_message, should_compress};

pub fn compress_chat_request(req: &mut ChatRequest, options: &RtkConfig) {
    if !options.enabled {
        return;
    }

    let detector = Detector::new();

    for msg in &mut req.messages {
        let content = match msg.content.as_ref() {
            Some(c) => c,
            None => continue,
        };

        if content.len() <= options.max_message_chars {
            continue;
        }

        let score = score_message(msg, content);
        if !should_compress(&score, options.max_message_chars) {
            continue;
        }

        let detection = detector.detect(content);

        let compressed = if detection.confidence > 0.3 {
            smart_compress(content, &detection, options)
        } else {
            head_tail_compress(content, options)
        };

        msg.content = Some(compressed);
    }
}

fn smart_compress(content: &str, detection: &detect::DetectionResult, options: &RtkConfig) -> String {
    match detection.tool {
        ToolOutput::GitDiff => compress_git_diff(content, options),
        ToolOutput::CargoTest => compress_test_output(content, options),
        ToolOutput::Pytest => compress_test_output(content, options),
        ToolOutput::CargoBuild => compress_build_output(content, options),
        ToolOutput::StackTrace => compress_stack_trace(content, options),
        _ => compress_general(content, detection, options),
    }
}

fn compress_git_diff(content: &str, options: &RtkConfig) -> String {
    let mut result = String::new();
    let mut lines = content.lines().peekable();
    let mut diff_count = 0;

    while let Some(line) = lines.next() {
        if line.starts_with("diff --git") {
            diff_count += 1;
        }
    }

    let head_chars = options.preserve_head_chars;
    let tail_chars = options.preserve_tail_chars;

    if diff_count <= 3 {
        head_tail_compress(content, options)
    } else {
        for line in content.lines() {
            if line.starts_with("diff --git") || line.starts_with("---") || line.starts_with("+++") {
                result.push_str(line);
                result.push('\n');
            }
        }

        if result.len() < head_chars {
            let tail = &content[content.len().saturating_sub(tail_chars)..];
            result.push_str(&format!("\n... (truncated body) ...\n\n"));
            result.push_str(tail);
        }

        result.push_str(&format!("\n[RTK: {} files changed in diff]", diff_count));
        result
    }
}

fn compress_test_output(content: &str, _options: &RtkConfig) -> String {
    let mut result = String::new();
    let mut failures = Vec::new();
    let mut passed_count = 0;
    let mut failure_count = 0;
    let mut in_failure = false;
    let mut failure_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("test ") && trimmed.contains("... ok") {
            passed_count += 1;
            if passed_count <= 3 {
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }

        if trimmed.starts_with("test ") && trimmed.contains("FAILED") {
            failure_count += 1;
            failures.push(line.to_string());
            in_failure = true;
            failure_lines.clear();
            continue;
        }

        if in_failure && !trimmed.starts_with("test ") && !trimmed.starts_with("running ") {
            failure_lines.push(line);
        } else if in_failure {
            in_failure = false;
            if !failure_lines.is_empty() {
                let fail_detail: String = failure_lines.iter().take(10).map(|l| format!("  {}\n", l)).collect();
                failures.push(fail_detail.trim_end().to_string());
            }
        }
    }

    if !failures.is_empty() {
        result.clear();
        for f in &failures {
            result.push_str(f);
            result.push('\n');
        }
    }

    result.push_str(&format!(
        "\n[RTK: {} passed, {} failed, {} total]",
        passed_count,
        failure_count,
        passed_count + failure_count
    ));

    result
}

fn compress_build_output(content: &str, _options: &RtkConfig) -> String {
    let mut result = String::new();
    let mut error_lines = Vec::new();
    let mut warning_count = 0;
    let mut compiling_count = 0;
    let mut saw_finished = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Compiling ") {
            compiling_count += 1;
            if compiling_count <= 2 {
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }
        if trimmed.starts_with("Finished ") {
            saw_finished = true;
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if trimmed.starts_with("error") || trimmed.starts_with("Error") {
            error_lines.push(line);
            continue;
        }
        if trimmed.contains("warning[") || trimmed.starts_with("warning:") {
            warning_count += 1;
            if warning_count <= 3 {
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }
    }

    if !error_lines.is_empty() {
        result.clear();
        for e in error_lines {
            result.push_str(e);
            result.push('\n');
        }
    }

    if !saw_finished {
        result.push_str(&format!("\n[RTK: {} crates compiled, {} warnings]", compiling_count, warning_count));
    }

    result
}

fn compress_stack_trace(content: &str, _options: &RtkConfig) -> String {
    let mut result = String::new();
    let mut lines = content.lines();
    let mut kept_lines = 0;
    let mut total_lines = 0;

    while let Some(line) = lines.next() {
        total_lines += 1;
        if kept_lines >= 20 { break; }

        if line.contains("error") || line.contains("Error") || line.starts_with("    at") {
            if kept_lines > 0 || line.starts_with("    at") {
                result.push_str(line);
                result.push('\n');
                kept_lines += 1;
            }
            continue;
        }

        if kept_lines < 5 {
            result.push_str(line);
            result.push('\n');
            kept_lines += 1;
        }
    }

    if total_lines > kept_lines {
        result.push_str(&format!("... ({} frames omitted)", total_lines - kept_lines));
    }

    result
}

fn compress_general(content: &str, detection: &detect::DetectionResult, options: &RtkConfig) -> String {
    let original_chars = content.len();
    let mut important_lines = Vec::new();
    let mut normal_lines = Vec::new();
    let mut total = 0usize;

    let error_prefixes = [
        "error", "Error", "ERROR", "failed", "FAILED", "fatal", "FATAL",
        "panic", "PANIC", "warning", "Warning", "WARNING", "note:",
    ];

    for line in content.lines() {
        total += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        let is_important = error_prefixes.iter().any(|p| trimmed.starts_with(p))
            || trimmed.starts_with('+') || trimmed.starts_with('-')
            || trimmed.starts_with("@@");

        if is_important {
            important_lines.push(line);
        } else {
            normal_lines.push(line);
        }
    }

    let head_chars = options.preserve_head_chars;
    let tail_chars = options.preserve_tail_chars;

    if !important_lines.is_empty() {
        let mut result = important_lines.join("\n");
        if result.len() < head_chars / 2 {
            let head_bytes = content.char_indices().nth(head_chars).map(|(i, _)| i).unwrap_or(content.len());
            let head = &content[..head_bytes.min(content.len())];
            let tail_start = content.len().saturating_sub(tail_chars);
            let tail = &content[tail_start..];
            result = format!("{}\n\n... (compressed) ...\n\n{}", head, tail);
        }
        return result;
    }

    let head_bytes = content.char_indices().nth(head_chars).map(|(i, _)| i).unwrap_or(content.len());
    let head = &content[..head_bytes.min(content.len())];
    let tail_start = content.len().saturating_sub(tail_chars);
    let tail = &content[tail_start..];

    format!(
        "{}\n\n[RTK_COMPRESSED: original_chars={}, tool={:?}, lines={}]\n\n{}",
        head, original_chars, detection.tool, total, tail
    )
}

fn head_tail_compress(content: &str, options: &RtkConfig) -> String {
    let head_chars = options.preserve_head_chars.min(content.len());
    let tail_chars = options.preserve_tail_chars.min(content.len());
    let original_chars = content.len();

    let head_bytes = content.char_indices().nth(head_chars).map(|(i, _)| i).unwrap_or(content.len());
    let head = &content[..head_bytes.min(content.len())];
    let tail_start = content.len().saturating_sub(tail_chars);
    let tail = &content[tail_start..];

    format!(
        "{}\n\n[RTK_COMPRESSED: original_chars={}]\n\n{}",
        head, original_chars, tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatMessage;
    use std::collections::HashMap;

    #[test]
    fn test_compress_git_diff_keeps_summary() {
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc..def 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@
 fn main() {
     println!("Hello");
+    println!("World");
 }
diff --git a/src/lib.rs b/src/lib.rs
index 123..456 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,4 @@
 pub fn add(x: i32, y: i32) -> i32 {
     x + y
 }
+"#;
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 100,
            preserve_head_chars: 50,
            preserve_tail_chars: 50,
        };
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(diff.to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        let compressed = req.messages[0].content.as_ref().unwrap();
        assert!(compressed.contains("diff --git"), "should keep diff headers");
        assert!(compressed.contains("RTK"), "should have RTK marker");
    }

    #[test]
    fn test_compress_many_diffs() {
        let mut diff = String::new();
        for i in 0..5 {
            diff.push_str(&format!("diff --git a/src/file{}.rs b/src/file{}.rs\n", i, i));
            diff.push_str("index abc..def 100644\n--- a/src/file.rs\n+++ b/src/file.rs\n@@ -1,5 +1,6 @@\n fn main() {\n+    println!(\"new\");\n }\n");
        }
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 100,
            preserve_head_chars: 50,
            preserve_tail_chars: 50,
        };
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(diff),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        let compressed = req.messages[0].content.as_ref().unwrap();
        assert!(compressed.contains("5 files changed"), "should count files for many diffs");
    }

    #[test]
    fn test_compress_test_output_failures_first() {
        let test_out = r#"running 5 tests
test test_add ... ok
test test_sub ... ok
test test_mul ... FAILED
test test_div ... FAILED
test test_mod ... ok

failures:
---- test_mul stdout ----
assert_eq!(2 * 3, 5)

---- test_div stdout ----
assert_eq!(6 / 2, 2)

failures:
    test_mul
    test_div

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 50,
            preserve_head_chars: 30,
            preserve_tail_chars: 30,
        };
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(test_out.to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        let compressed = req.messages[0].content.as_ref().unwrap();
        assert!(compressed.contains("FAILED"), "should keep failures");
        assert!(compressed.contains("[RTK:"), "should have summary marker");
    }

    #[test]
    fn test_compress_disabled() {
        let config = RtkConfig {
            enabled: false,
            max_message_chars: 50,
            preserve_head_chars: 20,
            preserve_tail_chars: 10,
        };
        let long = "A".repeat(100);
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(long.clone()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        assert_eq!(req.messages[0].content.as_ref().unwrap(), &long);
    }

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
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        assert_eq!(req.messages[0].content.as_deref(), Some("short"));
    }

    #[test]
    fn test_system_messages_not_compressed() {
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 50,
            preserve_head_chars: 20,
            preserve_tail_chars: 10,
        };
        let long = "system instruction that is very long ".repeat(20);
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "system".to_string(),
                content: Some(long.clone()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        assert_eq!(req.messages[0].content.as_ref().unwrap(), &long);
    }

    #[test]
    fn test_error_dense_content_kept() {
        let config = RtkConfig {
            enabled: true,
            max_message_chars: 50,
            preserve_head_chars: 20,
            preserve_tail_chars: 10,
        };
        let error_content = (0..20).map(|i| format!("error line {}: something failed badly\n", i)).collect::<String>();
        let mut req = ChatRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(error_content),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };
        compress_chat_request(&mut req, &config);
        let compressed = req.messages[0].content.as_ref().unwrap();
        assert!(compressed.contains("error"), "should keep error lines");
        assert!(compressed.contains("[RTK:"), "should have marker");
    }

    #[test]
    fn test_detection_works() {
        let detector = Detector::new();

        let git_diff = "diff --git a/src/main.rs b/src/main.rs\nindex abc..def\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,5 +1,6 @@\n fn main() {\n+    new line\n }";
        let result = detector.detect(git_diff);
        assert_eq!(result.tool, ToolOutput::GitDiff);
        assert!(result.confidence > 0.0);

        let cargo_test = "running 3 tests\ntest test_a ... ok\ntest test_b ... FAILED\ntest result: FAILED. 1 passed; 1 failed;";
        let result = detector.detect(cargo_test);
        assert_eq!(result.tool, ToolOutput::CargoTest);

        let plain_text = "Hello, how are you?\nI'm doing great!";
        let result = detector.detect(plain_text);
        assert_eq!(result.tool, ToolOutput::Unknown);
        assert!(result.confidence == 0.0);
    }
}
