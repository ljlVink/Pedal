use super::*;

pub(super) fn parse(
    reader: &mut Reader<'_>,
    config_count: usize,
    budget: &mut usize,
) -> AbcResult<Vec<ResourceEntry>> {
    let data_offset = reader.u32()? as usize;
    let mut configs = BTreeMap::new();
    for _ in 0..config_count {
        tag(reader, b"KEYS")?;
        let id = reader.u32()?;
        if configs.insert(id, qualifiers(reader)?).is_some() {
            return Err(invalid(reader.pos(), "duplicate configuration ID"));
        }
    }
    let ids_start = reader.pos();
    tag(reader, b"IDSS")?;
    let ids_size = reader.u32()? as usize;
    if ids_size < 16 || ids_start.checked_add(ids_size) != Some(data_offset) {
        return Err(invalid(ids_start, "invalid ID table length"));
    }
    let mut ids = Reader::new(take(reader, ids_size - 8)?);
    let type_count = count(&mut ids, 12)?;
    let id_count = count(&mut ids, 12)?;
    let mut names = BTreeMap::new();
    let mut types = BTreeSet::new();
    for _ in 0..type_count {
        let kind = ids.u32()?;
        let type_size = ids.u32()? as usize;
        if type_size < 12 || !types.insert(kind) {
            return Err(invalid(ids_start, "invalid resource type header"));
        }
        let mut type_block = Reader::new(take(&mut ids, type_size - 8)?);
        let count = count(&mut type_block, 12)?;
        for _ in 0..count {
            let id = type_block.u32()?;
            let offset = type_block.u32()? as usize;
            let size = type_block.u32()? as usize;
            let name = take(&mut type_block, size)?;
            charge(budget, size, ids_start)?;
            if names
                .insert(id, (kind, utf8(name, ids_start)?, offset))
                .is_some()
            {
                return Err(invalid(ids_start, "duplicate resource ID"));
            }
        }
        if type_block.remaining() != 0 {
            return Err(invalid(ids_start, "resource type length mismatch"));
        }
    }
    if ids.remaining() != 0 || names.len() != id_count {
        return Err(invalid(ids_start, "ID table count or length mismatch"));
    }
    tag(reader, b"DATA")?;
    let data_size = reader.u32()? as usize;
    if data_size < 12 {
        return Err(invalid(data_offset, "invalid data table length"));
    }
    let mut records = Reader::new(take(reader, data_size - 8)?);
    let record_count = count(&mut records, 12)?;
    if record_count != id_count {
        return Err(invalid(data_offset, "data table count mismatch"));
    }
    let pool_start = reader.pos();
    let mut entries = Vec::new();
    for _ in 0..record_count {
        let offset = data_offset + 8 + records.pos();
        let id = records.u32()?;
        let size = records.u32()? as usize;
        let Some((kind, name, expected_offset)) = names.remove(&id) else {
            return Err(invalid(offset, "unknown or duplicate resource ID"));
        };
        if offset != expected_offset || size < 12 {
            return Err(invalid(offset, "resource record offset or length mismatch"));
        }
        let mut record = Reader::new(take(&mut records, size - 8)?);
        let value_count = count(&mut record, 8)?;
        if value_count == 0 || value_count > MAX_ENTRIES - entries.len() {
            return Err(invalid(offset, "invalid resource value count"));
        }
        let mut seen_configs = BTreeSet::new();
        for _ in 0..value_count {
            let config_id = record.u32()?;
            let value_offset = record.u32()? as usize;
            let Some(keys) = configs.get(&config_id) else {
                return Err(invalid(offset, "unknown configuration ID"));
            };
            if !seen_configs.insert(config_id) || value_offset < pool_start {
                return Err(invalid(
                    offset,
                    "duplicate configuration or invalid value offset",
                ));
            }
            reader.seek(value_offset)?;
            let value_data = length_prefixed(reader)?;
            charge(
                budget,
                value_data.len() + name.len() + keys.len() * 8,
                value_offset,
            )?;
            entries.push(ResourceEntry {
                id,
                name: name.clone(),
                resource_type: kind,
                qualifiers: keys.clone(),
                value: decode_value(value_data, kind, value_offset)?,
            });
        }
        if record.remaining() != 0 {
            return Err(invalid(offset, "resource record length mismatch"));
        }
    }
    if records.remaining() != 0 {
        return Err(invalid(data_offset, "data table length mismatch"));
    }
    Ok(entries)
}
