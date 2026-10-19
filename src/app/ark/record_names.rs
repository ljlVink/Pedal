//! record 类型描述符的可读名称与可导出文件名。
//!
//! @author ljlVink

/// 仅解析已支持的完整描述符；未知或损坏的编码保留原名。
enum Descriptor<'a> {
    Reference(&'a str),
    Array(usize, Box<Descriptor<'a>>),
    Union(Vec<Descriptor<'a>>),
}

impl Descriptor<'_> {
    fn has_union(&self) -> bool {
        match self {
            Self::Reference(_) => false,
            Self::Array(_, element) => element.has_union(),
            Self::Union(_) => true,
        }
    }

    fn render(&self, in_union: bool, short: bool) -> String {
        match self {
            Self::Reference(name) => {
                let name = without_version(name);
                if !in_union {
                    return name.to_string();
                }
                match name {
                    "std/core/Null" => "null".into(),
                    "std/core/Undefined" => "undefined".into(),
                    _ if short => name.rsplit('/').next().unwrap_or(name).to_string(),
                    _ => name.replace('/', "."),
                }
            }
            Self::Array(dimensions, element) => {
                format!(
                    "{}{}",
                    element.render(in_union, short),
                    "[]".repeat(*dimensions)
                )
            }
            Self::Union(members) => {
                let members: Vec<_> = members.iter().map(|m| m.render(true, short)).collect();
                format!("({})", members.join(" | "))
            }
        }
    }
}

fn parse_prefix(s: &str, depth: usize) -> Option<(Descriptor<'_>, &str)> {
    if depth >= 32 {
        return None;
    }
    let dimensions = s.bytes().take_while(|&b| b == b'[').count();
    if dimensions > 0 {
        let (element, rest) = parse_prefix(&s[dimensions..], depth + 1)?;
        return Some((Descriptor::Array(dimensions, Box::new(element)), rest));
    }
    if let Some(mut rest) = s.strip_prefix("{U") {
        let mut members = Vec::new();
        while !rest.starts_with('}') {
            let (member, next) = parse_prefix(rest, depth + 1)?;
            members.push(member);
            rest = next;
        }
        return (members.len() >= 2).then(|| (Descriptor::Union(members), &rest[1..]));
    }
    let (name, rest) = s.strip_prefix('L')?.split_once(';')?;
    (!name.is_empty()).then_some((Descriptor::Reference(name), rest))
}

fn parse_descriptor(s: &str) -> Option<Descriptor<'_>> {
    let (descriptor, rest) = parse_prefix(s, 0)?;
    rest.is_empty().then_some(descriptor)
}

fn without_version(name: &str) -> &str {
    if let Some((base, version)) = name.rsplit_once('&')
        && !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return base;
    }
    name
}

/// 完整可读名称用于源码说明；普通类型保留包路径，联合成员用点分限定名。
pub(super) fn display_record_name(name: &str) -> String {
    parse_descriptor(name).map_or_else(
        || without_version(name).to_string(),
        |descriptor| descriptor.render(false, false),
    )
}

/// 联合类型使用单个文件，界面保留 `|`，磁盘名称用 `or` 并以 record 偏移消歧。
pub(super) fn record_entry_name(name: &str, offset: u32) -> (String, Option<String>) {
    let Some(descriptor) = parse_descriptor(name).filter(Descriptor::has_union) else {
        return (display_record_name(name), None);
    };
    let label = descriptor.render(false, true);
    let stem = label.replace('|', "or");
    let mut safe = String::new();
    for c in stem.chars() {
        if safe.len() + c.len_utf8() > 120 {
            break;
        }
        safe.push(if c.is_control() || "<>:\"/\\|?*".contains(c) {
            '_'
        } else {
            c
        });
    }
    (format!("{safe}@{offset:x}"), Some(format!("{label}.ets")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_members_and_array_precedence_are_preserved() {
        assert_eq!(
            display_record_name("{UL@ohos/base/BusinessError;Lstd/core/Null;}"),
            "(@ohos.base.BusinessError | null)"
        );
        assert_eq!(
            display_record_name("{U[Lpkg/A;Lpkg/B;}"),
            "(pkg.A[] | pkg.B)"
        );
        assert_eq!(
            display_record_name("[{ULpkg/A;Lpkg/B;}"),
            "(pkg.A | pkg.B)[]"
        );
        assert_eq!(
            display_record_name("{ULpkg/A;{ULpkg/B;Lstd/core/Undefined;}}"),
            "(pkg.A | (pkg.B | undefined))"
        );
        for malformed in ["{U}", "{ULx;}", "{ULx;Ly;", "{ULx;Ly;}extra", "{ULx;Q}"] {
            assert_eq!(display_record_name(malformed), malformed);
            assert_eq!(record_entry_name(malformed, 1).1, None);
        }
    }

    #[test]
    fn union_filenames_are_bounded_portable_and_do_not_merge_same_short_names() {
        let (first, label) = record_entry_name("{ULfoo/A;Lstd/core/Null;}", 0x100);
        let (second, _) = record_entry_name("{ULbar/A;Lstd/core/Null;}", 0x200);
        assert_eq!(label.as_deref(), Some("(A | null).ets"));
        assert_ne!(first, second);
        assert_eq!(first, "(A or null)@100");
        let long = format!("{{UL{};Lstd/core/Null;}}", "名称".repeat(100));
        let (filename, _) = record_entry_name(&long, 0x300);
        assert!(filename.len() <= 125);
        assert!(
            !filename
                .chars()
                .any(|c| "<>:\"/\\|?*".contains(c) || c.is_control())
        );
        assert!(filename.ends_with("@300"));
    }
}
