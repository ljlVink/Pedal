//! 真实编译器生成的异步、异常及类归属回归测试。
//!
//! 小型 ABC 仅保存在本地；缺失时跳过，无需本地 SDK、Node 或 test_examples。
//! @author ljlVink

use pedal_abc_bridge::{decompile, disasm, file::AbcFile};

mod common;

const FIXTURES: &[(&str, &str)] = &[
    ("debug", "advanced_recovery/debug.abc"),
    ("release", "advanced_recovery/release.abc"),
];

fn source(bytes: &[u8]) -> String {
    source_record(bytes, "AdvancedRecovery")
}

fn source_record(bytes: &[u8], name: &str) -> String {
    let file = AbcFile::parse(bytes.to_vec()).expect("compiler fixture parses");
    let record = file
        .classes
        .iter()
        .find(|record| record.name.contains(name))
        .expect("fixture record exists");
    decompile::decompile_record(&file, record)
        .expect("fixture record decompiles")
        .source
}

/// 当前探针的字符串不含花括号，按平衡括号获取单个声明即可。
fn declaration<'a>(source: &'a str, prefix: &str) -> &'a str {
    let begin = source
        .find(prefix)
        .unwrap_or_else(|| panic!("missing {prefix}:\n{source}"));
    let opening = begin + source[begin..].find('{').expect("declaration body");
    let mut depth = 0;
    for (offset, character) in source[opening..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[begin..=opening + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed {prefix}:\n{source}");
}

#[test]
fn local_compiler_fixtures_decode_every_method() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let file = AbcFile::parse(bytes).expect("compiler fixture parses");
        let mut count = 0;
        for method in file.classes.iter().flat_map(|record| &record.methods) {
            if let Some(code) = &method.code {
                disasm::decode(code.instructions(&file.data))
                    .unwrap_or_else(|error| panic!("{mode} {}: {error}", method.name));
                count += 1;
            }
        }
        assert!(count >= 23, "{mode}: all probe methods must be present");
    }
}

#[test]
fn async_methods_restore_await_without_vm_resume_scaffolding() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source(&bytes);
        for name in [
            "awaitSuccess",
            "awaitFailure",
            "catchFinally",
            "finallyOverridesReturn",
            "nestedFinally",
            "catchThrows",
            "awaitLoop",
            "awaitInFinally",
        ] {
            let body = declaration(&source, &format!("async function {name}("));
            assert!(body.contains("await "), "{mode} {name}:\n{body}");
            for scaffold in [
                "asyncfunctionenter",
                "asyncfunctionawaituncaught",
                "suspendgenerator",
                "resumegenerator",
                "getresumemode",
            ] {
                assert!(!body.contains(scaffold), "{mode} {name}: {scaffold}");
            }
        }
    }
}

#[test]
fn return_override_and_nested_finalizers_remain_structured() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source(&bytes);
        let override_body = declaration(&source, "async function finallyOverridesReturn(");
        let finalizer = declaration(override_body, "finally {");
        assert!(finalizer.contains("return "), "{mode}:\n{override_body}");
        let nested = declaration(&source, "async function nestedFinally(");
        assert_eq!(nested.matches("finally {").count(), 2, "{mode}:\n{nested}");
        let rethrow = declaration(&source, "async function catchThrows(");
        assert!(rethrow.contains("catch ("), "{mode}:\n{rethrow}");
        let catch_body = declaration(rethrow, "catch (");
        assert!(catch_body.contains("throw "), "{mode}:\n{rethrow}");
        assert!(rethrow.contains("finally {"), "{mode}:\n{rethrow}");
    }
}

#[test]
fn awaiting_loop_retains_loop_and_finalizer() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source(&bytes);
        let body = declaration(&source, "async function awaitLoop(");
        assert!(
            body.contains("while (") || body.contains("for ("),
            "{mode}:\n{body}"
        );
        assert!(body.contains("finally {"), "{mode}:\n{body}");
        assert!(body.contains("await "), "{mode}:\n{body}");
    }
}

#[test]
fn duplicate_members_belong_to_their_own_classes() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source(&bytes);
        let base = declaration(&source, "class RecoveryBase");
        let child = declaration(&source, "class RecoveryChild extends RecoveryBase");
        let sibling = declaration(&source, "class RecoverySibling");
        assert!(base.contains("get value("), "{mode}:\n{base}");
        assert!(base.contains("set value("), "{mode}:\n{base}");
        assert!(base.contains("static identity("), "{mode}:\n{base}");
        assert!(child.contains("super("), "{mode}:\n{child}");
        assert!(child.contains("super.describe"), "{mode}:\n{child}");
        assert!(sibling.contains("static identity("), "{mode}:\n{sibling}");
        assert!(sibling.contains("sibling"), "{mode}:\n{sibling}");
        assert_eq!(source.matches("function describe(").count(), 0, "{mode}");
    }
}

#[test]
fn debug_parameters_and_owned_capture_names_survive_compilation() {
    let Some(bytes) = common::read_fixture(FIXTURES[0].1) else {
        return;
    };
    let source = source(&bytes);
    assert!(source.contains("async function awaitSuccess(value, trace)"));
    assert!(source.contains("async function awaitLoop(count, trace)"));
    assert!(source.contains("function scopedNames(input)"));
    let closure = declaration(&source, "function closureNames(initial)");
    assert!(closure.contains("let current = initial;"), "{closure}");
    assert!(!closure.contains("let current;"), "{closure}");
    assert!(closure.contains("return current;"), "{closure}");
}

