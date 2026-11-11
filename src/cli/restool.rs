use crate::resources;
use pedal_abc_bridge::error::AbcResult;
use pedal_abc_bridge::hap::HapArchive;
use pedal_abc_bridge::resource::{MAX_INDEX_SIZE, ResourceIndex};
use std::io::{Read, Write};
use std::path::Path;

pub(super) fn read(input: &Path, config_only: bool) -> AbcResult<String> {
    if input
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("index"))
    {
        let mut data = Vec::new();
        std::fs::File::open(input)?
            .take(MAX_INDEX_SIZE as u64 + 1)
            .read_to_end(&mut data)?;
        let index = ResourceIndex::parse(&data)?;
        resources::pretty(&resources::dump(&index, None, config_only))
    } else {
        let hap = HapArchive::parse(std::fs::read(input)?)?;
        resources::dump_package_entry(&hap, "resources.index", config_only)
    }
}

pub(super) fn run(input: &Path, config_only: bool) -> AbcResult<()> {
    let text = read(input, config_only)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}
