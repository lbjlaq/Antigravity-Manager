//! 工具结果输出压缩模块
//! 
//! 提供智能压缩功能:
//! - 浏览器快照压缩 (头+尾保留)
//! - 大文件提示压缩 (提取关键信息)
//! - 通用截断 (200,000 字符限制)

use regex::Regex;
use serde_json::Value;
use tracing::{debug, info};

/// 最大工具结果字符数 (约 20 万,防止 prompt 超长)
const MAX_TOOL_RESULT_CHARS: usize = 200_000;

/// 浏览器快照检测阈值
const SNAPSHOT_DETECTION_THRESHOLD: usize = 20_000;

/// 浏览器快照压缩后的最大字符数
const SNAPSHOT_MAX_CHARS: usize = 16_000;

/// 浏览器快照头部保留比例
const SNAPSHOT_HEAD_RATIO: f64 = 0.7;

/// 浏览器快照尾部保留比例
#[allow(dead_code)]
const SNAPSHOT_TAIL_RATIO: f64 = 0.3;

/// 压缩工具结果文本
/// 
/// 根据内容类型自动选择最佳压缩策略:
/// 1. 大文件提示 → 提取关键信息
/// 2. 浏览器快照 → 头+尾保留
/// 3. 其他 → 简单截断
pub fn compact_tool_result_text(text: &str, max_chars: usize) -> String {
    if text.is_empty() || text.len() <= max_chars {
        return text.to_string();
    }
    
    // [NEW] 针对可能的 HTML 内容进行深度预处理
    let cleaned_text = if text.contains("<html") || text.contains("<body") || text.contains("<!DOCTYPE") {
        let cleaned = deep_clean_html(text);
        debug!("[ToolCompressor] Deep cleaned HTML, reduced {} -> {} chars", text.len(), cleaned.len());
        cleaned
    } else {
        text.to_string()
    };

    if cleaned_text.len() <= max_chars {
        return cleaned_text;
    }

    // 1. 检测大文件提示模式
    if let Some(compacted) = compact_saved_output_notice(&cleaned_text, max_chars) {
        debug!("[ToolCompressor] Detected saved output notice, compacted to {} chars", compacted.len());
        return compacted;
    }
    
    // 2. 检测浏览器快照模式
    if cleaned_text.len() > SNAPSHOT_DETECTION_THRESHOLD {
        if let Some(compacted) = compact_browser_snapshot(&cleaned_text, max_chars) {
            debug!("[ToolCompressor] Detected browser snapshot, compacted to {} chars", compacted.len());
            return compacted;
        }
    }
    
    // 3. 结构化截断
    debug!("[ToolCompressor] Using structured truncation for {} chars", cleaned_text.len());
    truncate_text_safe(&cleaned_text, max_chars)
}

