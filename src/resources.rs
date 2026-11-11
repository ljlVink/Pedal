use pedal_abc_bridge::error::{AbcError, AbcResult};
use pedal_abc_bridge::hap::HapArchive;
use pedal_abc_bridge::resource::{MAX_INDEX_SIZE, Qualifier, ResourceIndex, ResourceValue};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn dump(index: &ResourceIndex, descriptor: Option<&[u8]>, config_only: bool) -> Value {
    if config_only {
        let configs: BTreeSet<_> = index
            .entries
            .iter()
            .map(|e| config_name(&e.qualifiers))
            .collect();
        return json!({ "config": configs });
    }
    let mut grouped = BTreeMap::<_, Vec<_>>::new();
    for entry in &index.entries {
        grouped.entry(entry.id).or_default().push(entry);
    }
    let resources: Vec<_> = grouped
        .into_iter()
        .map(|(id, entries)| {
            let first = entries[0];
            let values: Vec<_> = entries
                .into_iter()
                .map(|entry| {
                    let mut value = Map::new();
                    match &entry.value {
                        ResourceValue::String(text) => {
                            value.insert("value".into(), json!(text));
                        }
                        ResourceValue::Array(items) => {
                            value.insert("value".into(), json!(items));
                        }
                        ResourceValue::Pairs { parent, values } => {
                            let pairs: Map<_, _> =
                                values.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                            value.insert("value".into(), Value::Object(pairs));
                            if let Some(parent) = parent {
                                value.insert("parent".into(), json!(parent));
                            }
                        }
                    }
                    for key in &entry.qualifiers {
                        value.insert(qualifier_name(key.kind), json!(qualifier_value(key)));
                    }
                    Value::Object(value)
                })
                .collect();
            json!({"id": id, "name": first.name, "type": type_name(first.resource_type),
            "entryCount": values.len(), "entryValues": values})
        })
        .collect();
    let mut result = json!({"resource": resources});
    if let Some(descriptor) = descriptor.and_then(|data| serde_json::from_slice::<Value>(data).ok())
        && let (Some(bundle), Some(module)) = (
            descriptor
                .pointer("/app/bundleName")
                .and_then(Value::as_str),
            descriptor.pointer("/module/name").and_then(Value::as_str),
        )
        && !bundle.is_empty()
        && !module.is_empty()
    {
        result["bundleName"] = json!(bundle);
        result["moduleName"] = json!(module);
    }
    result
}

pub(crate) fn dump_package_entry(
    hap: &HapArchive,
    path: &str,
    config_only: bool,
) -> AbcResult<String> {
    let (index, descriptor) = read_package_index(hap, path)?;
    pretty(&dump(&index, descriptor.as_deref(), config_only))
}

pub(crate) fn read_package_index(
    hap: &HapArchive,
    path: &str,
) -> AbcResult<(ResourceIndex, Option<Vec<u8>>)> {
    let bytes = hap.read_entry_limited(path, MAX_INDEX_SIZE)?;
    let index = ResourceIndex::parse(&bytes)?;
    let prefix = path.strip_suffix("resources.index").unwrap_or("");
    let descriptor = ["module.json", "config.json"].into_iter().find_map(|name| {
        hap.read_entry_limited(&format!("{prefix}{name}"), MAX_INDEX_SIZE)
            .ok()
    });
    Ok((index, descriptor))
}

pub(crate) fn pretty(value: &Value) -> AbcResult<String> {
    serde_json::to_string_pretty(value).map_err(|e| AbcError::Io(e.to_string()))
}

