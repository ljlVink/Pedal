//! HiNote 注释所暴露指令的 SDK 样例回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile};

mod common;

#[test]
fn sdk_private_fields_nested_classes_and_async_closures_recover() {
    for path in ["hinote_recovery/debug.abc", "hinote_recovery/release.abc"] {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
        let file = AbcFile::parse(bytes).unwrap();
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains("HiNoteRecovery"))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for expected in [
            "class Counter",
            "class Box",
            "get value()",
            "set value(",
            "globalThis.WeakMap",
            "async function asyncClosure",
            "class Child extends Base",
            "super.value",
            "Reflect.construct",
        ] {
            assert!(source.contains(expected), "missing {expected}: {source}");
        }
        for missing in [
            "Conservative fallback",
            "/* ldprivateproperty",
            "/* stprivateproperty",
            "/* callruntime.callinit",
            "/* getnextpropname",
            "Unresolved closure",
            "/* ldsuperbyname",
            "/* ldsuperbyvalue",
            "/* stsuperbyname",
        ] {
            assert!(!source.contains(missing), "{missing}: {source}");
        }
        // 两个私有槽都必须有局部声明，不能依赖宽松模式下隐式创建的全局变量。
        assert_eq!(
            source
                .lines()
                .filter(|line| line.starts_with("let private_"))
                .count(),
            2,
            "{source}"
        );
        let method = record
            .methods
            .iter()
            .find(|method| method.name == "enumerableKeys")
            .unwrap();
        let code = method.code.as_ref().unwrap();
        let ins = pedal_abc_bridge::disasm::decode(code.instructions(file.bytes())).unwrap();
        let graph = decompile::cfg::build(&ins, code.code_size as usize, &code.try_blocks);
        let body = decompile::lift::lift(&file, record, method, code, &graph, None);
        let loop_body = body
            .stmts
            .iter()
            .find_map(|stmt| {
                if let decompile::lift::Stmt::While(_, body) = stmt {
                    Some(body)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            format!("{loop_body:?}").contains("\"push\""),
            "loop call escaped its iteration: {source}"
        );
    }
}