/// 压缩"输出已保存到文件"类型的提示
/// 
/// 检测模式: "result (N characters) exceeds maximum allowed tokens. Output saved to <path>"
/// 策略: 提取关键信息(文件路径、字符数、格式说明)
/// 
/// 参考: anthropicGeminiBridgeService.js:278-310
fn compact_saved_output_notice(text: &str, max_chars: usize) -> Option<String> {
    // 正则匹配: result (N characters) exceeds maximum allowed tokens. Output saved to <path>
    let re = Regex::new(
        r"(?i)result\s*\(\s*(?P<count>[\d,]+)\s*characters\s*\)\s*exceeds\s+maximum\s+allowed\s+tokens\.\s*Output\s+(?:has\s+been\s+)?saved\s+to\s+(?P<path>[^\r\n]+)"
    ).ok()?;
    
    let caps = re.captures(text)?;
    let count = caps.name("count")?.as_str();
    let raw_path = caps.name("path")?.as_str();
    
    // 清理文件路径 (移除尾部的括号、引号、句号)
    let file_path = raw_path
        .trim()
        .trim_end_matches(&[')', ']', '"', '\'', '.'][..])
        .trim();
    
    // 提取关键行
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    
    // 查找通知行
    let notice_line = lines.iter()
        .find(|l| l.to_lowercase().contains("exceeds maximum allowed tokens") && l.to_lowercase().contains("saved to"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("result ({} characters) exceeds maximum allowed tokens. Output has been saved to {}", count, file_path));
    
    // 查找格式说明行
    let format_line = lines.iter()
        .find(|l| l.starts_with("Format:") || l.contains("JSON array with schema") || l.to_lowercase().starts_with("schema:"))
        .map(|s| s.to_string());
    
    // 构建压缩后的输出
    let mut compact_lines = vec![notice_line];
    if let Some(fmt) = format_line {
        if !compact_lines.contains(&fmt) {
            compact_lines.push(fmt);
        }
    }
    compact_lines.push(format!(
        "[tool_result omitted to reduce prompt size; read file locally if needed: {}]",
        file_path
    ));
    
    let result = compact_lines.join("\n");
    Some(truncate_text_safe(&result, max_chars))
}

/// 压缩浏览器快照 (头+尾保留策略)
/// 
/// 检测: "page snapshot" 或 "页面快照" 或大量 "ref=" 引用
/// 策略: 保留头部 70% + 尾部 30%,中间省略
/// 
/// 参考: anthropicGeminiBridgeService.js:312-339
fn compact_browser_snapshot(text: &str, max_chars: usize) -> Option<String> {
    // 检测是否是浏览器快照
    let is_snapshot = text.to_lowercase().contains("page snapshot")
        || text.contains("页面快照")
        || text.matches("ref=").count() > 30
        || text.matches("[ref=").count() > 30;
    
    if !is_snapshot {
        return None;
    }
    
    let desired_max = max_chars.min(SNAPSHOT_MAX_CHARS);
    if desired_max < 2000 || text.len() <= desired_max {
        return None;
    }
    
    let meta = format!("[page snapshot summarized to reduce prompt size; original {} chars]", text.len());
    let overhead = meta.len() + 200;
    let budget = desired_max.saturating_sub(overhead);
    
    if budget < 1000 {
        return None;
    }
    
    // 计算头部和尾部长度
    let head_len = (budget as f64 * SNAPSHOT_HEAD_RATIO).floor() as usize;
    let head_len = head_len.min(10_000).max(500);
    let tail_len = budget.saturating_sub(head_len).min(3_000);
    
    let head = &text[..head_len.min(text.len())];
    let tail = if tail_len > 0 && text.len() > head_len {
        let start = text.len().saturating_sub(tail_len);
        &text[start..]
    } else {
        ""
    };
    
    let omitted = text.len().saturating_sub(head_len).saturating_sub(tail_len);
    
    let summarized = if tail.is_empty() {
        format!("{}\n---[HEAD]---\n{}\n---[...omitted {} chars]---", meta, head, omitted)
    } else {
        format!(
            "{}\n---[HEAD]---\n{}\n---[...omitted {} chars]---\n---[TAIL]---\n{}",
            meta, head, omitted, tail
        )
    };
    
    Some(truncate_text_safe(&summarized, max_chars))
}

/// 安全的文本截断 (尽量不在标签中间截断)
fn truncate_text_safe(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    
    // 尝试寻找一个安全的截断点 (不在 < 和 > 之间)
    let mut split_pos = max_chars;
    
    // 向前查找是否有未闭合的标签开始符
    let sub = &text[..max_chars];
    if let Some(last_open) = sub.rfind('<') {
        if let Some(last_close) = sub.rfind('>') {
            if last_open > last_close {
                // 截断点在标签中间，回退到标签开始前
                split_pos = last_open;
            }
        } else {
            // 只有开始没有结束，回退到标签开始前
            split_pos = last_open;
        }
    }
    
    // 也要避免在 JSON 大括号中间截断
    if let Some(last_open_brace) = sub.rfind('{') {
        if let Some(last_close_brace) = sub.rfind('}') {
            if last_open_brace > last_close_brace {
                // 可能在 JSON 中间，如果距离截断点较近，尝试回退
                if max_chars - last_open_brace < 100 {
                    split_pos = split_pos.min(last_open_brace);
                }
            }
        }
    }

    let truncated = &text[..split_pos];
    let omitted = text.len() - split_pos;
    format!("{}\n...[truncated {} chars]", truncated, omitted)
}

/// 深度清理 HTML (移除 style, script, base64 等)
fn deep_clean_html(html: &str) -> String {
    let mut result = html.to_string();
    
    // 1. 移除 <style>...</style> 及其内容
    if let Ok(re) = Regex::new(r"(?is)<style\b[^>]*>.*?</style>") {
        result = re.replace_all(&result, "[style omitted]").to_string();
    }
    
    // 2. 移除 <script>...</script> 及其内容
    if let Ok(re) = Regex::new(r"(?is)<script\b[^>]*>.*?</script>") {
        result = re.replace_all(&result, "[script omitted]").to_string();
    }
    
    // 3. 移除 inline Base64 数据 (如 src="data:image/png;base64,...")
    if let Ok(re) = Regex::new(r#"(?i)data:[^;/]+/[^;]+;base64,[A-Za-z0-9+/=]+"#) {
        result = re.replace_all(&result, "[base64 omitted]").to_string();
    }

    // 4. 移除冗余的空白字符
    if let Ok(re) = Regex::new(r"\n\s*\n") {
        result = re.replace_all(&result, "\n").to_string();
    }
    
    result
}

/// 清理工具结果 content blocks
/// 
/// 处理逻辑:
/// 1. 移除 base64 图片 (避免体积过大)
/// 2. 压缩文本内容 (使用智能压缩策略)
/// 3. 限制总字符数 (默认 200,000)
/// 
/// 参考: anthropicGeminiBridgeService.js:540-597
pub fn sanitize_tool_result_blocks(blocks: &mut Vec<Value>) {
    let mut used_chars = 0;
    let mut cleaned_blocks = Vec::new();
    let mut removed_image = false;
    
    if !blocks.is_empty() {
        info!(
            "[ToolCompressor] Processing {} blocks for truncation (MAX: {} chars)",
            blocks.len(),
            MAX_TOOL_RESULT_CHARS
        );
    }
    
    for block in blocks.iter() {
        // 移除 base64 图片
        if is_base64_image(block) {
            removed_image = true;
            debug!("[ToolCompressor] Removed base64 image block");
            continue;
        }
        
        // 压缩文本内容
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            let remaining = MAX_TOOL_RESULT_CHARS.saturating_sub(used_chars);
            if remaining == 0 {
                debug!("[ToolCompressor] Reached character limit, stopping");
                break;
            }
            
            let compacted = compact_tool_result_text(text, remaining);
            let mut new_block = block.clone();
            new_block["text"] = Value::String(compacted.clone());
            cleaned_blocks.push(new_block);
            used_chars += compacted.len();
            
            debug!(
                "[ToolCompressor] Compacted text block: {} → {} chars",
                text.len(),
                compacted.len()
            );
        } else {
            cleaned_blocks.push(block.clone());
            used_chars += 100; // 估算非文本块大小
        }
        
        if used_chars >= MAX_TOOL_RESULT_CHARS {
            break;
        }
    }
    
    if removed_image {
        cleaned_blocks.push(serde_json::json!({
            "type": "text",
            "text": "[image omitted to fit Antigravity prompt limits; use the file path in the previous text block]"
        }));
    }
    
    info!(
        "[ToolCompressor] Sanitization complete: {} → {} blocks, {} chars used",
        blocks.len(),
        cleaned_blocks.len(),
        used_chars
    );
    
    *blocks = cleaned_blocks;
}