pub(crate) fn output_stem(path: &str) -> String {
    let safe = path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with(['.', ' '])
            && !part
                .chars()
                .any(|c| c.is_control() || "<>:\"\\|?*".contains(c))
            && !matches!(
                part.split('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase()
                    .as_str(),
                "CON"
                    | "PRN"
                    | "AUX"
                    | "NUL"
                    | "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            )
    });
    if safe {
        path.into()
    } else {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(path.as_bytes());
        let name: String = hash[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("resources-{name}.index")
    }
}

pub(crate) fn type_name(kind: u32) -> String {
    match kind {
        0 => "element",
        6 => "rawfile",
        8 => "integer",
        9 => "string",
        10 => "strarray",
        11 => "intarray",
        12 => "boolean",
        14 => "color",
        15 => "id",
        16 => "theme",
        17 => "plural",
        18 => "float",
        19 => "media",
        20 => "profile",
        22 => "pattern",
        23 => "symbol",
        24 => "resfile",
        _ => return format!("unknown type: {kind}"),
    }
    .into()
}

pub(crate) fn qualifier_name(kind: u32) -> String {
    match kind {
        0 => "language",
        1 => "region",
        2 => "density",
        3 => "orientation",
        4 => "device",
        5 => "script",
        6 => "colorMode",
        7 => "mcc",
        8 => "mnc",
        10 => "inputDevice",
        _ => return format!("unknown type: {kind}"),
    }
    .into()
}

pub(crate) fn qualifier_value(key: &Qualifier) -> String {
    let value = key.value;
    match key.kind {
        0 | 1 | 5 => value
            .to_be_bytes()
            .into_iter()
            .filter(|b| *b != 0)
            .map(char::from)
            .collect(),
        2 => match value {
            120 => "sdpi",
            160 => "mdpi",
            240 => "ldpi",
            320 => "xldpi",
            480 => "xxldpi",
            640 => "xxxldpi",
            _ => return value.to_string(),
        }
        .into(),
        3 => match value {
            0 => "vertical",
            1 => "horizontal",
            _ => return value.to_string(),
        }
        .into(),
        4 => match value {
            0 => "phone",
            1 => "tablet",
            2 => "car",
            4 => "tv",
            6 => "wearable",
            7 => "2in1",
            _ => return value.to_string(),
        }
        .into(),
        6 => match value {
            0 => "dark",
            1 => "light",
            _ => return value.to_string(),
        }
        .into(),
        10 => match value {
            0 => "pointDevice",
            u32::MAX => "not set",
            _ => return value.to_string(),
        }
        .into(),
        _ => value.to_string(),
    }
}

pub(crate) fn config_name(keys: &[Qualifier]) -> String {
    if keys.is_empty() {
        return "base".into();
    }
    let mut result = String::new();
    for key in keys {
        if !result.is_empty() {
            result.push(if matches!(key.kind, 1 | 8) { '_' } else { '-' });
        }
        if key.kind == 7 {
            result.push_str("mcc");
        }
        if key.kind == 8 {
            result.push_str("mnc");
        }
        result.push_str(&qualifier_value(key));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(value: &mut Value) {
        if let Some(resources) = value["resource"].as_array_mut() {
            for resource in resources {
                resource["entryValues"]
                    .as_array_mut()
                    .unwrap()
                    .sort_by_key(Value::to_string);
            }
        }
        if let Some(configs) = value["config"].as_array_mut() {
            configs.sort_by_key(Value::to_string);
        }
    }

    #[test]
    fn dump_matches_official_restool_for_both_formats() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/pedal-abc-bridge/tests/fixtures/resources");
        let descriptor = std::fs::read(root.join("module.json")).unwrap();
        for version in ["v1", "v2"] {
            let index = ResourceIndex::parse(
                &std::fs::read(root.join(format!("{version}.index"))).unwrap(),
            )
            .unwrap();
            for config_only in [false, true] {
                let suffix = if config_only { ".config" } else { "" };
                let mut expected: Value = serde_json::from_slice(
                    &std::fs::read(root.join(format!("{version}{suffix}.json"))).unwrap(),
                )
                .unwrap();
                let mut actual = dump(&index, Some(&descriptor), config_only);
                normalize(&mut expected);
                normalize(&mut actual);
                assert_eq!(actual, expected, "{version}, config={config_only}");
            }
        }
    }

    #[test]
    fn generated_paths_remain_relative_and_portable() {
        assert_eq!(
            output_stem("assets/entry/resources.index"),
            "assets/entry/resources.index"
        );
        let paths = [
            "../resources.index",
            "/resources.index",
            "C:/resources.index",
            "..\\resources.index",
            "CON/resources.index",
            "x./resources.index",
        ];
        let outputs: BTreeSet<_> = paths.into_iter().map(output_stem).collect();
        assert_eq!(outputs.len(), paths.len());
        assert!(
            outputs
                .iter()
                .all(|path| path.starts_with("resources-") && !path.contains(['/', '\\', ':']))
        );
    }
}
