//! 合成 Ark 字节码的反编译可读性回归测试。
//!
//! Fixture 直接构造最小现代 AbcFile，不依赖本地语料或生成的 HAP。
//!
//! @author ljlVink

use pedal_abc_bridge::file::{
    AbcFile, ClassInfo, CodeInfo, FileHeader, FunctionKind, HeaderKind, MethodInfo,
};
use pedal_abc_bridge::{decompile, disasm};

fn file_with_code(
    name: &str,
    code: &[u8],
    num_args: u32,
    num_vregs: u32,
) -> (AbcFile, ClassInfo, MethodInfo) {
    let file = AbcFile {
        data: code.to_vec(),
        header: FileHeader {
            version: [12, 0, 0, 0],
            file_size: code.len() as u32,
            foreign_off: 0,
            foreign_size: 0,
            num_classes: 0,
            class_idx_off: 0,
            num_lnps: 0,
            lnp_idx_off: 0,
            num_literalarrays: 0,
            literalarray_idx_off: 0,
            num_index_regions: 0,
            index_section_off: 0,
            kind: HeaderKind::Modern,
        },
        class_index: Vec::new(),
        index_headers: Vec::new(),
        classes: Vec::new(),
    };
    let class = ClassInfo {
        offset: 0,
        name: "Probe".to_string(),
        access_flags: 0,
        source_file: None,
        fields: Vec::new(),
        methods: Vec::new(),
        module_record_idx: None,
    };
    let method = MethodInfo {
        offset: 0,
        class_off: 0,
        proto_off: 0,
        name: name.to_string(),
        header_index: 0,
        kind: FunctionKind::Function,
        code: Some(CodeInfo {
            offset: 0,
            num_vregs,
            num_args,
            code_size: code.len() as u32,
            ins_off: 0,
            try_blocks: Vec::new(),
        }),
        debug_info_off: 0,
        foreign: false,
    };
    (file, class, method)
}

fn lift(name: &str, code: &[u8], num_args: u32, num_vregs: u32) -> String {
    let (file, class, method) = file_with_code(name, code, num_args, num_vregs);
    let code = method.code.as_ref().expect("fixture has code");
    let ins = disasm::decode(code.instructions(&file.data)).expect("fixture instructions decode");
    let cfg = decompile::cfg::build(&ins, code.code_size as usize, &code.try_blocks);
    format!(
        "{:?}",
        decompile::lift::lift(&file, &class, &method, code, &cfg, None).stmts
    )
}

#[test]
fn arguments_and_spread_super_use_actual_call_arguments_and_explicit_operand() {
    let src = lift(
        "constructor",
        &[
            0x6c, // getunmappedargs
            0x61, 1, // sta v1
            0x60, 2, // lda currentFunction (num_vregs = 2)
            0xb9, 7, 1, // supercallspread cache_slot=7, v1
            0x64,
        ],
        3,
        2,
    );
    assert!(src.contains("Special(\"arguments\")"), "{src}");
    assert!(
        src.contains("callee: Special(\"super\"), args: [Spread(Reg(1))]"),
        "{src}"
    );
    assert!(!src.contains("Comment("), "{src}");
}

#[test]
fn computed_super_load_recovers_only_a_proven_this_receiver() {
    // lda this; sta v0; ldai 7; ldsuperbyvalue cache=55,v0; return。
    let code = [0x60, 5, 0x61, 0, 0x62, 7, 0, 0, 0, 0x39, 55, 0, 0x64];
    let source = lift("method", &code, 3, 3);
    assert!(
        source.contains("Index(Special(\"super\"), Int(7))"),
        "{source}"
    );
    // 自定义接收者不能改写为原生 super[key]，否则 getter 得到错误的 this。
    let source = lift("method", &code, 4, 2);
    assert!(source.contains("Comment"), "{source}");
}

#[test]
fn array_spread_uses_start_index_and_defines_own_properties() {
    // dst=a0, start=a1, iterable=a2；返回新下标，不能假设从数组尾追加。
    let src = lift(
        "spreadInto",
        &[
            0x60, 5, // lda a2
            0xc6, 3, 4, // starrayspread a0, a1
            0x64,
        ],
        6,
        0,
    );
    assert!(src.contains("ForOf("), "{src}");
    assert!(src.contains("globalThis.Object.defineProperty"), "{src}");
    assert!(src.contains("Arg(3)"), "{src}");
    assert!(src.contains("Arg(4)"), "{src}");
    assert!(src.contains("Arg(5)"), "{src}");
    assert!(src.contains("Return(Some(Reg(8)))"), "{src}");
    assert!(src.contains("If(Reg(9)"), "{src}");
    assert!(src.contains("Member(Reg(7), \"length\")"), "{src}");
    assert!(!src.contains("\"push\""), "{src}");
}

