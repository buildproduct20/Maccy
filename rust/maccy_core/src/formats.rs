use crate::{CoreError, Result};

/// Text-only HTML fallback: no renderer, URLs, scripts, styles or network access.
pub fn html_text(bytes: &[u8]) -> Result<String> {
    let input = std::str::from_utf8(bytes).map_err(|_| CoreError::InvalidInput)?;
    let mut output = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    let mut quote = None;
    let mut hidden: Option<&str> = None;
    for c in input.chars() {
        if in_tag {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() && (c == '\'' || c == '"') {
                quote = Some(c);
            } else if quote.is_none() && c == '>' {
                let trimmed = tag.trim();
                let closing = trimmed.starts_with('/');
                let name = trimmed
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/');
                if closing && hidden == Some(name) {
                    hidden = None;
                } else if !closing && hidden.is_none() {
                    hidden = match name {
                        "script" => Some("script"),
                        "style" => Some("style"),
                        "head" => Some("head"),
                        _ => None,
                    };
                }
                if hidden.is_none()
                    && matches!(name, "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3")
                    && !output.ends_with('\n')
                {
                    output.push('\n');
                }
                tag.clear();
                in_tag = false;
            } else if tag.len() < 256 && quote.is_none() {
                tag.extend(c.to_lowercase());
            }
        } else if c == '<' {
            in_tag = true;
        } else if hidden.is_none() {
            output.push(c);
        }
        if output.len() > 2 * 1024 * 1024 {
            return Err(CoreError::InvalidInput);
        }
    }
    let mut decoded = String::new();
    let mut rest = output.as_str();
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest
            .as_bytes()
            .iter()
            .take(17)
            .position(|&byte| byte == b';')
        else {
            decoded.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let scalar = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                u32::from_str_radix(&entity[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if entity.starts_with('#') => {
                entity[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(c) = scalar {
            decoded.push(c);
        } else {
            decoded.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    decoded.push_str(rest);
    Ok(decoded.trim_matches('\n').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_is_readable_without_rendering_or_loading_external_resources() {
        let html = br#"<html><head><style>private</style></head><body><p>Hello &amp; &#x1f680;</p><script src="https://example.invalid/x">secret()</script><a href="file:///must-not-open">world</a><br>next</body></html>"#;
        assert_eq!(html_text(html).unwrap(), "Hello & 🚀\nworld\nnext");
        assert_eq!(html_text(b"<b>foo</b>").unwrap(), "foo");
        assert_eq!(
            html_text(b"literal &unknown; &lt;x&gt;").unwrap(),
            "literal &unknown; <x>"
        );
    }
}
