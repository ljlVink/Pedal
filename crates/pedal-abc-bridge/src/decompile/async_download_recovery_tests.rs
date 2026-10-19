//! 优化器异步寄存器快照与未知协议的保守匹配回归。
//! @author ljlVink

use super::*;
use crate::file::{CatchBlock, TryBlock};

fn region(begin: usize, end: usize, handler: usize, length: usize) -> TryBlock {
    TryBlock {
        start_pc: begin as u32,
        length: (end - begin) as u32,
        catch_blocks: vec![CatchBlock {
            type_idx: 0,
            handler_pc: handler as u32,
            code_size: length as u32,
        }],
    }
}

fn normalize(bytes: &[u8], regions: &[TryBlock]) -> Option<AsyncNormalization> {
    let instructions = crate::disasm::decode(bytes).expect("valid fixture");
    let graph = crate::decompile::cfg::build(&instructions, bytes.len(), regions);
    normalize_async(&instructions, regions, &graph)
}

#[test]
fn defining_a_closure_or_loading_a_wide_module_does_not_consume_the_async_machine() {
    for operation in [
        vec![0x33, 0, 0, 0, 0],
        vec![0xfd, 0x11, 2, 1],
        vec![0x09, 1, 0x00],
    ] {
        let mut bytes = vec![0xae, 0x61, 0];
        bytes.extend(operation);
        bytes.extend([0xcd, 0, 0x64]);
        let handler = bytes.len();
        bytes.extend([0x61, 1, 0x60, 1, 0xce, 0, 0x64]);
        let regions = [region(3, handler, handler, bytes.len() - handler)];
        assert!(
            normalize(&bytes, &regions).is_some(),
            "ISA in/out flags must not invent a machine read: {bytes:?}"
        );
    }
}

/// 与 download-install 优化方法相同：协议指令之间穿插对象别名和普通值快照。
fn await_fixture(
    snapshot: bool,
    constant: i32,
    clobber: bool,
    leak_mode: bool,
) -> (Vec<u8>, Vec<TryBlock>) {
    let mut bytes = vec![0xae, 0x61, 0]; // asyncfunctionenter; sta v0
    let begin = bytes.len();
    bytes.extend([0x44, 0x01]); // mov v1,v0
    bytes.push(0x62);
    bytes.extend(constant.to_le_bytes());
    bytes.extend([0x61, 4, 0x60, 8]); // THROW constant; await argument
    let mut emit = |op: &[u8]| {
        bytes.extend(op);
        if snapshot {
            bytes.extend([0x44, 0x01, 0x44, 0x76]); // machine alias + ordinary v6=v7
        }
    };
    emit(&[0xc4, 0]);
    emit(&[0xc3, 0]);
    emit(&[0x60, 0]);
    emit(&[0xbf]);
    emit(&[0x61, 2]);
    if clobber {
        emit(&[0x44, 0x72]);
    } // overwrite resumed value
    emit(&[0x60, 0]);
    emit(&[0xc0]);
    emit(&[0x61, 3]);
    emit(&[0x60, 4]);
    emit(&[0x0f, 0, 3]);
    let branch = bytes.len();
    bytes.extend([0x4f, 0]); // jeqz; patched to continuation
    if snapshot {
        bytes.extend([0x44, 0x01, 0x44, 0x76]);
    }
    bytes.extend([0x60, 2, 0xfe, 0]); // throw resumed rejection
    let continuation = bytes.len();
    bytes[branch + 1] = (continuation - branch) as u8;
    if snapshot {
        bytes.extend([0x44, 0x01, 0x44, 0x76]);
    }
    bytes.extend([0x60, if leak_mode { 3 } else { 2 }, 0xcd, 0, 0x64]);
    let handler = bytes.len();
    bytes.extend([0x61, 5, 0x60, 5, 0xce, 1, 0x64]);
    let regions = vec![region(begin, handler, handler, bytes.len() - handler)];
    (bytes, regions)
}

#[test]
fn interleaved_snapshots_normalize_and_preserve_ordinary_assignments() {
    for snapshot in [false, true] {
        let (bytes, regions) = await_fixture(snapshot, 1, false, false);
        let normalization = normalize(&bytes, &regions).expect("verified await protocol");
        let instructions = crate::disasm::decode(&bytes).unwrap();
        assert!(normalization.regions.is_empty());
        for (index, instruction) in instructions.iter().enumerate() {
            if instruction.meta.mnemonic == "mov" {
                let operands = instruction.sig_values();
                assert_eq!(
                    normalization.erase.contains(&index),
                    operands[0].2 == 1,
                    "only machine-object snapshots can be erased"
                );
            }
        }
    }
}

#[test]
fn wrong_resume_mode_result_clobber_and_escaping_mode_remain_raw() {
    for (constant, clobber, leak) in [(2, false, false), (1, true, false), (1, false, true)] {
        let (bytes, regions) = await_fixture(true, constant, clobber, leak);
        assert!(
            normalize(&bytes, &regions).is_none(),
            "constant={constant} clobber={clobber} mode_escape={leak}"
        );
    }
}