/// 检测是否是 base64 图片块
fn is_base64_image(block: &Value) -> bool {
    block.get("type").and_then(|v| v.as_str()) == Some("image")
        && block
            .get("source")
            .and_then(|s| s.get("type"))
            .and_then(|v| v.as_str())
            == Some("base64")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_text() {
        let text = "a".repeat(300_000);
        let result = truncate_text_safe(&text, 200_000);
        assert!(result.len() < 210_000); // 包含截断提示
        assert!(result.contains("[truncated"));
        assert!(result.contains("100000 chars]"));
    }

    #[test]
    fn test_truncate_text_no_truncation() {
        let text = "short text";
        let result = truncate_text_safe(text, 1000);
        assert_eq!(result, text);
    }

    #[test]
    fn test_compact_browser_snapshot() {
        let snapshot = format!("page snapshot: {}", "ref=abc ".repeat(10_000));
        let result = compact_tool_result_text(&snapshot, 16_000);
        
        assert!(result.len() <= 16_500); // 允许一些 overhead
        assert!(result.contains("[HEAD]"));
        assert!(result.contains("[TAIL]"));
        assert!(result.contains("page snapshot summarized"));
    }

    #[test]
    fn test_compact_saved_output_notice() {
        let text = r#"result (150000 characters) exceeds maximum allowed tokens. Output has been saved to /tmp/output.txt
Format: JSON array with schema
Please read the file locally."#;
        
        let result = compact_tool_result_text(text, 500);
        println!("Result: {}", result);
        assert!(result.contains("150000 characters") || result.contains("150,000 characters"));
        assert!(result.contains("/tmp/output.txt"));
        assert!(result.contains("[tool_result omitted") || result.len() <= 500);
    }

    #[test]
    fn test_sanitize_tool_result_blocks() {
        let mut blocks = vec![
            serde_json::json!({
                "type": "text",
                "text": "a".repeat(100_000)
            }),
            serde_json::json!({
                "type": "text",
                "text": "b".repeat(150_000)
            }),
        ];

        sanitize_tool_result_blocks(&mut blocks);

        assert_eq!(blocks.len(), 2);
        // 第一个块应该保持原样
        assert_eq!(blocks[0]["text"].as_str().unwrap().len(), 100_000);
        // 第二个块应该被截断到 100,000 (200,000 - 100,000)
        assert!(blocks[1]["text"].as_str().unwrap().len() < 110_000);
    }

    #[test]
    fn test_sanitize_removes_base64_image() {
        let mut blocks = vec![
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
                }
            }),
            serde_json::json!({
                "type": "text",
                "text": "some text"
            }),
        ];

        sanitize_tool_result_blocks(&mut blocks);

        // 图片应该被移除,添加了提示文本
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "some text");
        assert!(blocks[1]["text"].as_str().unwrap().contains("[image omitted"));
    }

    #[test]
    fn test_is_base64_image() {
        let image_block = serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "data": "abc123"
            }
        });
        assert!(is_base64_image(&image_block));

        let text_block = serde_json::json!({
            "type": "text",
            "text": "hello"
        });
        assert!(!is_base64_image(&text_block));
    }
}
