use crate::error::{AbcError, AbcResult};
use crate::reader::Reader;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;
mod v2;

pub const MAX_INDEX_SIZE: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;
const MAX_DECODED_SIZE: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qualifier {
    pub kind: u32,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceValue {
    String(String),
    Array(Vec<String>),
    Pairs {
        parent: Option<String>,
        values: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEntry {
    pub id: u32,
    pub name: String,
    pub resource_type: u32,
    pub qualifiers: Vec<Qualifier>,
    pub value: ResourceValue,
}

#[derive(Debug, Clone)]
pub struct ResourceIndex {
    pub version: String,
    pub entries: Vec<ResourceEntry>,
}

impl ResourceIndex {
    pub fn parse(data: &[u8]) -> AbcResult<Self> {
        if data.len() > MAX_INDEX_SIZE {
            return Err(invalid(0, "index exceeds size limit"));
        }
        let mut reader = Reader::new(data);
        let version_bytes = take(&mut reader, 128)?;
        let version_end = version_bytes.iter().position(|b| *b == 0).unwrap_or(128);
        let version = utf8(&version_bytes[..version_end], 0)?;
        let is_v2 = if version.starts_with("RestoolV2 ") {
            true
        } else if version.starts_with("Restool ") {
            false
        } else {
            return Err(invalid(0, "unsupported resource index signature"));
        };
        let size = reader.u32()? as usize;
        if size != data.len() {
            return Err(invalid(128, "declared file size does not match input"));
        }
        let config_count = count(&mut reader, 12)?;
        let mut budget = MAX_DECODED_SIZE;
        let mut entries = if is_v2 {
            v2::parse(&mut reader, config_count, &mut budget)?
        } else {
            parse_v1(&mut reader, config_count, &mut budget)?
        };
        entries.sort_by_key(|entry| entry.id);
        let mut identities = BTreeMap::new();
        for entry in &entries {
            if let Some((name, kind)) =
                identities.insert(entry.id, (&entry.name, entry.resource_type))
                && (name != &entry.name || kind != entry.resource_type)
            {
                return Err(invalid(0, "resource ID has conflicting names or types"));
            }
        }
        Ok(Self { version, entries })
    }
}

fn parse_v1(
    reader: &mut Reader<'_>,
    config_count: usize,
    budget: &mut usize,
) -> AbcResult<Vec<ResourceEntry>> {
    let mut configs = Vec::new();
    let mut offsets = BTreeSet::new();
    for _ in 0..config_count {
        tag(reader, b"KEYS")?;
        let offset = reader.u32()? as usize;
        if !offsets.insert(offset) {
            return Err(invalid(reader.pos(), "duplicate ID table offset"));
        }
        configs.push((offset, qualifiers(reader)?));
    }
    let keys_end = reader.pos();
    let mut references = Vec::new();
    let mut tables_end = keys_end;
    for (offset, keys) in &configs {
        if *offset < keys_end {
            return Err(invalid(*offset, "ID table overlaps configuration header"));
        }
        reader.seek(*offset)?;
        tag(reader, b"IDSS")?;
        let id_count = count(reader, 8)?;
        let mut ids = BTreeSet::new();
        for _ in 0..id_count {
            let id = reader.u32()?;
            let offset = reader.u32()? as usize;
            if !ids.insert(id) || references.len() >= MAX_ENTRIES {
                return Err(invalid(reader.pos(), "duplicate or excessive resource IDs"));
            }
            references.push((id, offset, keys));
        }
        tables_end = tables_end.max(reader.pos());
    }
    let mut entries = Vec::new();
    for (id, offset, keys) in references {
        if offset < tables_end {
            return Err(invalid(offset, "resource overlaps index tables"));
        }
        reader.seek(offset)?;
        let size = reader.u32()? as usize;
        let mut record = Reader::new(take(reader, size)?);
        let resource_type = record.u32()?;
        if record.u32()? != id {
            return Err(invalid(offset, "resource ID does not match ID table"));
        }
        let value_data = length_prefixed(&mut record)?;
        let value_data = terminated(value_data, offset)?;
        let name_data = terminated(length_prefixed(&mut record)?, offset)?;
        charge(
            budget,
            value_data.len() + name_data.len() + keys.len() * 8,
            offset,
        )?;
        let value = decode_value(value_data, resource_type, offset)?;
        let name = utf8(name_data, offset)?;
        if record.remaining() != 0 {
            return Err(invalid(offset, "trailing bytes in resource record"));
        }
        entries.push(ResourceEntry {
            id,
            name,
            resource_type,
            qualifiers: keys.clone(),
            value,
        });
    }
    Ok(entries)
}

fn decode_value(data: &[u8], kind: u32, offset: usize) -> AbcResult<ResourceValue> {
    if !matches!(kind, 10 | 11 | 16 | 17 | 22) {
        return Ok(ResourceValue::String(utf8(data, offset)?));
    }
    let mut reader = Reader::new(data);
    let mut strings = Vec::new();
    while reader.remaining() != 0 {
        let bytes = length_prefixed(&mut reader)?;
        strings.push(utf8(bytes, offset)?);
        if reader.u8()? != 0 {
            return Err(invalid(offset, "array item is not terminated"));
        }
    }
    if matches!(kind, 10 | 11) {
        return Ok(ResourceValue::Array(strings));
    }
    let parent = if strings.len() % 2 == 1 {
        Some(strings.remove(0))
    } else {
        None
    };
    let mut values = Vec::new();
    let mut items = strings.into_iter();
    while let (Some(key), Some(value)) = (items.next(), items.next()) {
        values.push((key, value));
    }
    Ok(ResourceValue::Pairs { parent, values })
}

fn qualifiers(reader: &mut Reader<'_>) -> AbcResult<Vec<Qualifier>> {
    let count = count(reader, 8)?;
    if count > 256 {
        return Err(invalid(reader.pos(), "too many qualifiers"));
    }
    let mut result = Vec::new();
    let mut kinds = BTreeSet::new();
    for _ in 0..count {
        let kind = reader.u32()?;
        let value = reader.u32()?;
        if !kinds.insert(kind) {
            return Err(invalid(reader.pos(), "duplicate qualifier"));
        }
        result.push(Qualifier { kind, value });
    }
    Ok(result)
}

fn take<'a>(reader: &mut Reader<'a>, size: usize) -> AbcResult<&'a [u8]> {
    if size > reader.remaining() {
        return Err(invalid(reader.pos(), "truncated resource data"));
    }
    let start = reader.pos();
    reader.seek(start + size)?;
    Ok(&reader.bytes()[start..start + size])
}

fn length_prefixed<'a>(reader: &mut Reader<'a>) -> AbcResult<&'a [u8]> {
    let size = reader.u16()? as usize;
    take(reader, size)
}