#[test]
fn array_spread_does_not_delay_writes_to_argument_slots() {
    let src = lift(
        "observableDestination",
        &[
            0x05, 0, // createemptyarray
            0x61, 4, // a0=[] (num_vregs=1, implicit args=3)
            0x62, 0, 0, 0, 0, // ldai 0
            0x61, 0, // v0=0
            0x60, 5, // lda a1
            0xc6, 4, 0, // spread into a0 from index v0
            0x64,
        ],
        5,
        1,
    );
    assert!(src.contains("Assign(Arg(3), Special(\"[]\"))"), "{src}");
    assert!(src.contains("ForOf("), "{src}");
    assert!(!src.contains("Array([Spread"), "{src}");
}

#[test]
fn clamp_style_branches_keep_the_comparison_and_both_values() {
    // if (a0 < a1) return a1; return a0;
    // less reads v3 as the left operand and the accumulator as the right operand.
    let src = lift(
        "clampLow",
        &[
            0x60, 0, // lda v0 (first explicit argument)
            0x60, 1, // lda v1 (second explicit argument)
            0x11, 0, 0, // less v0, acc
            0x51, 5, // jnez -> return a1
            0x60, 0,    // lda v0
            0x64, // return
            0x60, 1,    // lda v1
            0x64, // return
        ],
        2,
        0,
    );
    assert!(
        src.contains("Bin(\"<\", Arg(0), Arg(1))"),
        "comparison lost or operands swapped: {src}"
    );
    assert!(
        src.contains("Return(Some(Arg(1)))"),
        "true branch value missing: {src}"
    );
    assert!(
        src.contains("Return(Some(Arg(0)))"),
        "fallthrough return missing: {src}"
    );
}

#[test]
fn clamp_style_branch_assignments_keep_each_value_and_result_register() {
    // if (a0 < a1) v2 = a1; else v2 = a0; return v2;
    let src = lift(
        "clampAssign",
        &[
            0x60, 3, // lda v3 (a0)
            0x60, 4, // lda v4 (a1)
            0x11, 0, 3, // less v3, acc
            0x51, 8, // jnez -> true assignment
            0x60, 3, // else: lda v3
            0x61, 2, // sta local v2
            0x4d, 6, // jmp to merge
            0x60, 4, // then: lda v4
            0x61, 2, // sta local v2
            0x60, 2,    // merge: lda v2
            0x64, // return
        ],
        2,
        3,
    );
    assert!(
        src.contains("Bin(\"<\", Arg(0), Arg(1))"),
        "comparison lost or operands swapped: {src}"
    );
    assert!(
        src.contains("Assign(Reg(2), Arg(0))"),
        "else assignment missing or wrong: {src}"
    );
    assert!(
        src.contains("Assign(Reg(2), Arg(1))"),
        "then assignment missing or wrong: {src}"
    );
    assert!(
        src.contains("Return(Some(Reg(2)))"),
        "merged result register was not returned: {src}"
    );
}

#[test]
fn newobjrange_uses_count_and_not_inline_cache_slot() {
    // v0 is constructor, v1 is its sole argument. imm1=0x1234 is an IC slot;
    // imm2=2 counts constructor + one argument.
    let src = lift(
        "construct",
        &[
            0x83, 0x34, 0x12, 2, 0,    // newobjrange 0x1234, 2, v0
            0x64, // return
        ],
        3,
        0,
    );
    assert!(
        src.contains("callee: Arg(0)"),
        "constructor register was not used as callee: {src}"
    );
    assert!(
        src.contains("args: [Arg(1)]"),
        "IC slot was mistaken for argument count: {src}"
    );
    assert!(
        src.contains("new: true"),
        "newobjrange did not create a constructor call: {src}"
    );
}

#[test]
fn definefunc_value_is_written_to_accumulator() {
    // definefunc -> acc, sta v3, lda v3, return. The unresolved method id is
    // intentionally harmless; the assertion checks that its value flows onward.
    let src = lift(
        "closureValue",
        &[
            0x33, 0, 0, 0, 0, // definefunc flags=0, method index=0, trailing flags=0
            0x61, 3, // sta v3
            0x60, 3,    // lda v3
            0x64, // return
        ],
        3,
        1,
    );
    assert!(
        src.contains("Closure { method_off: 0 }"),
        "definefunc result did not reach return: {src}"
    );
    assert!(
        src.contains("Return(Some(Arg(2)))"),
        "stored closure was not returned from its register: {src}"
    );
}