#[test]
fn ordinary_catch_payload_can_reuse_an_old_generator_alias() {
    let bytes = [
        0xae, 0x61, 0, // generator v0
        0x44, 0x01, // generator snapshot v1
        0x62, 7, 0, 0, 0, 0xfe, 0, // throw 7
        0x61, 1, 0x60, 1, 0xfe, 0, // catch v1; throw v1
        0x61, 5, 0x60, 5, 0xce, 0, 0x64, // outer reject v0
    ];
    let regions = [region(3, 12, 12, 6), region(3, 18, 18, 7)];
    let normalization = normalize(&bytes, &regions).expect("alias lifetime ends at catch write");
    assert_eq!(normalization.regions.len(), 1);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let catch_load = instructions.iter().position(|i| i.pc == 14).unwrap();
    assert!(!normalization.erase.contains(&catch_load));
}

#[test]
fn machine_object_leak_to_user_return_remains_raw() {
    let (mut bytes, regions) = await_fixture(false, 1, false, false);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let resolve = instructions
        .iter()
        .find(|i| i.meta.mnemonic == "asyncfunctionresolve")
        .unwrap()
        .pc;
    bytes[resolve - 1] = 1; // lda machine alias, instead of resumed value
    assert!(normalize(&bytes, &regions).is_none());
}

#[test]
fn optimized_resume_edge_is_preserved_as_an_unconditional_jump() {
    let (mut bytes, mut regions) = await_fixture(false, 1, false, false);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let branch = instructions
        .iter()
        .find(|i| i.meta.mnemonic == "jeqz")
        .unwrap()
        .pc;
    let original_target = instructions
        .iter()
        .find(|i| i.pc == branch)
        .unwrap()
        .jump_target()
        .unwrap();
    let skipped = [0x62, 99, 0, 0, 0, 0x61, 6];
    bytes.splice(original_target..original_target, skipped);
    bytes[branch + 1] += skipped.len() as u8;
    regions[0].length += skipped.len() as u32;
    regions[0].catch_blocks[0].handler_pc += skipped.len() as u32;
    let normalization = normalize(&bytes, &regions).expect("threaded normal resume edge");
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let branch_index = instructions.iter().position(|i| i.pc == branch).unwrap();
    assert_eq!(
        normalization.jumps,
        std::collections::HashSet::from([branch_index])
    );
}

#[test]
fn resume_constant_must_agree_on_every_reaching_branch() {
    let (mut bytes, mut regions) = await_fixture(false, 1, false, false);
    // One branch preserves v4=1, the other writes v4=2 before the same await.
    let insertion = 12;
    let branch = [0x60, 9, 0x4f, 9, 0x62, 2, 0, 0, 0, 0x61, 4];
    bytes.splice(insertion..insertion, branch);
    regions[0].length += branch.len() as u32;
    regions[0].catch_blocks[0].handler_pc += branch.len() as u32;
    assert!(
        normalize(&bytes, &regions).is_none(),
        "ambiguous THROW constant cannot be erased"
    );
}

#[test]
fn ambiguous_machine_alias_at_exception_join_remains_raw() {
    let (mut bytes, mut regions) = await_fixture(false, 1, false, false);
    // Before await, one normal branch keeps v1's machine object and another overwrites it.
    let branch = [0x60, 9, 0x4f, 4, 0x44, 0x91];
    bytes.splice(12..12, branch);
    regions[0].length += branch.len() as u32;
    regions[0].catch_blocks[0].handler_pc += branch.len() as u32;
    assert!(
        normalize(&bytes, &regions).is_none(),
        "outer reject must receive the machine on every path"
    );
}

#[test]
fn machine_alias_in_implicit_call_range_arguments_remains_raw() {
    let (mut bytes, mut regions) = await_fixture(false, 1, false, false);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let await_pc = instructions
        .iter()
        .find(|i| i.meta.mnemonic == "asyncfunctionawaituncaught")
        .unwrap()
        .pc;
    // The encoded range starts at v4; v6 is an implicit argument containing the machine.
    let leak = [0x44, 0x06, 0x73, 0, 3, 4];
    bytes.splice(await_pc..await_pc, leak);
    regions[0].length += leak.len() as u32;
    regions[0].catch_blocks[0].handler_pc += leak.len() as u32;
    assert!(
        normalize(&bytes, &regions).is_none(),
        "machine must not escape through a range argument"
    );
}

fn concurrent_async_fixture(resolve_notify: bool, reject_notify: bool) -> (Vec<u8>, Vec<TryBlock>) {
    let (mut bytes, mut regions) = await_fixture(false, 1, false, false);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let returns: Vec<_> = instructions
        .iter()
        .filter(|i| i.meta.mnemonic == "return")
        .map(|i| i.pc)
        .collect();
    let tail = [0x61, 6, 0xfb, 0, 0x60, 6];
    if reject_notify {
        bytes.splice(returns[1]..returns[1], tail);
        regions[0].catch_blocks[0].code_size += tail.len() as u32;
    }
    if resolve_notify {
        bytes.splice(returns[0]..returns[0], tail);
        regions[0].length += tail.len() as u32;
        regions[0].catch_blocks[0].handler_pc += tail.len() as u32;
    }
    (bytes, regions)
}

