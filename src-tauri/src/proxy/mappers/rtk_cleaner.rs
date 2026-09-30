//! RTK Cleaner Module
//!
//! Rust-native implementation of terminal/shell log cleanup pipeline.
//! Ported and optimized from OmniRoute's RTK engine.

use once_cell::sync::Lazy;
use regex::Regex;

// Lazy static regular expressions to optimize processing
static ANSI_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\x1B\[[0-9;?]*[a-zA-Z]").unwrap());

static DIGITS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+").unwrap());

static ERROR_KEYWORDS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)error|failed|exception|traceback|FAIL|✖|TS\d{4}").unwrap());

/// Clean terminal outputs: strip ANSI codes, deduplicate lines, group similar lines, and smart truncate.
pub struct RtkCleaner;

impl RtkCleaner {
    /// Main pipeline to clean a terminal/shell output log
    pub fn clean(text: &str, max_lines: usize) -> String {
        if text.is_empty() {
            return String::new();
        }

        // 1. Strip ANSI escape sequences
        let text_stripped = Self::strip_ansi(text);

        // 2. Split into lines
        let lines: Vec<&str> = text_stripped.lines().collect();

        // 3. Deduplicate exact consecutive duplicates
        let deduped_lines = Self::deduplicate_lines(&lines);

        // 4. Group near-equivalent consecutive lines (e.g. progress bars, similar build messages)
        let grouped_lines = Self::group_similar_lines(&deduped_lines);

        // 5. Smart Truncate: Keep head/tail, but preserve error/failure rows from the middle
        Self::smart_truncate(&grouped_lines, max_lines)
    }

    /// Strip terminal color and control sequences
    pub fn strip_ansi(text: &str) -> String {
        ANSI_RE.replace_all(text, "").to_string()
    }

    /// Deduplicate identical consecutive lines
    pub fn deduplicate_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
        let mut result = Vec::new();
        let mut last_line = None;

        for &line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                result.push(line);
                continue;
            }
            if Some(trimmed) != last_line {
                result.push(line);
                last_line = Some(trimmed);
            }
        }
        result
    }

    /// Group similar consecutive lines by replacing digit clusters with a placeholder
    /// and collapsing runs of identical "skeletons".
    pub fn group_similar_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
        if lines.len() < 3 {
            return lines.to_vec();
        }

        let mut result = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            let current = lines[index];
            let current_trimmed = current.trim();

            if current_trimmed.is_empty() || current_trimmed.len() < 5 {
                result.push(current);
                index += 1;
                continue;
            }

            // Extract "skeleton" by replacing all digits with '#'
            let get_skeleton =
                |s: &str| -> String { DIGITS_RE.replace_all(s.trim(), "#").to_string() };

            let current_skeleton = get_skeleton(current_trimmed);
            let mut run_end = index + 1;

            while run_end < lines.len() {
                let next = lines[run_end];
                let next_trimmed = next.trim();

                if next_trimmed.is_empty() {
                    run_end += 1;
                    continue;
                }

                let next_skeleton = get_skeleton(next_trimmed);
                if next_skeleton == current_skeleton {
                    run_end += 1;
                } else {
                    break;
                }
            }

            let run_len = run_end - index;
            if run_len >= 3 {
                // We have a run of 3 or more similar lines. Keep the first line,
                // add a placeholder indicating collapsed lines, and keep the last line of the run.
                result.push(lines[index]);

                // Construct placeholder string
                // We use static allocation for the placeholder to avoid lifetime issues
                let collapsed_msg = Box::leak(
                    format!("... [Collapsed {} similar lines] ...", run_len - 2).into_boxed_str(),
                );
                result.push(collapsed_msg);

                result.push(lines[run_end - 1]);
            } else {
                // Copy the elements as-is
                for i in index..run_end {
                    result.push(lines[i]);
                }
            }

            index = run_end;
        }

        result
    }

    /// Smart truncate: Keep top N and bottom N lines, but scan the middle and preserve error lines.
    pub fn smart_truncate(lines: &[&str], max_lines: usize) -> String {
        if lines.len() <= max_lines {
            return lines.join("\n");
        }

        // Determine how many lines to keep at head and tail
        let keep_head = (max_lines / 2).max(1);
        let keep_tail = (max_lines - keep_head).max(1);

        let head_end = keep_head;
        let tail_start = lines.len().saturating_sub(keep_tail);

        let mut output = Vec::new();

        // 1. Keep Head
        for i in 0..head_end {
            output.push(lines[i].to_string());
        }

        // 2. Scan Middle and extract only errors/failures
        let mut middle_errors = Vec::new();
        let mut skipped_count = 0;

        for i in head_end..tail_start {
            let line = lines[i];
            if ERROR_KEYWORDS_RE.is_match(line) {
                middle_errors.push(line.to_string());
            } else {
                skipped_count += 1;
            }
        }

        // 3. Append Middle errors or collapse info
        if !middle_errors.is_empty() {
            output.push(format!(
                "... [Truncated {} verbose lines, preserved {} error lines] ...",
                skipped_count,
                middle_errors.len()
            ));
            output.extend(middle_errors);
        } else if skipped_count > 0 {
            output.push(format!(
                "... [Truncated {} verbose lines] ...",
                skipped_count
            ));
        }

        // 4. Keep Tail
        for i in tail_start..lines.len() {
            output.push(lines[i].to_string());
        }

        output.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "\x1B[31mError:\x1B[0m something went wrong";
        assert_eq!(RtkCleaner::strip_ansi(input), "Error: something went wrong");
    }

    #[test]
    fn test_deduplicate_lines() {
        let lines = vec![
            "Compiling...",
            "Compiling...",
            "Warning in x",
            "Warning in x",
            "Done.",
        ];
        let result = RtkCleaner::deduplicate_lines(&lines);
        assert_eq!(result, vec!["Compiling...", "Warning in x", "Done."]);
    }

    #[test]
    fn test_group_similar_lines() {
        let lines = vec![
            "[ 10%] Building index.ts",
            "[ 20%] Building index.ts",
            "[ 30%] Building index.ts",
            "[ 40%] Building index.ts",
            "Finished building.",
        ];
        let result = RtkCleaner::group_similar_lines(&lines);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "[ 10%] Building index.ts");
        assert!(result[1].contains("Collapsed 2 similar lines"));
        assert_eq!(result[2], "[ 40%] Building index.ts");
        assert_eq!(result[3], "Finished building.");
    }

    #[test]
    fn test_smart_truncate() {
        let lines = vec![
            "Start",
            "Info 1",
            "Info 2",
            "FAIL tests/a.test.ts",
            "Info 3",
            "Error: stacktrace here",
            "Info 4",
            "End",
        ];
        // max_lines = 4, keep head = 2 (index 0,1), keep tail = 2 (index 6,7)
        // scan middle (index 2 to 5) for errors -> should keep FAIL and Error lines
        let result = RtkCleaner::smart_truncate(&lines, 4);
        assert!(result.contains("Start"));
        assert!(result.contains("Info 1"));
        assert!(result.contains("FAIL tests/a.test.ts"));
        assert!(result.contains("Error: stacktrace here"));
        assert!(result.contains("Info 4"));
        assert!(result.contains("End"));
        assert!(!result.contains("Info 2"));
        assert!(!result.contains("Info 3"));
    }
}
