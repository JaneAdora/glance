//! HTML strip + entity decode + URL extraction for Google Calendar
//! event descriptions. Tiny state machine; no external HTML parser.

const BLOCK_TAGS: &[&str] = &["p", "div", "li", "br", "tr", "h1", "h2", "h3", "h4", "blockquote"];

/// Strip HTML tags and decode entities. Block-level tags emit newlines.
/// Runs of newlines collapse to one; leading / trailing whitespace trimmed.
pub fn strip_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'<' {
            let tag_start = i + 1;
            let close = raw[tag_start..].find('>').map(|p| tag_start + p);
            let Some(end) = close else { break; };
            let tag = &raw[tag_start..end];
            let lower = tag.to_ascii_lowercase();
            let name = lower
                .trim_start_matches('/')
                .split_whitespace().next().unwrap_or("")
                .trim_end_matches('/');
            if BLOCK_TAGS.contains(&name) && !out.ends_with('\n') {
                out.push('\n');
            }
            i = end + 1;
        } else if c == b'&' {
            let semi = raw[i..].find(';').map(|p| i + p);
            if let Some(s) = semi {
                let entity = &raw[i + 1..s];
                if let Some(decoded) = decode_entity(entity) {
                    out.push_str(&decoded);
                    i = s + 1;
                    continue;
                }
            }
            out.push('&');
            i += 1;
        } else {
            out.push(c as char);
            i += 1;
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
        "nbsp" => Some(" ".into()),
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

/// Extract https?:// URLs from a string. Trims trailing punctuation. Dedupes
/// preserving first-seen order.
pub fn extract_urls(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // case-insensitive "http" prefix check
        let look = (bytes.len() - i).min(8);
        let head = &raw[i..i + look];
        if head.to_ascii_lowercase().starts_with("http://")
            || head.to_ascii_lowercase().starts_with("https://")
        {
            let start = i;
            let mut end = i;
            while end < bytes.len() {
                let c = bytes[end];
                if c.is_ascii_whitespace() || c == b'<' || c == b'>' || c == b'"' || c == b'\'' {
                    break;
                }
                end += 1;
            }
            let mut url = raw[start..end].to_string();
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
            i = end.max(start + 1);
        } else {
            i += 1;
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
}
