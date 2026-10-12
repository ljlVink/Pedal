//! 端到端集成测试：真实 abc / HAP 语料（缺失时自动跳过）
//!
//! 语料目录不入库，CI 上自动 skip；本地存在时验证：
//! - 容器解析（12.x 与 0.1.0.7 两代布局）
//! - 指令解码覆盖率
//! - 反编译产物非空且包含函数头
//!
//! @author ljlVink

use pedal_abc_bridge::decompile;
use pedal_abc_bridge::disasm;
use pedal_abc_bridge::file::{AbcFile, HeaderKind};
use pedal_abc_bridge::hap::HapArchive;
use std::path::Path;

const CORPUS: &str = "test_examples";

fn corpus_dir() -> Option<std::path::PathBuf> {
    let dir = Path::new(CORPUS);
    dir.exists().then(|| dir.to_path_buf())
}

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
fn modern_abc_from_hap_parses_and_decodes() {
    let Some(dir) = corpus_dir() else { return };
    let hap_path = dir.join("AppGallery/service.hap");
    if !hap_path.exists() {
        return;
    }
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
fn modern_abc_decompiles_records() {
    let Some(dir) = corpus_dir() else { return };
    let hap_path = dir.join("AppGallery/service.hap");
    if !hap_path.exists() {
        return;
    }
    let hap = HapArchive::parse(std::fs::read(&hap_path).unwrap()).unwrap();
    let bytes = hap.read_entry(hap.main_abc().unwrap()).unwrap();
    let file = AbcFile::parse(bytes).unwrap();
    let mut decompiled = 0;
    for c in file.classes.iter().take(40) {
        let rec = decompile::decompile_record(&file, c).unwrap();
        if rec.source.contains("function ") {
            decompiled += 1;
        }
    }
    assert!(
        decompiled > 5,
        "expected decompilable records, got {decompiled}"
    );
}

#[test]
fn legacy_abc_parses_and_mostly_decodes() {
    let Some(dir) = corpus_dir() else { return };
    let abc_path = dir.join("system_framework/crypto_framework_ets.abc");
    if !abc_path.exists() {
        return;
    }
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
fn hap_listing_categorizes_entries() {
    let Some(dir) = corpus_dir() else { return };
    let hap_path = dir.join("Hinote/HiNote.hap");
    if !hap_path.exists() {
        return;
    }
    let hap = HapArchive::parse(std::fs::read(&hap_path).unwrap()).unwrap();
    assert!(hap.entries.iter().any(|e| e.is_abc));
    assert!(hap.entries.iter().any(|e| e.is_so));
    assert_eq!(hap.main_abc(), Some("ets/modules.abc"));
}

#[test]
fn hinote_logutil_lifts_globals_and_lexvars() {
    // 回归：tryldglobalbyname 未实现 / ldlexvar 误写寄存器时，
    // LogUtil 会渲染成 undefined.info(...) 且操作数串位
    let Some(dir) = corpus_dir() else { return };
    let hap_path = dir.join("Hinote/HiNote.hap");
    if !hap_path.exists() {
        return;
    }
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
        out.source.contains("globalThis.console.info("),
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