#[test]
fn concurrent_async_requires_complete_matched_notifications_on_both_completions() {
    let (bytes, regions) = concurrent_async_fixture(true, true);
    assert!(
        normalize(&bytes, &regions)
            .expect("complete concurrent protocol")
            .is_concurrent
    );
    for (resolve, reject) in [(true, false), (false, true)] {
        let (bytes, regions) = concurrent_async_fixture(resolve, reject);
        assert!(
            normalize(&bytes, &regions).is_none(),
            "partial taskpool completion cannot be erased"
        );
    }
    let (mut bytes, regions) = concurrent_async_fixture(true, true);
    let instructions = crate::disasm::decode(&bytes).unwrap();
    let notify = instructions
        .iter()
        .find(|i| i.meta.mnemonic == "callruntime.notifyconcurrentresult")
        .unwrap()
        .pc;
    bytes[notify + 3] = 7; // reload a different value from the completion spill
    assert!(normalize(&bytes, &regions).is_none());
}

#[test]
fn synchronous_notifications_must_be_terminal_and_cover_every_return() {
    let valid = [0x60, 4, 0x61, 0, 0xfb, 0, 0x60, 0, 0x64];
    assert!(normalize_concurrent_sync(&crate::disasm::decode(&valid).unwrap()).is_some());
    let implicit = [0x00, 0xfb, 0, 0x65]; // undefined; notify; returnundefined
    assert!(normalize_concurrent_sync(&crate::disasm::decode(&implicit).unwrap()).is_some());
    for invalid in [
        vec![0x60, 4, 0xfb, 0, 0x62, 1, 0, 0, 0, 0x64], // mid-body notification
        vec![0x60, 4, 0x61, 0, 0xfb, 0, 0x60, 1, 0x64], // different return value
        vec![0x60, 4, 0x61, 0, 0xfb, 0, 0x60, 0, 0x64, 0x65], // uncovered return
        vec![0x60, 4, 0x4f, 8, 0x61, 0, 0xfb, 0, 0x60, 0, 0x64], // branch bypasses notify
    ] {
        assert!(normalize_concurrent_sync(&crate::disasm::decode(&invalid).unwrap()).is_none());
    }
}

#[test]
fn sdk_concurrent_fixtures_restore_directive_before_locals_and_all_notifications() {
    for path in [
        "advanced_recovery/concurrent-debug.abc",
        "advanced_recovery/concurrent-release.abc",
    ] {
        let Some(bytes) = crate::test_support::read_fixture(path) else {
            continue;
        };
        let file = crate::file::AbcFile::parse(bytes).unwrap();
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains("ConcurrentRecovery"))
            .unwrap();
        let source = crate::decompile::decompile_record(&file, record)
            .unwrap()
            .source;
        assert_eq!(source.matches("\"use concurrent\";").count(), 6, "{source}");
        for name in [
            "concurrentFulfilled",
            "concurrentRejected",
            "concurrentValue",
            "concurrentImplicit",
            "concurrentChoose",
            "concurrentBranch",
        ] {
            let start = source
                .find(&format!("function {name}("))
                .expect("recovered function");
            let opening = start + source[start..].find('{').unwrap();
            assert!(
                source[opening + 1..]
                    .trim_start()
                    .starts_with("\"use concurrent\";"),
                "directive must precede local declarations: {name}\n{source}"
            );
        }
        assert!(!source.contains("Conservative fallback"), "{source}");
        assert!(!source.contains("notifyconcurrentresult"), "{source}");
        assert!(
            source.contains("async function concurrentFulfilled"),
            "{source}"
        );
        assert!(
            source.contains("async function concurrentRejected"),
            "{source}"
        );
    }
}

#[test]
fn concurrent_function_metadata_does_not_hide_verified_async_modifier() {
    let Some(bytes) = crate::test_support::read_fixture("advanced_recovery/concurrent-release.abc")
    else {
        return;
    };
    let mut file = crate::file::AbcFile::parse(bytes).unwrap();
    for method in file
        .classes
        .iter_mut()
        .flat_map(|record| &mut record.methods)
    {
        if method.name.starts_with("concurrent") {
            method.kind = crate::file::FunctionKind::ConcurrentFunction;
        }
    }
    let record = file
        .classes
        .iter()
        .find(|record| record.name.contains("ConcurrentRecovery"))
        .unwrap();
    let source = crate::decompile::decompile_record(&file, record)
        .unwrap()
        .source;
    assert!(
        source.contains("async function concurrentFulfilled"),
        "{source}"
    );
    assert!(
        source.contains("async function concurrentRejected"),
        "{source}"
    );
    assert_eq!(source.matches("\"use concurrent\";").count(), 6, "{source}");
}
