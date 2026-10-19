//! 导出一个 record 的完整伪 ArkTS，用于源码与反编译行为对照。
//!
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: export_record <file.abc> <record substring> <output.ets>");
        std::process::exit(2);
    }
    let file = AbcFile::parse(std::fs::read(&args[1])?)?;
    let matches: Vec<_> = file
        .classes
        .iter()
        .filter(|record| record.name.contains(&args[2]))
        .collect();
    if matches.len() != 1 {
        return Err(format!(
            "expected one matching record, found {}: {:?}",
            matches.len(),
            matches
                .iter()
                .map(|record| &record.name)
                .collect::<Vec<_>>()
        )
        .into());
    }
    let output = decompile::decompile_record(&file, matches[0])?;
    std::fs::write(&args[3], output.source)?;
    println!("{} -> {}", output.name, args[3]);
    Ok(())
}
