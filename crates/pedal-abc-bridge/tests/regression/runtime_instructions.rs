//! 运行时与模块指令的表达式提升回归。
//! @author ljlVink

use pedal_abc_bridge::file::{
    AbcFile, ClassInfo, CodeInfo, FileHeader, FunctionKind, HeaderKind, MethodInfo, ModuleRecord,
    RegularImport,
};
use pedal_abc_bridge::{decompile, disasm};

fn lift(code: &[u8], module: Option<&ModuleRecord>) -> String {
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
        name: "Probe".into(),
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
        name: "probe".into(),
        header_index: 0,
        kind: FunctionKind::Function,
        code: Some(CodeInfo {
            offset: 0,
            num_vregs: 1,
            num_args: 0,
            code_size: code.len() as u32,
            ins_off: 0,
            try_blocks: Vec::new(),
        }),
        debug_info_off: 0,
        foreign: false,
    };
    let code_info = method.code.as_ref().unwrap();
    let instructions = disasm::decode(code_info.instructions(&file.data)).unwrap();
    let cfg = decompile::cfg::build(&instructions, code_info.code_size as usize, &[]);
    format!(
        "{:?}",
        decompile::lift::lift(&file, &class, &method, code_info, &cfg, module).stmts
    )
}

#[test]
fn lazy_module_load_uses_the_import_slot_and_sets_accumulator() {
    let module = ModuleRecord {
        requests: vec![
            "@normalized:other/module".into(),
            "@normalized:some/module".into(),
        ],
        regular_imports: vec![RegularImport {
            local_name: "ImportedThing".into(),
            import_name: "ImportedThing".into(),
            // This is the request index; the instruction immediate is the import slot (0).
            module_idx: 1,
        }],
        ..ModuleRecord::default()
    };
    // callruntime.ldlazymodulevar slot 0; return acc.
    let source = lift(&[0xfb, 0x15, 0x00, 0x64], Some(&module));
    assert!(
        source.contains("Return(Some(Name(\"ImportedThing\")))"),
        "{source}"
    );
    assert!(!source.contains("Comment"), "{source}");

    // The widened opcode keeps the same slot mapping.
    let source = lift(&[0xfb, 0x16, 0x00, 0x00, 0x64], Some(&module));
    assert!(
        source.contains("Return(Some(Name(\"ImportedThing\")))"),
        "{source}"
    );
}

#[test]
fn dynamic_import_wraps_the_accumulator_specifier_in_import_call() {
    // ldundefined; dynamicimport; return acc.
    let source = lift(&[0x00, 0xbd, 0x64], None);
    assert!(
        source.contains("Call { callee: Name(\"import\"), args: [Special(\"undefined\")]"),
        "{source}"
    );
    assert!(!source.contains("Comment(\"dynamicimport"), "{source}");
}

#[test]
fn delete_object_property_uses_accumulator_key_and_register_object() {
    // createemptyobject; sta v0; ldai 0; delobjprop v0; return acc.
    let source = lift(&[0x05, 0x61, 0, 0x62, 0, 0, 0, 0, 0xc2, 0, 0x64], None);
    assert!(
        source.contains("Un(\"delete\", Index(Reg(0), Int(0)))"),
        "{source}"
    );
    assert!(!source.contains("Comment(\"delobjprop"), "{source}");
}

#[test]
fn iterator_result_check_retains_type_error_and_accepts_functions() {
    // lda v0; throw.ifnotobject v0; return，检查不消费累加器。
    let source = lift(&[0x60, 0, 0xfe, 5, 0, 0x64], None);
    for token in [
        "TypeError",
        "typeof",
        "object",
        "function",
        "Special(\"null\")",
        "Return(Some(Reg(0)))",
    ] {
        assert!(source.contains(token), "missing {token}: {source}");
    }
    assert!(!source.contains("Comment"), "{source}");
}

#[test]
fn panda_getiterator_preserves_noncallable_method_and_evaluates_it_once() {
    let source = lift(&[0x60, 0, 0x67, 0, 0x64], None);
    assert_eq!(source.matches("\"iterator\"").count(), 1, "{source}");
    assert!(source.contains("Cond("), "{source}");
    assert!(source.contains("Reflect"), "{source}");
    assert!(
        !source.contains("TypeError"),
        "Panda opcode returns non-callable values unchanged: {source}"
    );
}

#[test]
fn wide_rest_copy_uses_the_full_start_index() {
    // wide.copyrestargs 258; return acc。
    let source = lift(&[0xfd, 0x0b, 2, 1, 0x64], None);
    assert!(source.contains("Int(258)"), "{source}");
    assert!(source.contains("Array.prototype.slice.call"), "{source}");
    assert!(!source.contains("Comment"), "{source}");
}

#[test]
fn wide_module_store_and_load_keep_the_full_export_index() {
    let module = ModuleRecord {
        local_exports: (0..259)
            .map(|index| pedal_abc_bridge::file::LocalExport {
                local_name: format!("local{index}"),
                export_name: format!("public{index}"),
            })
            .collect(),
        ..ModuleRecord::default()
    };
    // 缓存/索引不能截成八位；258 必须与导出槽 2 区分。
    let source = lift(
        &[0x62, 7, 0, 0, 0, 0xfd, 0x0f, 2, 1, 0xfd, 0x10, 2, 1, 0x64],
        Some(&module),
    );
    assert!(
        source.contains("Name(\"export:public258\"), Int(7)"),
        "{source}"
    );
    assert!(
        source.contains("Return(Some(Name(\"local258\")))"),
        "{source}"
    );
    assert!(!source.contains("Comment"), "{source}");
}

#[test]
fn sendable_module_loads_use_import_slots_in_both_widths() {
    let module = ModuleRecord {
        requests: vec!["unused".into(), "actual/module".into()],
        regular_imports: (0..259)
            .map(|index| RegularImport {
                local_name: format!("Imported{index}"),
                import_name: format!("Imported{index}"),
                module_idx: 1,
            })
            .collect(),
        ..ModuleRecord::default()
    };
    for (bytes, name) in [
        (vec![0xfb, 9, 2, 0x64], "Imported2"),
        (vec![0xfb, 10, 2, 1, 0x64], "Imported258"),
    ] {
        let source = lift(&bytes, Some(&module));
        assert!(
            source.contains(&format!("Return(Some(Name({name:?})))")),
            "{source}"
        );
        assert!(!source.contains("Comment"), "{source}");
    }
}

#[test]
fn unknown_private_layout_keeps_its_diagnostic() {
    let source = lift(&[0x60, 0, 0xd8, 7, 2, 0, 3, 0, 0x64], None);
    assert!(source.contains("Comment"), "{source}");
    assert!(!source.contains("WeakMap"), "{source}");
}
