//! 现代命名空间初始化闭包及旧命名启发式的编译器回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, disasm, file::AbcFile};

use super::common;

const FIXTURES: &[(&str, &str)] = &[
    ("debug", "namespace_recovery/debug.abc"),
    ("release", "namespace_recovery/release.abc"),
];

fn record(file: &AbcFile) -> &pedal_abc_bridge::file::ClassInfo {
    file.classes
        .iter()
        .find(|record| record.name.contains("NamespaceRecovery"))
        .expect("namespace record")
}

fn assert_namespace(mode: &str, source: &str) {
    assert!(
        !source.contains("class "),
        "namespace must remain an initialized object: {mode}\n{source}"
    );
    for reference in [
        "shaderHandler",
        "firstCounter",
        "secondCounter",
        ".resolve =",
        ".makeCounter =",
        ".makeCounter(3)",
        ".makeCounter(10)",
        "\"shader\"",
        "async ",
        "await ",
        "return () =>",
    ] {
        assert!(
            source.contains(reference),
            "missing {reference}: {mode}\n{source}"
        );
    }
    for unresolved in [
        "Conservative fallback",
        "lex0_",
        "lex1_",
        "unresolved closure",
    ] {
        assert!(
            !source.contains(unresolved),
            "lost captured binding {unresolved}: {mode}\n{source}"
        );
    }
}

#[test]
fn namespace_compiler_fixtures_decode_every_method() {
    for (mode, path) in FIXTURES {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let methods = &record(&file).methods;
        assert_eq!(
            methods.len(),
            5,
            "{mode}: namespace, async method, factory, returned closure and initializer"
        );
        for method in methods {
            let code = method.code.as_ref().expect("compiler method body");
            disasm::decode(code.instructions(&file.data))
                .unwrap_or_else(|error| panic!("{mode} {}: {error}", method.name));
        }
    }
}

#[test]
fn namespaces_preserve_private_captures_async_export_and_two_closure_instances() {
    for (mode, path) in FIXTURES {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let source = decompile::decompile_record(&file, record(&file))
            .unwrap()
            .source;
        assert_namespace(mode, &source);
    }
}

#[test]
fn production_style_namespace_names_do_not_trigger_legacy_class_synthesis() {
    for (mode, path) in FIXTURES {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let mut namespace = record(&file).clone();
        // SDK 26 的匿名函数名使用哈希；生产语料的旧编译器把 namespace
        // 初始化闭包命名为 #&#Name。仅修改传入 record 的分类元数据：
        // 原 ABC 与 method_at 读取保持一致，闭包渲染仍使用真实编译器数据。
        let initializer = namespace
            .methods
            .iter_mut()
            .find(|method| {
                method.code.as_ref().is_some_and(|code| {
                    disasm::decode(code.instructions(&file.data))
                        .unwrap()
                        .iter()
                        .filter(|i| i.meta.mnemonic == "stobjbyname")
                        .count()
                        == 2
                })
            })
            .expect("namespace initializer with two exported property stores");
        initializer.name = "#&#shaderHandler".into();
        for method in &mut namespace.methods {
            if matches!(method.name.as_str(), "resolve" | "makeCounter") {
                method.name = format!("#&@0*#{}", method.name);
            }
        }
        // 同时提供初始化器与成员族，确保未加 Modern/Legacy 限制时会触发旧启发式。
        assert!(namespace.methods.iter().any(|m| matches!(
            decompile::classify_method(&m.name),
            decompile::MethodFamily::ClassInit(_)
        )));
        assert_eq!(
            namespace
                .methods
                .iter()
                .filter(|m| matches!(
                    decompile::classify_method(&m.name),
                    decompile::MethodFamily::ClassMethod { .. }
                ))
                .count(),
            2
        );
        let source = decompile::decompile_record(&file, &namespace)
            .unwrap()
            .source;
        assert_namespace(mode, &source);
    }
}
