/// Escapes text for safe interpolation into HTML element content and quoted attribute values.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_script_tag() {
        let out = escape_html("<script>alert(1)</script>");
        assert_eq!(out, "&lt;script&gt;alert(1)&lt;/script&gt;");
        assert!(!out.contains('<'));
    }

    #[test]
    fn escapes_attribute_quote_breakout() {
        let out = escape_html(r#"" onmouseover='alert(1)' x=""#);
        assert!(!out.contains('"'));
        assert!(!out.contains('\''));
        assert_eq!(out, "&quot; onmouseover=&#39;alert(1)&#39; x=&quot;");
    }

    #[test]
    fn escapes_ampersand_first_without_double_escaping_output() {
        assert_eq!(escape_html("a&b"), "a&amp;b");
        assert_eq!(escape_html("&lt;"), "&amp;lt;");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        let s = "Plain text 123 - 授权失败 ✅";
        assert_eq!(escape_html(s), s);
    }
}
