use serde::Deserialize;

const MAX_FORMATTED_SIZE: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

pub(crate) fn is_json_file(path: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("info")
        })
}

pub(crate) fn format_json(text: &str) -> Option<String> {
    if text.len() > MAX_FORMATTED_SIZE {
        return None;
    }
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut offset = 0;
    let mut depth = 0usize;
    let mut previous = None;
    while let Some(&byte) = bytes.get(offset) {
        if is_whitespace(byte) {
            offset += 1;
            continue;
        }
        match byte {
            b'"' => {
                let start = offset;
                offset += 1;
                loop {
                    match *bytes.get(offset)? {
                        b'"' => {
                            offset += 1;
                            break;
                        }
                        b'\\' => offset += 2,
                        _ => offset += 1,
                    }
                }
                append(&mut output, &text[start..offset])?;
            }
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return None;
                }
                append(&mut output, &text[offset..offset + 1])?;
                offset += 1;
                let closing = if byte == b'{' { b'}' } else { b']' };
                let next = bytes[offset..].iter().find(|&&byte| !is_whitespace(byte));
                if next != Some(&closing) {
                    newline(&mut output, depth)?;
                }
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                let opening = if byte == b'}' { b'{' } else { b'[' };
                if previous != Some(opening) {
                    newline(&mut output, depth)?;
                }
                append(&mut output, &text[offset..offset + 1])?;
                offset += 1;
            }
            b',' => {
                append(&mut output, ",")?;
                newline(&mut output, depth)?;
                offset += 1;
            }
            b':' => {
                append(&mut output, ": ")?;
                offset += 1;
            }
            _ => {
                let start = offset;
                while bytes.get(offset).is_some_and(|&byte| {
                    !is_whitespace(byte)
                        && !matches!(byte, b'{' | b'[' | b'}' | b']' | b',' | b':' | b'"')
                }) {
                    offset += 1;
                }
                append(&mut output, &text[start..offset])?;
            }
        }
        previous = Some(byte);
    }
    if depth != 0 {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    serde::de::IgnoredAny::deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(output)
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn append(output: &mut String, value: &str) -> Option<()> {
    if output.len().checked_add(value.len())? > MAX_FORMATTED_SIZE {
        return None;
    }
    output.push_str(value);
    Some(())
}

fn newline(output: &mut String, depth: usize) -> Option<()> {
    let spaces = depth.checked_mul(2)?;
    if output.len().checked_add(spaces + 1)? > MAX_FORMATTED_SIZE {
        return None;
    }
    output.push('\n');
    output.extend(std::iter::repeat_n(' ', spaces));
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_json_and_info_extensions_including_hidden_names() {
        for path in [
            "module.json",
            "pack.INFO",
            "resources/.pages.info",
            ".info",
            r"C:\sample\file.JSON",
        ] {
            assert!(is_json_file(path), "missed {path}");
        }
        for path in [
            "info",
            "config.json.txt",
            "file.png",
            "folder.json/file",
            "a.info/",
        ] {
            assert!(!is_json_file(path), "matched {path}");
        }
    }

    #[test]
    fn formats_nested_json_and_preserves_empty_containers() {
        let source = " { \"pages\" : [\"pages/Index\",{\"empty\": [ ], \"options\": { }}],\"enabled\":true } \r\n";
        let expected = "{\n  \"pages\": [\n    \"pages/Index\",\n    {\n      \"empty\": [],\n      \"options\": {}\n    }\n  ],\n  \"enabled\": true\n}";
        assert_eq!(format_json(source).as_deref(), Some(expected));
        assert_eq!(format_json(expected).as_deref(), Some(expected));
        for (source, expected) in [(" [ ] ", "[]"), (" { } ", "{}"), (" true ", "true")] {
            assert_eq!(format_json(source).as_deref(), Some(expected));
        }
    }

    #[test]
    fn preserves_key_order_duplicates_and_numeric_literals() {
        let source = r#"{"z":1844674407370955161600001,"a":-0,"z":1.2300e+999,"b":-1E-0003}"#;
        let expected = "{\n  \"z\": 1844674407370955161600001,\n  \"a\": -0,\n  \"z\": 1.2300e+999,\n  \"b\": -1E-0003\n}";
        assert_eq!(format_json(source).as_deref(), Some(expected));
    }

    #[test]
    fn keeps_unicode_and_string_whitespace_and_escapes() {
        let source = r#"{"文字":"  中文 { [ ] } , : ","escape":"\"\\\/\b\f\n\r\t\u4e2d","pair":"\ud83d\ude00"}"#;
        let expected = concat!(
            "{\n  \"文字\": \"  中文 { [ ] } , : \",\n",
            "  \"escape\": \"\\\"\\\\\\/\\b\\f\\n\\r\\t\\u4e2d\",\n",
            "  \"pair\": \"\\ud83d\\ude00\"\n}"
        );
        assert_eq!(format_json(source).as_deref(), Some(expected));
    }

    #[test]
    fn rejects_invalid_json_and_excessive_nesting() {
        for source in [
            "",
            " ",
            "{",
            "[}",
            "{]",
            "[1,]",
            "{\"a\":}",
            "01",
            "NaN",
            "true false",
            "{\"a\":1} trailing",
            "{\"a\":\"\\q\"}",
            "\"unterminated",
            "\"trailing\\",
            "{\"a\":\"line\nbreak\"}",
            "{\"a\":1 // comment\n}",
        ] {
            assert!(format_json(source).is_none(), "accepted {source:?}");
        }
        let deeply_nested = format!(
            "{}0{}",
            "[".repeat(MAX_DEPTH + 1),
            "]".repeat(MAX_DEPTH + 1)
        );
        assert!(format_json(&deeply_nested).is_none());
    }
}
