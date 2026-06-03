use crate::models::ChatMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    Critical,
    High,
    Normal,
    Low,
}

impl Importance {
    #[allow(dead_code)]
    pub fn score(self) -> u8 {
        match self {
            Importance::Critical => 4,
            Importance::High => 3,
            Importance::Normal => 2,
            Importance::Low => 1,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MessageScore {
    pub importance: Importance,
    pub estimated_tokens: usize,
    pub compressible_tokens: usize,
    pub error_density: f64,
    pub is_system: bool,
    pub is_assistant: bool,
}

pub fn score_message(msg: &ChatMessage, content: &str) -> MessageScore {
    let char_count = content.len();
    let estimated_tokens = (char_count as f64 / 4.0).ceil() as usize;
    let line_count = content.lines().count();

    let error_count = content
        .lines()
        .filter(|l| {
            l.contains("error")
                || l.contains("Error")
                || l.contains("ERROR")
                || l.contains("failed")
                || l.contains("FAILED")
                || l.contains("fatal")
                || l.contains("FATAL")
                || l.contains("panic")
                || l.contains("PANIC")
        })
        .count();

    let error_density = if line_count > 0 {
        error_count as f64 / line_count as f64
    } else {
        0.0
    };

    let is_system = msg.role == "system";
    let is_assistant = msg.role == "assistant";

    let importance = if is_system {
        Importance::Critical
    } else if is_assistant {
        if error_density > 0.1 || char_count < 200 {
            Importance::High
        } else {
            Importance::Normal
        }
    } else {
        if error_density > 0.2 {
            Importance::High
        } else if char_count < 500 {
            Importance::Normal
        } else {
            Importance::Low
        }
    };

    let compressible = if char_count > 1000 {
        let base = char_count.saturating_sub(500);
        if error_density > 0.3 {
            base / 3
        } else {
            base
        }
    } else {
        0
    };

    MessageScore {
        importance,
        estimated_tokens,
        compressible_tokens: compressible / 4,
        error_density,
        is_system,
        is_assistant,
    }
}

pub fn should_compress(score: &MessageScore, config_threshold_chars: usize) -> bool {
    if score.is_system {
        return false;
    }
    if score.importance == Importance::Critical {
        return false;
    }
    if score.estimated_tokens * 4 < config_threshold_chars {
        return false;
    }
    true
}
