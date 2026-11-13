//! Colour codes written in text: `#RRGGBB`, `#RGB` and `rgb(r, g, b)` (LN-9).

use std::sync::LazyLock;

use lens_core::color::Rgb;
use lens_core::{Detection, RecognizeContext};
use regex::Regex;

use super::TextInput;
use crate::image::color::color_detection;

static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\w&#])(#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{3}))\b").unwrap());
static RGB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\brgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*[\d.]+%?\s*)?\)").unwrap());

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    for c in HEX.captures_iter(text) {
        let m = c.get(1).expect("group 1");
        if let Some(rgb) = Rgb::parse_hex(m.as_str()) {
            out.push(color_detection(rgb, 0.9).span(m.range()));
        }
    }
    for c in RGB.captures_iter(text) {
        let channel = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u8>().ok());
        if let (Some(r), Some(g), Some(b)) = (channel(1), channel(2), channel(3)) {
            out.push(color_detection(Rgb::new(r, g, b), 0.9).span(c.get(0).expect("match").range()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lens_core::Value;

    fn colors(text: &str) -> Vec<String> {
        detect(&TextInput { text, layout: None }, &super::super::testing::cx())
            .into_iter()
            .filter_map(|d| match d.value {
                Value::Color(c) => Some(c.hex()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hex_and_rgb_colours_in_text_are_found() {
        assert_eq!(colors("Brand colour #FF8800 and rgb(12, 34, 56)"), vec!["#FF8800", "#0C2238"]);
        assert_eq!(colors("short #0af, alpha rgba(1,2,3,0.5)"), vec!["#00AAFF", "#010203"]);
    }

    #[test]
    fn issue_numbers_entities_and_out_of_range_values_are_not_colours() {
        assert!(colors("see issue #123456789 and &#123; and rgb(300, 0, 0) and a#fff").is_empty());
    }
}