#[test]
fn returned_closures_and_module_exports_remain_executable_expressions() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source(&bytes);
        let closure = declaration(&source, "function closureNames(");
        assert!(closure.contains("return () => {"), "{mode}:\n{closure}");
        assert!(!closure.contains("return /*closure@"), "{mode}:\n{closure}");
        assert!(!source.contains("export:"), "{mode}: invalid export label");
        let base = source.find("class RecoveryBase").unwrap();
        let child = source.find("class RecoveryChild").unwrap();
        assert!(
            base < child,
            "{mode}: superclass must exist before derived class"
        );
    }
}

#[test]
fn arkui_component_keeps_all_members_and_valid_constructor_order() {
    let fixtures = [
        ("debug", "advanced_recovery/arkui-debug.abc"),
        ("release", "advanced_recovery/arkui-release.abc"),
    ];
    for (mode, path) in fixtures {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source_record(&bytes, "ArkUIProbe");
        let page = declaration(&source, "class ProbePage extends globalThis.ViewPU");
        for member in [
            "constructor(",
            "setInitiallyProvidedValue(",
            "updateStateVars(",
            "purgeVariableDependenciesOnElmtId(",
            "aboutToBeDeleted(",
            "get counter(",
            "set counter(",
            "get title(",
            "set title(",
            "initialRender(",
            "rerender(",
            "static getEntryName(",
        ] {
            assert!(
                page.contains(&format!("\n    {member}")),
                "{mode}: missing {member}"
            );
        }
        let constructor = declaration(page, "constructor(");
        assert!(
            page.contains("types inferred from ArkUI ViewPU"),
            "{mode}: {constructor}"
        );
        for ty in [
            ": IView | undefined",
            "params: unknown",
            ": LocalStorage | undefined",
            "elmtId: number | undefined",
            "paramsLambda: (() => Object) | undefined",
            "extraInfo: ExtraInfo | undefined",
        ] {
            assert!(
                constructor.contains(ty),
                "{mode}: missing {ty}:\n{constructor}"
            );
        }
        // 普通类即使源码含 number，也不能从擦除后的 any 伪造原始类型。
        let circle = declaration(&source, "class Circle");
        assert!(!circle.contains("inferred from ArkUI"), "{mode}");
        assert!(
            !declaration(circle, "constructor(").contains(": number"),
            "{mode}"
        );
        let super_call = constructor.find("super(").expect("derived super call");
        assert!(
            !constructor[..super_call].contains("this"),
            "{mode}: this before super:\n{constructor}"
        );
        let render = declaration(page, "initialRender(");
        assert!(render.contains("observeComponentCreation2("), "{mode}");
        assert!(render.contains("forEachUpdateFunction("), "{mode}");
        assert!(!render.contains("let this;"), "{mode}");
        assert!(!source.contains("function ProbePage("), "{mode}");
        assert!(!source.contains("function initialRender("), "{mode}");
        assert!(
            source.find("class Circle").unwrap() < source.find("class ProbePage").unwrap(),
            "{mode}"
        );
    }
}

#[test]
fn compiler_property_definitions_recover_fields_and_preserve_dynamic_descriptors() {
    for path in [
        "advanced_recovery/properties-debug.abc",
        "advanced_recovery/properties-release.abc",
    ] {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let file = AbcFile::parse(bytes.to_vec()).unwrap();
        let definitions = file
            .classes
            .iter()
            .flat_map(|class| &class.methods)
            .filter_map(|method| method.code.as_ref())
            .flat_map(|code| disasm::decode(code.instructions(&file.data)).unwrap())
            .filter(|instruction| {
                matches!(
                    instruction.meta.mnemonic,
                    "definepropertybyname" | "definefieldbyname"
                )
            })
            .count();
        assert_eq!(
            definitions, 5,
            "fixture must exercise actual own-property definition instructions"
        );
        let source = source_record(&bytes, "PropertyDefinitions");
        assert!(source.contains("static answer = 42;"), "{source}");
        assert!(source.contains("static label = \"ready\";"), "{source}");
        assert!(source.contains("static enabled = true;"), "{source}");
        assert!(
            source.contains(
                "{ value: [1, 2, 3], writable: true, enumerable: true, configurable: true }"
            ),
            "{source}"
        );
        assert!(!source.contains("/* definepropertybyname"), "{source}");
    }
}

#[test]
fn imported_classes_keep_inheritance_members_and_spread_constructors() {
    for path in [
        "advanced_recovery/inheritance-debug.abc",
        "advanced_recovery/inheritance-release.abc",
    ] {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let source = source_record(&bytes, "ImportedInheritance");
        for (name, base) in [
            ("ImportedSheet", "ImportedBase"),
            ("NamespaceSheet", "BaseModule.Base"),
            ("SpreadSheet", "ImportedBase"),
            ("ModifiedSheet", "ImportedBase"),
        ] {
            let class = declaration(&source, &format!("class {name} extends {base}"));
            assert!(class.contains("constructor("), "{source}");
            assert!(class.contains("super("), "{source}");
            assert!(class.contains("isSheetShow = false;"), "{source}");
            assert!(!source.contains(&format!("function {name}(")), "{source}");
        }
        let class = declaration(&source, "class ImportedSheet");
        assert!(class.contains("get visible()"), "{source}");
        assert!(class.contains("set visible("), "{source}");
        assert!(class.contains("getIsShow()"), "{source}");
        assert!(!source.contains("import:"), "{source}");
        assert!(!source.contains("supercallspread"), "{source}");
        let spread = declaration(&source, "class SpreadSheet");
        assert!(!spread.contains(".push("), "{source}");
    }
}
