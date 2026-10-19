use std::{io::ErrorKind, path::Path};

pub fn read_fixture(relative_path: &str) -> Option<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative_path);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!("skipping missing fixture: {}", path.display());
            None
        }
        Err(error) => panic!("failed to read fixture {}: {error}", path.display()),
    }
}
