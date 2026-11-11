use super::corpus_path;
use pedal_abc_bridge::hap::HapArchive;
use pedal_abc_bridge::resource::{MAX_INDEX_SIZE, ResourceIndex, ResourceValue};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

fn check_package(path: &Path, expected_missing: &[&str]) -> usize {
    let archive = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let bytes = archive
        .read_entry_limited("resources.index", MAX_INDEX_SIZE)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let index =
        ResourceIndex::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(!index.entries.is_empty(), "{}", path.display());
    let mut missing = BTreeSet::new();
    for entry in &index.entries {
        assert!(!entry.name.is_empty(), "{}: {}", path.display(), entry.id);
        if matches!(entry.resource_type, 19 | 20) {
            let ResourceValue::String(value) = &entry.value else {
                panic!("file resource has no path")
            };
            if let Some((_, relative)) = value.split_once("resources/") {
                let resource_path = format!("resources/{relative}");
                if !archive.entries.iter().any(|e| e.path == resource_path) {
                    missing.insert(resource_path);
                }
            }
        }
    }
    assert_eq!(
        missing,
        expected_missing.iter().map(|p| p.to_string()).collect(),
        "{}",
        path.display()
    );
    eprintln!(
        "{}: {} resource variants",
        path.display(),
        index.entries.len()
    );
    index.entries.len()
}

#[test]
#[ignore = "requires test_examples/AppGallery HAP/HSP resource corpus"]
fn appgallery_resources() {
    let entry = corpus_path("AppGallery/entry.hap");
    let mut packages: Vec<_> = std::fs::read_dir(entry.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "hap" || ext == "hsp")
        })
        .collect();
    packages.sort();
    let mut variants = 0;
    for package in packages {
        variants += check_package(&package, &[]);
    }
    assert!(variants > 1000);
}

#[test]
#[ignore = "requires test_examples/Hinote/HiNote.hap"]
fn hinote_resources() {
    check_package(&corpus_path("Hinote/HiNote.hap"), &[]);
}

#[test]
#[ignore = "requires test_examples/encrypted HAP"]
fn encrypted_package_resources() {
    check_package(
        &corpus_path("encrypted/1791634601426-357f36c5-48b7-494e-a2ad-32a21eb3c2ca.hap"),
        &["resources/base/profile/OWNERS"],
    );
}

#[test]
#[ignore = "requires test_examples/arkts_probe SDK build output"]
fn probe_resources() {
    check_package(
        &corpus_path("arkts_probe/entry/build/default/outputs/default/entry-default-unsigned.hap"),
        &[],
    );
}

#[test]
#[ignore = "requires test_examples/weixin_decryptedDump/com.tencent.wechat_decrypted_unsigned.hap"]
fn wechat_resources() {
    let path = corpus_path("weixin_decryptedDump/com.tencent.wechat_decrypted_unsigned.hap");
    assert_eq!(check_package(&path, &[]), 16_045);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut png_count = 0;
    let mut webp_count = 0;
    let mut json_count = 0;
    let mut max_pixels = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if !entry.name().to_ascii_lowercase().ends_with(".png") {
            continue;
        }
        let mut header = [0; 24];
        entry.read_exact(&mut header).unwrap();
        if header.starts_with(b"\x89PNG\r\n\x1a\n") {
            assert_eq!(&header[12..16], b"IHDR", "{}", entry.name());
            let width = u32::from_be_bytes(header[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(header[20..24].try_into().unwrap());
            assert!(width > 0 && height > 0, "{}", entry.name());
            max_pixels = max_pixels.max(u64::from(width) * u64::from(height));
            png_count += 1;
        } else if header.starts_with(b"RIFF") && &header[8..12] == b"WEBP" {
            webp_count += 1;
        } else {
            assert!(header.starts_with(b"{\n"), "{}", entry.name());
            assert!(
                entry
                    .name()
                    .starts_with("resources/resfile/mapsdk/config/mapcfg_arrow_batching")
            );
            json_count += 1;
        }
    }
    assert_eq!((png_count, webp_count, json_count), (1307, 75, 2));
    assert_eq!(max_pixels, 2_742_336);
}
