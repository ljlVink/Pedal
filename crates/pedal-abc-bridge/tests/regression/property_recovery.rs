//! SDK 计算属性、对象展开、深层对象方法与异步 rest 参数回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile};

use super::common;

#[test]
fn compiler_properties_and_async_rest_keep_their_bodies() {
    for (mode, path) in [
        ("debug", "property_recovery/debug.abc"),
        ("release", "property_recovery/release.abc"),
    ] {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains("PropertyRecovery"))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for name in ["asyncRest", "asyncRestNoAwait", "asyncRestRejected"] {
            assert!(
                source.contains(&format!("async function {name}(")),
                "{mode}: {source}"
            );
        }
        for operation in [
            "Reflect.ownKeys",
            "Object.getOwnPropertyDescriptor",
            "Reflect.defineProperty",
            "\"name\"",
            "leaf()",
            " + 3;",
        ] {
            assert!(
                source.contains(operation),
                "{mode}: missing {operation}: {source}"
            );
        }
        assert_eq!(
            source.matches(", 4, { value:").count(),
            1,
            "{mode}: {source}"
        );
        assert_eq!(
            source.matches(", 9, { value:").count(),
            1,
            "{mode}: {source}"
        );
        for placeholder in [
            "Conservative fallback",
            "method@",
            "Unresolved closure",
            "definefieldby",
            "stownbyvaluewithnameset",
            "copydataproperties",
            "target[",
        ] {
            assert!(
                !source.contains(placeholder),
                "{mode}: {placeholder}: {source}"
            );
        }
    }
}
