//! 文本匹配区间
//!
//! @author sky

/// 查找匹配项（字节偏移）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    /// 起始字节偏移
    pub start: usize,
    /// 结束字节偏移
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

pub struct SearchPattern {
    pattern: Option<regex::Regex>,
    word_start: Option<regex::Regex>,
}

impl SearchPattern {
    pub fn new(query: &str, options: SearchOptions) -> Result<Self, String> {
        if query.is_empty() {
            return Ok(Self {
                pattern: None,
                word_start: None,
            });
        }
        let expression = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let pattern = compile(&expression, options)?;
        if !options.whole_word {
            return Ok(Self {
                pattern: Some(pattern),
                word_start: None,
            });
        }
        let boundary = r"[^\p{Alphabetic}\p{M}\p{N}\p{Pc}\u{200C}\u{200D}$]";
        let suffix = format!(r"(?:{boundary}|\z)");
        let wrap = |prefix: &str| {
            let wrapped = format!("{prefix}({expression}){suffix}");
            compile(&wrapped, options).or_else(|error| {
                if expression.contains('#') {
                    compile(&format!("{prefix}({expression}\n){suffix}"), options)
                        .map_err(|_| error)
                } else {
                    Err(error)
                }
            })
        };
        Ok(Self {
            pattern: Some(wrap(boundary)?),
            word_start: Some(wrap(r"\A")?),
        })
    }

    pub fn find_iter<'a>(&'a self, text: &'a str) -> impl Iterator<Item = FindMatch> + 'a {
        let mut plain = self.pattern.as_ref().map(|pattern| pattern.find_iter(text));
        let mut first = true;
        let mut finished = false;
        let mut offset = 0;
        std::iter::from_fn(move || {
            let Some(word_start) = &self.word_start else {
                return plain
                    .as_mut()?
                    .find(|found| !found.is_empty())
                    .map(|found| FindMatch {
                        start: found.start(),
                        end: found.end(),
                    });
            };
            if finished {
                return None;
            }
            let pattern = self.pattern.as_ref()?;
            loop {
                let found = if first {
                    first = false;
                    word_start
                        .captures(text)
                        .and_then(|captures| captures.get(1))
                } else {
                    let Some(captures) = pattern.captures_at(text, offset) else {
                        finished = true;
                        return None;
                    };
                    captures.get(1)
                };
                let Some(found) = found else {
                    continue;
                };
                if found.is_empty() {
                    if found.end() == text.len() {
                        finished = true;
                        return None;
                    }
                    offset = found.end();
                    continue;
                }
                finished = found.end() == text.len();
                offset = text[..found.end()]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index);
                return Some(FindMatch {
                    start: found.start(),
                    end: found.end(),
                });
            }
        })
    }
}

