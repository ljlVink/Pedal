//! 对象字面量方法展开与词法捕获回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile};

const FIXTURES: &[(&str, &[u8])] = &[
    ("debug", include_bytes!("fixtures/object_methods/debug.abc")),
    (
        "release",
        include_bytes!("fixtures/object_methods/release.abc"),
    ),
];

const METHODS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota",
];

fn has_lex_fallback(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.windows(3).enumerate().any(|(index, token)| {
        if token != b"lex" {
            return false;
        }
        let mut cursor = index + 3;
        let digits_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        cursor > digits_start && bytes.get(cursor) == Some(&b'_')
    })
}

#[test]
fn compiler_object_methods_are_embedded_once_and_capture_the_outer_parameter() {
    for (mode, bytes) in FIXTURES {
        let file = AbcFile::parse(bytes.to_vec()).expect("compiler fixture parses");
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains("ObjectMethods"))
            .expect("fixture record exists");
        let source = decompile::decompile_record(&file, record)
            .expect("fixture record decompiles")
            .source;

        assert!(
            source.contains("function makeObjectMethodGroups("),
            "{mode}:\n{source}"
        );
        assert!(
            source.contains("function makeDeepCapture("),
            "{mode}:\n{source}"
        );
        assert!(
            !source.contains("method@"),
            "{mode}: method placeholder remains\n{source}"
        );
        assert!(
            !source.contains("Unresolved closure @"),
            "{mode}: deeply nested closure remains unresolved\n{source}"
        );
        assert!(
            !has_lex_fallback(&source),
            "{mode}: unresolved lexical fallback\n{source}"
        );
        for method in METHODS {
            let signature = format!(" {method}() {{");
            assert_eq!(
                source.matches(&signature).count(),
                1,
                "{mode}: {method}\n{source}"
            );
            assert!(
                !source.contains(&format!("function {method}(")),
                "{mode}: {method} leaked as a top-level orphan\n{source}"
            );
        }
        let capture = source
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("return ")
                    .and_then(|line| line.strip_suffix(" + 7;"))
            })
            .expect("first method in the final group must read its captured parameter");
        assert!(
            source.contains(&format!("let {capture} = a0;")),
            "{mode}: captured binding {capture} has no declaration\n{source}"
        );
        for increment in [7, 11, 13] {
            assert!(
                source.contains(&format!("return {capture} + {increment};")),
                "{mode}: capture {capture} missing from method increment {increment}\n{source}"
            );
        }
        assert!(
            source.matches("() =>").count() >= 5,
            "{mode}: nested arrow chain was not embedded\n{source}"
        );
    }
}
