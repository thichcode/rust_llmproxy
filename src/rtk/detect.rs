use std::collections::HashMap;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolOutput {
    GitDiff,
    GitLog,
    GitStatus,
    Grep,
    Find,
    Ls,
    Tree,
    CatFile,
    CargoTest,
    CargoBuild,
    Pytest,
    JsonOutput,
    StackTrace,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub tool: ToolOutput,
    pub confidence: f64,
    #[allow(dead_code)]
    pub line_count: usize,
    #[allow(dead_code)]
    pub error_count: usize,
    #[allow(dead_code)]
    pub warning_count: usize,
    #[allow(dead_code)]
    pub has_truncatable_patterns: bool,
}

pub struct Detector {
    git_diff: Regex,
    git_log: Regex,
    git_status: Regex,
    grep_output: Regex,
    find_output: Regex,
    ls_output: Regex,
    tree_output: Regex,
    cat_file: Regex,
    cargo_test: Regex,
    cargo_build: Regex,
    pytest: Regex,
    stack_trace: Regex,
    command_error: Regex,
    long_line: Regex,
    error_line: Regex,
    warning_line: Regex,
}

impl Detector {
    pub fn new() -> Self {
        Detector {
            git_diff: Regex::new(r"(?m)^(diff --git |index |--- |\+\+\+ |@@ |^[+-]{1,2}[^+-])").unwrap(),
            git_log: Regex::new(r"(?m)^commit [0-9a-f]{40}").unwrap(),
            git_status: Regex::new(r"(?m)^\s*[MADRCU?! ]{1,2}\s+.+").unwrap(),
            grep_output: Regex::new(r"(?m)^[^\s]+:\d+:").unwrap(),
            find_output: Regex::new(r"(?m)^\./(?:[^/]+/)*[^/]+$").unwrap(),
            ls_output: Regex::new(r"(?m)^[dlspc\-][rwxsStT\-]{9}").unwrap(),
            tree_output: Regex::new(r"(?m)^[│├└─]").unwrap(),
            cat_file: Regex::new(r"(?m)^\s*\d+:\s").unwrap(),
            cargo_test: Regex::new(r"(?m)^(running \d+ test|test .* \.\.\. (ok|FAILED)|test result:)").unwrap(),
            cargo_build: Regex::new(r"(?m)^(Compiling |Finished |error|warning\[| =\s+note)").unwrap(),
            pytest: Regex::new(r"(?m)^(=+ (?:PASSED|FAILED|ERROR|short test summary)|test_\w+)").unwrap(),
            stack_trace: Regex::new(r"(?m)^\s+at\s+.+\([^)]+:\d+:\d+\)").unwrap(),
            command_error: Regex::new(r"(?mi)^(error|Error|ERROR|failed|FAILED|fatal|FATAL|panic|PANIC)").unwrap(),
            long_line: Regex::new(r"(?m)^.{200,}$").unwrap(),
            error_line: Regex::new(r"(?mi)(^|\s)(error|Error|ERROR|failed|FAILED|fatal|FATAL|panic|PANIC)").unwrap(),
            warning_line: Regex::new(r"(?mi)(^|\s)(warning|WARNING|warn|WARN)").unwrap(),
        }
    }

    pub fn detect(&self, content: &str) -> DetectionResult {
        let line_count = content.lines().count();
        let mut scores: HashMap<ToolOutput, usize> = HashMap::new();

        for (i, line) in content.lines().enumerate() {
            if line.is_empty() { continue; }

            if self.git_diff.is_match(line) {
                *scores.entry(ToolOutput::GitDiff).or_insert(0) += 1;
            }
            if self.git_log.is_match(line) {
                *scores.entry(ToolOutput::GitLog).or_insert(0) += 1;
            }
            if self.git_status.is_match(line) && i == 0 {
                *scores.entry(ToolOutput::GitStatus).or_insert(0) += 1;
            }
            if self.grep_output.is_match(line) {
                *scores.entry(ToolOutput::Grep).or_insert(0) += 1;
            }
            if self.find_output.is_match(line) && line.len() > 10 {
                *scores.entry(ToolOutput::Find).or_insert(0) += 1;
            }
            if self.ls_output.is_match(line) {
                *scores.entry(ToolOutput::Ls).or_insert(0) += 1;
            }
            if self.tree_output.is_match(line) {
                *scores.entry(ToolOutput::Tree).or_insert(0) += 1;
            }
            if self.cat_file.is_match(line) {
                *scores.entry(ToolOutput::CatFile).or_insert(0) += 1;
            }
            if self.cargo_test.is_match(line) {
                *scores.entry(ToolOutput::CargoTest).or_insert(0) += 1;
            }
            if self.cargo_build.is_match(line) && i < 5 {
                *scores.entry(ToolOutput::CargoBuild).or_insert(0) += 1;
            }
            if self.pytest.is_match(line) {
                *scores.entry(ToolOutput::Pytest).or_insert(0) += 1;
            }
            if self.stack_trace.is_match(line) {
                *scores.entry(ToolOutput::StackTrace).or_insert(0) += 1;
            }
        }

        if content.trim_start().starts_with('{') || content.trim_start().starts_with('[') {
            *scores.entry(ToolOutput::JsonOutput).or_insert(0) += line_count.min(5);
        }

        let has_cmd_error = self.command_error.is_match(content);

        let (tool, confidence) = scores
            .into_iter()
            .max_by_key(|(_, score)| *score)
            .map(|(t, score)| {
                let conf = (score as f64 / line_count.max(1) as f64).min(1.0);
                if has_cmd_error && score > 0 {
                    (t, conf.max(0.5))
                } else {
                    (t, conf)
                }
            })
            .unwrap_or((ToolOutput::Unknown, 0.0));

        DetectionResult {
            tool,
            confidence,
            line_count,
            error_count: self.error_line.find_iter(content).count(),
            warning_count: self.warning_line.find_iter(content).count(),
            has_truncatable_patterns: content.lines().any(|l| self.long_line.is_match(l)) || line_count > 50,
        }
    }
}