fn compile(expression: &str, options: SearchOptions) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(expression)
        .case_insensitive(!options.case_sensitive)
        .multi_line(true)
        .build()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(query: &str, text: &str, options: SearchOptions) -> Vec<(usize, usize)> {
        SearchPattern::new(query, options)
            .unwrap()
            .find_iter(text)
            .map(|found| (found.start, found.end))
            .collect()
    }

    #[test]
    fn literal_search_escapes_regex_syntax() {
        assert_eq!(
            ranges("a.b", "A.B axb a.b", SearchOptions::default()),
            vec![(0, 3), (8, 11)]
        );
    }

    #[test]
    fn case_sensitive_search_preserves_case() {
        assert_eq!(
            ranges(
                "Field",
                "field Field FIELD",
                SearchOptions {
                    case_sensitive: true,
                    ..SearchOptions::default()
                }
            ),
            vec![(6, 11)]
        );
    }

    #[test]
    fn unicode_case_insensitive_matches_have_byte_offsets() {
        assert_eq!(
            ranges("é", "中文É é", SearchOptions::default()),
            vec![(6, 8), (9, 11)]
        );
    }

    #[test]
    fn whole_words_respect_code_identifier_boundaries() {
        let text = "foo foo_bar $foo foo$ foobar 中foo foo中 foo()";
        let matches = ranges(
            "foo",
            text,
            SearchOptions {
                whole_word: true,
                ..SearchOptions::default()
            },
        );
        assert_eq!(matches, vec![(0, 3), (text.len() - 5, text.len() - 2)]);
    }

    #[test]
    fn whole_words_respect_combining_marks() {
        assert_eq!(
            ranges(
                "e",
                "e\u{301} e",
                SearchOptions {
                    whole_word: true,
                    ..SearchOptions::default()
                }
            ),
            vec![(4, 5)]
        );
    }

    #[test]
    fn regex_supports_line_anchors_and_reports_invalid_patterns() {
        let options = SearchOptions {
            regex: true,
            ..SearchOptions::default()
        };
        assert_eq!(
            ranges(r"^foo\d+", "foo1\nfoo22\nxfoo3", options),
            vec![(0, 4), (5, 10)]
        );
        assert!(SearchPattern::new("[", options).is_err());
    }

    #[test]
    fn empty_queries_and_zero_width_matches_are_ignored() {
        assert!(ranges("", "text", SearchOptions::default()).is_empty());
        let options = SearchOptions {
            regex: true,
            ..SearchOptions::default()
        };
        assert!(ranges(r"\b", "text", options).is_empty());
        assert_eq!(ranges("a*", "baa", options), vec![(1, 3)]);
    }

    #[test]
    fn multiline_literal_matches_preserve_exact_range() {
        assert_eq!(
            ranges("中\n文", "a中\n文b", SearchOptions::default()),
            vec![(1, 8)]
        );
    }

    #[test]
    fn whole_word_regex_retries_alternatives_and_lazy_repetitions() {
        let options = SearchOptions {
            regex: true,
            whole_word: true,
            ..SearchOptions::default()
        };
        assert_eq!(
            ranges("foo|foobar", "foobar foo", options),
            vec![(0, 6), (7, 10)]
        );
        assert_eq!(ranges("a.*?", "abc ax", options), vec![(0, 3), (4, 6)]);
        assert_eq!(
            ranges("(?P<word>foo|foobar)", "foobar foo", options),
            vec![(0, 6), (7, 10)]
        );
    }

    #[test]
    fn whole_word_matches_reuse_shared_boundaries() {
        let options = SearchOptions {
            whole_word: true,
            ..SearchOptions::default()
        };
        assert_eq!(
            ranges("foo", "foo foo,foo\nfoo", options),
            vec![(0, 3), (4, 7), (8, 11), (12, 15)]
        );
        assert_eq!(ranges(".", ".. .", options), vec![(0, 1), (1, 2), (3, 4)]);
        assert_eq!(
            ranges("中\n文", "中\n文 中\n文", options),
            vec![(0, 7), (8, 15)]
        );
    }

    #[test]
    fn whole_word_zero_width_matches_advance_on_unicode_boundaries() {
        let options = SearchOptions {
            regex: true,
            whole_word: true,
            ..SearchOptions::default()
        };
        assert_eq!(
            ranges(r"(?:\b|é+?)", " ÉÉ É ", options),
            vec![(1, 5), (6, 8)]
        );
        assert_eq!(ranges("a*", "  aa  a ", options), vec![(2, 4), (6, 7)]);
        assert!(ranges(r"\b", "中文 $foo  ", options).is_empty());
        assert!(ranges("^|$", "", options).is_empty());
    }

    #[test]
    fn whole_word_regex_preserves_flags_anchors_and_validation() {
        let options = SearchOptions {
            regex: true,
            whole_word: true,
            ..SearchOptions::default()
        };
        assert_eq!(
            ranges(r"^foo\d+", "foo1\nfoo22\nxfoo3", options),
            vec![(0, 4), (5, 10)]
        );
        assert_eq!(
            ranges("(?x) foo | foobar # alternatives", "foobar foo", options),
            vec![(0, 6), (7, 10)]
        );
        assert!(SearchPattern::new("foo)|bar|(", options).is_err());
    }
}
