//! HTML strip + entity decode + URL extraction for Google Calendar
//! event descriptions. Tiny state machine; no external HTML parser.
//!
//! UTF-8 safe: all iteration goes through `char_indices()`. Real-world
//! Google descriptions are dense with `&nbsp;` (which decodes to `\u{a0}`,
//! a 2-byte char) and `&#39;`, plus the occasional emoji.

const BLOCK_TAGS: &[&str] = &["p", "div", "li", "br", "tr", "h1", "h2", "h3", "h4", "blockquote"];

/// Strip HTML tags and decode entities. Block-level tags emit newlines.
/// Runs of newlines collapse to one; leading / trailing whitespace trimmed.
pub fn strip_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut iter = raw.char_indices().peekable();
    while let Some((i, ch)) = iter.next() {
        if ch == '<' {
            // Find the matching '>' (ASCII, safe to byte-search).
            let after = i + 1;
            let Some(rel_end) = raw[after..].find('>') else { break; };
            let end = after + rel_end;
            let tag = &raw[after..end];
            let lower = tag.to_ascii_lowercase();
            let name = lower
                .trim_start_matches('/')
                .split_whitespace().next().unwrap_or("")
                .trim_end_matches('/');
            if BLOCK_TAGS.contains(&name) && !out.ends_with('\n') {
                out.push('\n');
            }
            // Advance iter past the '>'. Char width of '>' is 1.
            while let Some(&(j, _)) = iter.peek() {
                if j > end { break; }
                iter.next();
            }
        } else if ch == '&' {
            let after = i + 1;
            if let Some(rel_semi) = raw[after..].find(';') {
                let semi = after + rel_semi;
                let entity = &raw[after..semi];
                if let Some(decoded) = decode_entity(entity) {
                    out.push_str(&decoded);
                    while let Some(&(j, _)) = iter.peek() {
                        if j > semi { break; }
                        iter.next();
                    }
                    continue;
                }
            }
            out.push('&');
        } else {
            out.push(ch);
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_nl = true;
    for ch in out.chars() {
        if ch == '\n' {
            if !prev_nl {
                collapsed.push('\n');
            }
            prev_nl = true;
        } else {
            collapsed.push(ch);
            prev_nl = false;
        }
    }
    collapsed.trim().to_string()
}

fn decode_entity(name: &str) -> Option<String> {
    match name {
        "amp" => Some("&".into()),
        "lt" => Some("<".into()),
        "gt" => Some(">".into()),
        "nbsp" => Some("\u{a0}".into()),
        "quot" => Some("\"".into()),
        "apos" => Some("'".into()),
        _ => {
            if let Some(rest) = name.strip_prefix('#') {
                let n: u32 = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    rest.parse().ok()?
                };
                char::from_u32(n).map(|c| c.to_string())
            } else {
                None
            }
        }
    }
}