#[test]
fn callargs2_and_callargs3_use_the_explicit_register_arguments() {
    let two = lift(
        "callTwo",
        &[
            0x60, 0, // lda currentFunction
            0x2b, 0xe7, 1, 2, // callargs2 IC slot 0xe7, args v1 and v2
            0x64,
        ],
        3,
        0,
    );
    assert!(
        two.contains("callee: Arg(0), args: [Arg(1), Arg(2)]"),
        "callargs2 operands are wrong: {two}"
    );

    let three = lift(
        "callThree",
        &[
            0x60, 0, // lda currentFunction
            0x2c, 0xd4, 1, 2, 3, // callargs3 IC slot 0xd4, args v1, v2, v3
            0x64,
        ],
        4,
        0,
    );
    assert!(
        three.contains("callee: Arg(0), args: [Arg(1), Arg(2), Arg(3)]"),
        "callargs3 operands are wrong: {three}"
    );
}

#[test]
fn fldai_preserves_the_f64_literal() {
    let mut code = vec![0x63]; // fldai
    code.extend_from_slice(&3.5f64.to_le_bytes());
    code.push(0x64); // return
    let src = lift("floatLiteral", &code, 3, 0);
    assert!(
        src.contains("Return(Some(Float(3.5)))"),
        "f64 bits were not decoded as a float: {src}"
    );
}

#[test]
fn packed_lexvar_operands_keep_depth_and_slot_order() {
    let src = lift(
        "lexSlot",
        &[
            0x3c, 0xa3, // ldlexvar depth=3, slot=10
            0x64,
        ],
        3,
        0,
    );
    assert!(
        src.contains("LexVar(3, 10)"),
        "lexical depth/slot were lost or swapped: {src}"
    );
    assert!(
        !src.contains("LexVar(10, 3)"),
        "lexical depth and slot were swapped: {src}"
    );
}

#[test]
fn literal_count_does_not_consume_the_following_buffer() {
    use pedal_abc_bridge::file::LiteralValue;
    let (mut file, _, _) = file_with_code("literal", &[], 3, 0);
    // 三个 tag/value 对计数为六，后面的无效 tag 不属于此数组。
    file.data = vec![
        6, 0, 0, 0, 2, 1, 0, 0, 0, 2, 2, 0, 0, 0, 2, 3, 0, 0, 0, 0xe0,
    ];
    let values = file.read_literal_array(0).unwrap();
    assert!(matches!(
        values.as_slice(),
        [
            LiteralValue::Int32(1),
            LiteralValue::Int32(2),
            LiteralValue::Int32(3)
        ]
    ));
    let truncated = file.read_literal_array_truncated(0).unwrap();
    assert_eq!(truncated.total, 3);
    assert_eq!(truncated.values.len(), 3);
}

#[test]
fn literal_payload_widths_keep_the_next_value_aligned() {
    use pedal_abc_bridge::file::LiteralValue;
    let (mut file, _, _) = file_with_code("literal", &[], 3, 0);
    file.data = vec![
        8, 0, 0, 0, 8, 0, 0xff, 0, 0x17, 0x78, 0x56, 0x34, 0x12, 0x19, 9,
    ];
    let values = file.read_literal_array(0).unwrap();
    assert!(matches!(
        values.as_slice(),
        [
            LiteralValue::Accessor(0),
            LiteralValue::Null,
            LiteralValue::LiteralBufferIndex(0x12345678),
            LiteralValue::BuiltinTypeIndex(9)
        ]
    ));
}

#[test]
fn branch_condition_call_is_evaluated_once_even_when_accumulator_is_returned() {
    // 调用结果用于分支，两个分支仍返回同一个累加器值。
    let src = lift("once", &[0x60, 3, 0x29, 0, 0x4f, 4, 0x4d, 2, 0x64], 4, 0);
    assert_eq!(
        src.matches("Call {").count(),
        1,
        "condition was evaluated again: {src}"
    );
}