fn terminated(data: &[u8], offset: usize) -> AbcResult<&[u8]> {
    data.strip_suffix(&[0])
        .ok_or_else(|| invalid(offset, "string is not terminated"))
}

fn utf8(data: &[u8], offset: usize) -> AbcResult<String> {
    std::str::from_utf8(data)
        .map(str::to_owned)
        .map_err(|_| invalid(offset, "invalid UTF-8"))
}

fn tag(reader: &mut Reader<'_>, expected: &[u8; 4]) -> AbcResult<()> {
    let offset = reader.pos();
    if take(reader, 4)? != expected {
        return Err(invalid(offset, "invalid block tag"));
    }
    Ok(())
}

fn count(reader: &mut Reader<'_>, stride: usize) -> AbcResult<usize> {
    let count = reader.u32()? as usize;
    if count > MAX_ENTRIES || count > reader.remaining() / stride {
        return Err(invalid(reader.pos(), "invalid item count"));
    }
    Ok(count)
}

fn charge(budget: &mut usize, size: usize, offset: usize) -> AbcResult<()> {
    *budget = budget
        .checked_sub(size)
        .ok_or_else(|| invalid(offset, "decoded resources exceed size limit"))?;
    Ok(())
}

fn invalid(offset: usize, reason: &'static str) -> AbcError {
    AbcError::ResourceIndex { offset, reason }
}