/// Extract https?:// URLs from a string. Trims trailing punctuation.
/// Dedupes preserving first-seen order. UTF-8 safe.
pub fn extract_urls(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        let lower_head: String = rest.chars().take(8).collect::<String>().to_ascii_lowercase();
        if lower_head.starts_with("http://") || lower_head.starts_with("https://") {
            // Walk forward by chars until whitespace or terminator.
            let mut end = i;
            for (off, ch) in rest.char_indices() {
                if ch.is_whitespace() || ch == '<' || ch == '>' || ch == '"' || ch == '\'' {
                    break;
                }
                end = i + off + ch.len_utf8();
            }
            let mut url = raw[i..end].to_string();
            while let Some(last) = url.chars().last() {
                if matches!(last, '.' | ',' | ';' | ':' | ')' | ']') {
                    url.pop();
                } else {
                    break;
                }
            }
            if (url.starts_with("http://") || url.starts_with("https://")) && !out.contains(&url) {
                out.push(url);
            }
            i = end.max(i + 1);
        } else {
            // Advance by one char (UTF-8 safe).
            let step = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            i += step;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_basic_tags_and_paragraphs() {
        let s = strip_html("<p>Hello <b>world</b></p><p>Line 2</p>");
        assert_eq!(s, "Hello world\nLine 2");
    }

    #[test]
    fn entity_decode_named() {
        assert!(strip_html("&amp;").contains('&'));
        assert!(strip_html("&lt;").contains('<'));
        assert!(strip_html("&gt;").contains('>'));
        assert!(strip_html("&quot;").contains('"'));
        assert!(strip_html("&apos;").contains('\''));
    }

    #[test]
    fn numeric_entity_decode() {
        assert!(strip_html("&#39;").contains('\''));
        assert!(strip_html("&#x27;").contains('\''));
    }

    #[test]
    fn extract_real_otter_url() {
        let desc = r#"Open Otter meeting notes:<br><a href="https://otter.ai/mt/example-transcript-id" target="_blank">https://otter.ai/mt/example-transcript-id</a>"#;
        let urls = extract_urls(desc);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://otter.ai/mt/example-transcript-id");
    }

    #[test]
    fn extract_dedups_preserving_order() {
        let s = "see https://a.com and https://b.com and https://a.com again";
        let urls = extract_urls(s);
        assert_eq!(urls, vec!["https://a.com".to_string(), "https://b.com".to_string()]);
    }

    #[test]
    fn trim_trailing_punctuation() {
        let urls = extract_urls("ping https://foo.com. then https://bar.com,");
        assert_eq!(urls, vec!["https://foo.com".to_string(), "https://bar.com".to_string()]);
    }

    // REGRESSION TEST: non-breaking space + multi-byte chars don't panic.
    // The real Daily Huddle description has `&nbsp;` (decodes to `\u{a0}`,
    // 2 bytes in UTF-8); the old byte-indexed code panicked the moment
    // the strip/extract loop tried to slice across one of these.
    #[test]
    fn strip_html_handles_nbsp() {
        let raw = "<p>On Track:\u{a0}</p><p>Off Track:\u{a0}details</p>";
        let s = strip_html(raw);
        assert!(s.contains("On Track:"));
        assert!(s.contains("Off Track:"));
        // No panic = pass.
    }

    #[test]
    fn extract_urls_handles_nbsp_around_link() {
        let raw = "checkin here:\u{a0}https://otter.ai/foo\u{a0}thanks";
        let urls = extract_urls(raw);
        assert_eq!(urls, vec!["https://otter.ai/foo".to_string()]);
    }

    #[test]
    fn extract_urls_handles_emoji_around_link() {
        // 📹 is 4 bytes; ensure we step over it cleanly.
        let raw = "📹 https://meet.google.com/abc 📹";
        let urls = extract_urls(raw);
        assert_eq!(urls, vec!["https://meet.google.com/abc".to_string()]);
    }

    #[test]
    fn strip_html_emoji_passthrough() {
        let s = strip_html("<p>📹 meeting</p>");
        assert_eq!(s, "📹 meeting");
    }

    #[test]
    fn full_daily_huddle_description_doesnt_panic() {
        // Approximation of the real description (the bytes-103-105 NBSP is in there).
        let raw = "We'll go in alphabetical order. You have two options for your checkin here:<br><br><ul><li><b>On Track:\u{a0}</b>You understand your priorities for the day and the rest of the week and have everything you need to complete your assignments.\u{a0}</li><li><b>Off Track:\u{a0}</b>You don't have what you need.</li></ul><br><br>Open Otter meeting notes:<br><a href=\"https://otter.ai/mt/example-transcript-id\" target=\"_blank\">https://otter.ai/mt/example-transcript-id</a>";
        let stripped = strip_html(raw);
        assert!(stripped.contains("checkin here:"));
        assert!(stripped.contains("On Track:"));
        let urls = extract_urls(raw);
        assert!(urls.iter().any(|u| u.contains("otter.ai/mt/example-transcript-id")));
    }
}