#[test]
fn oversized_module_initializer_preserves_disassembly() {
    let mut code = vec![0xd5; 2_000]; // nop
    code.push(0x65); // returnundefined
    assert_eq!(disasm::decode(&code).unwrap().len(), 2_001);
    let (file, mut class, method) = file_with_code("func_main_0", &code, 3, 0);
    class.methods.push(method);

    let source = decompile::decompile_record(&file, &class).unwrap().source;
    assert!(
        source.contains("func_main_0"),
        "initializer missing: {source}"
    );
    assert!(source.contains("2001 instructions"), "{source}");
    assert_eq!(source.matches("nop").count(), 2_000);
    assert!(source.contains("returnundefined"), "{source}");
}

#[test]
fn undecodable_module_initializer_preserves_diagnostic() {
    // lda 缺少寄存器操作数，应提示解码失败而不是返回空 record。
    let code = [0x60];
    assert!(disasm::decode(&code).is_err());
    let (file, mut class, method) = file_with_code("func_main_0", &code, 3, 0);
    class.methods.push(method);

    let source = decompile::decompile_record(&file, &class).unwrap().source;
    assert!(
        source.contains("func_main_0"),
        "initializer missing: {source}"
    );
    assert!(source.contains("bytecode decode failed"), "{source}");
    assert!(source.contains("0x60"), "failing opcode missing: {source}");
    assert!(
        source.contains("pc 0x0"),
        "failing offset missing: {source}"
    );
}

#[test]
fn defining_own_property_keeps_accumulator_and_evaluates_value_once() {
    use pedal_abc_bridge::file::IndexHeader;
    for opcode in [0xdb, 0xdc] {
        // a1(); define*byname cache_slot=250, key=0, receiver=a0; return acc
        let bytes = [0x60, 4, 0x29, 0, opcode, 250, 0, 0, 3, 0x64];
        let (mut file, mut class, method) = file_with_code("defineOwn", &bytes, 5, 0);
        file.data.resize(128, 0);
        file.data[96..100].copy_from_slice(&128u32.to_le_bytes());
        file.data.extend_from_slice(b"\x0danswer\0");
        file.index_headers.push(IndexHeader {
            start: 0,
            end: file.data.len() as u32,
            class_idx_size: 0,
            class_idx_off: 0,
            method_idx_size: 1,
            method_idx_off: 96,
            field_idx_size: 0,
            field_idx_off: 0,
            proto_idx_size: 0,
            proto_idx_off: 0,
        });
        class.methods.push(method);
        let source = decompile::decompile_record(&file, &class).unwrap().source;
        assert_eq!(source.matches("a1()").count(), 1, "{source}");
        assert!(source.contains("Object.defineProperty(a0, \"answer\", { value: v5, writable: true, enumerable: true, configurable: true })"), "{source}");
        assert!(source.contains("return v5;"), "{source}");
        assert!(
            !source.contains("a0.answer ="),
            "inherited setters must be bypassed: {source}"
        );
    }
}

#[test]
fn dynamic_and_indexed_fields_keep_receiver_key_and_accumulator() {
    for (opcode, key) in [
        (vec![0xfb, 1, 250, 4, 3], "a1"),
        (vec![0xfb, 2, 250, 0xcd, 0xab, 0x34, 0x12, 3], "305441741"),
    ] {
        // a2(); define field（缓存槽 250 与真实属性键不同）；return acc。
        let mut bytes = vec![0x60, 5, 0x29, 0];
        bytes.extend(opcode);
        bytes.push(0x64);
        let (file, mut class, method) = file_with_code("defineField", &bytes, 6, 0);
        class.methods.push(method);
        let source = decompile::decompile_record(&file, &class).unwrap().source;
        assert_eq!(source.matches("a2()").count(), 1, "{source}");
        assert!(
            source.contains(&format!("Object.defineProperty(a0, {key}, {{ value: v6,")),
            "{source}"
        );
        assert!(source.contains("return v6;"), "{source}");
        assert!(!source.contains("target["), "{source}");
    }
}

#[test]
fn object_copy_evaluates_source_once_and_returns_destination() {
    // a1(); copydataproperties a0; return acc。
    let bytes = [0x60, 4, 0x29, 0, 0xc5, 3, 0x64];
    let (file, mut class, method) = file_with_code("copy", &bytes, 5, 0);
    class.methods.push(method);
    let source = decompile::decompile_record(&file, &class).unwrap().source;
    assert_eq!(source.matches("a1()").count(), 1, "{source}");
    assert!(source.contains("return v6;"), "{source}");
    assert!(source.contains("let v6 = a0;"), "{source}");
    assert!(source.contains("Reflect.ownKeys"), "{source}");
    assert!(source.contains("Reflect.defineProperty(v6,"), "{source}");
    assert!(!source.contains("copydataproperties"), "{source}");
}
