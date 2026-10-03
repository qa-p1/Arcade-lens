use std::ops::Range;
use std::sync::LazyLock;

use lens_core::value::UrlValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{trim_token, TextInput};

static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:(?:https?|ftp|wss?)://|www\.)[^\s<>"'`{}|\\^]+"#).unwrap()
});

/// Every URL in `text` with its byte range. Shared with recognizers that
/// must not report pieces of a URL (domains, emails, paths).
pub fn find(text: &str) -> Vec<(Range<usize>, UrlValue)> {
    URL.find_iter(text)
        .filter_map(|m| {
            let raw = trim_token(m.as_str());
            let range = m.start()..m.start() + raw.len();
            let (url, scheme) = match raw.find("://") {
                Some(i) => (raw.to_string(), raw[..i].to_ascii_lowercase()),
                None => (format!("https://{raw}"), "https".to_string()),
            };
            let host = host_of(&url)?;
            // `www.` alone or a host without a dot (other than localhost) is noise.
            if !host.contains('.') && host != "localhost" && !host.starts_with('[') {
                return None;
            }
            Some((range, UrlValue { url, scheme, host: Some(host) }))
        })
        .collect()
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = &url[url.find("://")? + 3..];
    let authority = rest.split(['/', '?', '#']).next()?;
    let hostport = authority.rsplit('@').next()?;
    let host = if hostport.starts_with('[') {
        hostport.split(']').next().map(|h| format!("{h}]"))?
    } else {
        hostport.split(':').next()?.to_string()
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    find(input.text)
        .into_iter()
        .map(|(range, u)| {
            let explicit = input.text[range.clone()].contains("://");
            let host = u.host.clone().unwrap_or_default();
            Detection::new(caps::URL, Value::Url(u)).span(range).confidence(if explicit { 0.95 } else { 0.85 }).detail("Domain", host)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::texts;

    #[test]
    fn finds_urls_in_prose() {
        assert_eq!(
            texts(detect, "Docs at https://docs.rs/regex/latest/regex/#syntax, or (www.example.com/a_(b))."),
            vec!["https://docs.rs/regex/latest/regex/#syntax", "https://www.example.com/a_(b)"]
        );
        assert_eq!(texts(detect, "ws://localhost:8080/socket"), vec!["ws://localhost:8080/socket"]);
        assert!(texts(detect, "just text, www. nothing").is_empty());
    }

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://user:pw@Example.COM:8443/x?y").as_deref(), Some("example.com"));
        assert_eq!(host_of("http://[::1]:8080/").as_deref(), Some("[::1]"));
    }
}
