use super::*;

const V1: &[u8] = include_bytes!("../../tests/fixtures/resources/v1.index");
const V2: &[u8] = include_bytes!("../../tests/fixtures/resources/v2.index");

#[test]
fn official_indexes_preserve_values_and_qualifiers() {
    for bytes in [V1, V2] {
        let index = ResourceIndex::parse(bytes).unwrap();
        let greeting: Vec<_> = index
            .entries
            .iter()
            .filter(|e| e.name == "greeting")
            .collect();
        assert_eq!(greeting.len(), 2);
        assert!(
            greeting
                .iter()
                .any(|e| e.value == ResourceValue::String("你好，世界".into())
                    && e.qualifiers.contains(&Qualifier {
                        kind: 0,
                        value: 0x7a68
                    }))
        );
        assert!(
            greeting
                .iter()
                .any(|e| e.value == ResourceValue::String("Hello \"Ark\"\n世界".into()))
        );
        let labels = index.entries.iter().find(|e| e.name == "labels").unwrap();
        assert_eq!(
            labels.value,
            ResourceValue::Array(vec!["Alpha".into(), "中文".into(), "".into()])
        );
        let items = index.entries.iter().find(|e| e.name == "items").unwrap();
        assert!(
            matches!(&items.value, ResourceValue::Pairs { parent: None, values }
            if values.contains(&("other".into(), "%d items".into())))
        );
        assert!(
            index
                .entries
                .iter()
                .any(|e| e.name == "icon" && e.resource_type == 19)
        );
        assert!(
            index
                .entries
                .iter()
                .any(|e| e.name == "pages" && e.resource_type == 20)
        );
    }
}

#[test]
fn truncated_indexes_fail_even_with_adjusted_file_size() {
    for original in [V1, V2] {
        for end in 0..original.len() {
            let mut bytes = original[..end].to_vec();
            if end >= 132 {
                bytes[128..132].copy_from_slice(&(end as u32).to_le_bytes());
            }
            assert!(
                ResourceIndex::parse(&bytes).is_err(),
                "accepted truncated index at {end}"
            );
        }
    }
}

#[test]
fn invalid_headers_counts_and_offsets_fail() {
    for original in [V1, V2] {
        for offset in [0, 128, 132] {
            let mut bytes = original.to_vec();
            bytes[offset..offset + 4].fill(0xff);
            assert!(ResourceIndex::parse(&bytes).is_err(), "offset {offset}");
        }
        let keys = original.windows(4).position(|v| v == b"KEYS").unwrap();
        for offset in [keys, keys + 8] {
            let mut bytes = original.to_vec();
            bytes[offset..offset + 4].fill(0xff);
            assert!(ResourceIndex::parse(&bytes).is_err(), "offset {offset}");
        }
        let mut bytes = original.to_vec();
        bytes[136..140].fill(0xff);
        assert!(ResourceIndex::parse(&bytes).is_err());
    }
}

#[test]
fn mutated_indexes_never_panic() {
    for original in [V1, V2] {
        for offset in 0..original.len() {
            let mut bytes = original.to_vec();
            bytes[offset] ^= 0xff;
            let _ = ResourceIndex::parse(&bytes);
        }
    }
}
