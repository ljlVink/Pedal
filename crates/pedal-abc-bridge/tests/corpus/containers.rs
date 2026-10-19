//! 端到端集成测试：真实 abc / HAP 可选语料。
//!
//! 默认忽略；显式运行时要求所需语料存在，验证：
//! - 容器解析（12.x 与 0.1.0.7 两代布局）
//! - 指令解码覆盖率
//! - 反编译产物非空且包含函数或类声明
//!
//! @author ljlVink

use pedal_abc_bridge::decompile;
use pedal_abc_bridge::disasm;
use pedal_abc_bridge::file::{AbcFile, HeaderKind};
use pedal_abc_bridge::hap::HapArchive;
fn decode_coverage(file: &AbcFile) -> (usize, usize) {
    let legacy = file.header.kind == HeaderKind::Legacy;
    let mut total = 0;
    let mut ok = 0;
    for c in &file.classes {
        for m in &c.methods {
            let Some(code) = &m.code else { continue };
            total += 1;
            if disasm::decode_with(code.instructions(&file.data), legacy).is_ok() {
                ok += 1;
            }
        }
    }
    (ok, total)
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn modern_abc_from_hap_parses_and_decodes() {
    let hap_path = super::corpus_path("AppGallery/service.hap");
    let data = std::fs::read(&hap_path).unwrap();
    let hap = HapArchive::parse(data).unwrap();
    let abc_entry = hap.main_abc().expect("modules.abc present");
    let bytes = hap.read_entry(abc_entry).unwrap();
    let file = AbcFile::parse(bytes).unwrap();
    assert_eq!(file.header.kind, HeaderKind::Modern);
    assert!(!file.classes.is_empty(), "records must be enumerable");
    let (ok, total) = decode_coverage(&file);
    assert!(total > 100, "expected a real corpus file");
    assert_eq!(ok, total, "modern abc must decode 100% (got {ok}/{total})");
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn modern_abc_decompiles_records() {
    let hap_path = super::corpus_path("AppGallery/service.hap");
    let hap = HapArchive::parse(std::fs::read(&hap_path).unwrap()).unwrap();
    let bytes = hap.read_entry(hap.main_abc().unwrap()).unwrap();
    let file = AbcFile::parse(bytes).unwrap();
    let mut decompiled = 0;
    for c in file.classes.iter().take(40) {
        let rec = decompile::decompile_record(&file, c).unwrap();
        if rec.source.contains("function ") || rec.source.contains("class ") {
            decompiled += 1;
        }
    }
    assert!(
        decompiled > 5,
        "expected decompilable records, got {decompiled}"
    );
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn appgallery_widget_constructor_marks_erased_types_and_arkui_hints() {
    let path = super::corpus_path("AppGallery/entry.hap");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    let record = file.classes.iter().find(|record| record.name == "L&@hw-ae-presentation/entry-har/src/main/ets/widget/pages/HarWidgetCard&7.2.1-100;").unwrap();
    let constructor = record
        .methods
        .iter()
        .find(|method| method.name == "#~@0=#e408")
        .unwrap();
    assert_eq!(constructor.proto_off, 0);
    assert_eq!(constructor.code.as_ref().unwrap().num_args, 9);
    let debug = file.read_debug_info(constructor).unwrap().unwrap();
    assert!(debug.parameter_names.is_empty() && debug.locals.is_empty());
    let source = decompile::decompile_record(&file, record).unwrap().source;
    assert!(source.contains("types inferred from ArkUI ViewPU"));
    assert!(
        source.contains("constructor(\n        parent: IView | undefined,"),
        "{source}"
    );
    assert!(source.contains("params: unknown,"));
    assert!(source.contains("elmtId: number | undefined,"));
    assert!(source.contains("super(parent, localStorage, elmtId, extraInfo)"));
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn appgallery_analytics_constants_recover_all_72_static_values() {
    let path = super::corpus_path("AppGallery/entry.hap");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    let record = file
        .classes
        .iter()
        .find(|record| {
            record
                .name
                .contains("hianalytics-js/src/main/ets/common/Constants&")
        })
        .unwrap();
    let source = decompile::decompile_record(&file, record).unwrap().source;
    assert_eq!(source.matches("    static ").count(), 72, "{source}");
    assert!(source.contains("static DEFAULT_TAG = \"ha_standard\";"));
    assert!(source.contains("static TYPE_OPER = 0;"));
    assert!(source.contains("static ON_APP_START = \"$APP_START\";"));
    assert!(!source.contains("definepropertybyname"));
    assert!(!source.contains("Object.defineProperty"));
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn appgallery_download_key_resolver_initializes_register_declarations() {
    let path = super::corpus_path("AppGallery/@hw-ae-business+download-install-default.hsp");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    let record = file
        .classes
        .iter()
        .find(|record| {
            record
                .name
                .contains("DownloadManager/bean/DownloadTaskUniqueKeyResolver&")
        })
        .expect("DownloadTaskUniqueKeyResolver record present");
    let source = decompile::decompile_record(&file, record).unwrap().source;
    assert!(
        source.contains("static getUniqueKeyType(a0) {\n        let v0 = DownloadSceneType;"),
        "{source}"
    );
    assert!(
        source.contains("let v0 = DownloadTaskUniqueKeyResolver.SEPARATOR;"),
        "{source}"
    );
    assert!(
        source.contains("let v1 = DownloadTaskUniqueKeyType;"),
        "{source}"
    );
    // v2 的首次写入发生在条件分支，仍需在方法作用域声明。
    assert!(source.contains("        let v2;"), "{source}");
    assert!(!source.contains("let v2 = RemoteInstallType;"), "{source}");
    assert!(!source.contains("let v0;"), "{source}");
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn appgallery_entry_property_tables_rest_callbacks_and_object_methods_recover() {
    let path = super::corpus_path("AppGallery/entry.hap");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    for fragment in [
        "/startup/util/DiversionUtil&",
        "/BiReport/VideoBiReportUtil&",
        "/preload/util/SilenceUtil&",
        "/model/pattern/PafListModel&",
        "/agreement/GlobalAgreementInit&",
        "/consent/src/main/ets/f/g/h&",
    ] {
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains(fragment))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for missing in [
            "Conservative fallback",
            "method@",
            "definefieldbyvalue",
            "definefieldbyindex",
            "stownbyvaluewithnameset",
        ] {
            assert!(!source.contains(missing), "{fragment}: {missing}");
        }
        if fragment.contains("VideoBiReportUtil") {
            for key in ["PLAY_ICON_CLICK", "MUTE_ICON_CLICK", "FULL_ICON_CLICK"] {
                assert!(
                    source.contains(&format!("Object.defineProperty(v3, VideoEventId.{key},")),
                    "{fragment}: {key}"
                );
            }
        }
        if fragment.contains("SilenceUtil") {
            for key in 0..8 {
                assert!(
                    source.contains(&format!("Object.defineProperty(v0, {key},")),
                    "{fragment}: {key}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn legacy_abc_parses_and_mostly_decodes() {
    let abc_path = super::corpus_path("system_framework/crypto_framework_ets.abc");
    let file = AbcFile::parse(std::fs::read(&abc_path).unwrap()).unwrap();
    assert_eq!(file.header.kind, HeaderKind::Legacy);
    assert!(
        file.classes.len() > 100,
        "legacy records must be enumerable, got {}",
        file.classes.len()
    );
    let (ok, total) = decode_coverage(&file);
    // 0.1.0.x 覆盖表下 ~90%；低于 80% 视为回归
    let rate = ok as f64 / total.max(1) as f64;
    assert!(
        rate > 0.80,
        "legacy decode coverage regression: {ok}/{total} ({rate:.1}%)"
    );
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn appgallery_about_sheet_recovers_imported_base_and_implicit_constructor() {
    let path = super::corpus_path("AppGallery/entry.hap");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    let record = file
        .classes
        .iter()
        .find(|record| {
            record
                .name
                .contains("ui-widget-about-v2/src/default/ets/model/PafAboutSheetConfig&")
        })
        .unwrap();
    let source = decompile::decompile_record(&file, record).unwrap().source;
    assert!(
        source.contains("class PafAboutSheetConfig extends PafSheetConfig {"),
        "{source}"
    );
    assert!(source.contains("super(...[...arguments]);"), "{source}");
    assert!(source.contains("this.isSheetShow = false;"), "{source}");
    assert!(source.contains("    setSheetShowStatus("), "{source}");
    assert!(source.contains("    getSheetShowStatus()"), "{source}");
    for invalid in [
        "function ",
        "import:",
        "getunmappedargs",
        "supercallspread",
        "currentFunction",
        "catch (",
        ".push(",
    ] {
        assert!(!source.contains(invalid), "{source}");
    }
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn hap_listing_categorizes_entries() {
    let hap_path = super::corpus_path("Hinote/HiNote.hap");
    let hap = HapArchive::parse(std::fs::read(&hap_path).unwrap()).unwrap();
    assert!(hap.entries.iter().any(|e| e.is_abc));
    assert!(hap.entries.iter().any(|e| e.is_so));
    assert_eq!(hap.main_abc(), Some("ets/modules.abc"));
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn hinote_logutil_lifts_globals_and_lexvars() {
    // 回归：tryldglobalbyname 未实现 / ldlexvar 误写寄存器时，
    // LogUtil 会渲染成 undefined.info(...) 且操作数串位
    let hap_path = super::corpus_path("Hinote/HiNote.hap");
    let hap = HapArchive::parse(std::fs::read(&hap_path).unwrap()).unwrap();
    let bytes = hap.read_entry(hap.main_abc().unwrap()).unwrap();
    let file = AbcFile::parse(bytes).unwrap();
    let rec = file
        .classes
        .iter()
        .find(|c| c.name.ends_with("common/LogUtil&13.0.1-128;"))
        .expect("LogUtil record present");
    let out = decompile::decompile_record(&file, rec).unwrap();
    assert!(
        out.source.contains("globalThis.console") && out.source.contains(".info("),
        "tryldglobalbyname must resolve the global: {}",
        &out.source[..out.source.len().min(400)]
    );
    assert!(
        !out.source.contains("undefined.info("),
        "unresolved global must not leak undefined callee"
    );
    // ldlexvar 值进 acc：拼接链不应把调用目标吞进操作数
    assert!(
        !out.source.contains("+ globalThis.console.info))"),
        "callee must not leak into concat operand"
    );
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn hinote_diagnostic_records_recover_module_class_and_private_instructions() {
    let path = super::corpus_path("Hinote/HiNote.hap");
    let hap = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(hap.read_entry(hap.main_abc().unwrap()).unwrap()).unwrap();
    let (decoded, total) = decode_coverage(&file);
    assert!(total > 100);
    assert_eq!(decoded, total);
    for fragment in [
        "hwr/sdk/Params&",
        "/model/FocusArea&",
        "p-limit/index&",
        "yocto-queue/index&",
        "/http/HttpClientSelectorImpl&",
    ] {
        let record = file
            .classes
            .iter()
            .find(|record| record.name.contains(fragment))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for missing in [
            "/*definesendableclass",
            "/* callruntime.ldsendableexternalmodulevar",
            "/* wide.stmodulevar",
            "/* wide.ldlocalmodulevar",
            "/* definegettersetterbyvalue",
            "/* ldprivateproperty",
            "/* stprivateproperty",
            "/* callruntime.defineprivateproperty",
            "/* callruntime.createprivateproperty",
            "/* callruntime.callinit",
            "unrecognized async resume protocol",
        ] {
            assert!(!source.contains(missing), "{fragment}: {missing}");
        }
        if fragment == "hwr/sdk/Params&" {
            assert!(source.contains("@Sendable\nclass Params"), "{source}");
            assert!(source.contains("static createInstance()"), "{source}");
            assert!(source.contains("new ParamsImpl("), "{source}");
        }
        if fragment == "yocto-queue/index&" {
            assert!(
                source.contains("unrecognized generator resume protocol"),
                "{source}"
            );
            assert!(
                !source.contains("let v1 = 0;\n        /* creategeneratorobj"),
                "{source}"
            );
        }
    }
}
