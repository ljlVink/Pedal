# pedal-archive

Pure-Rust archive crate for Pedal: ZIP/JAR reading with progress reporting,
in-memory modification tracking (edit / rename / delete), snapshot export, and
virtual archive assembly for non-ZIP sources (the ArkTS workspace builds a
`JarArchive` from decompiled entries). No UI dependencies; every public
operation returns `Result<T, ArchiveError>`.


## Modules

| Module | Role |
|---|---|
| `jar` | `JarArchive` (open with progress, `from_entries` virtual archives, modified-entry tracking, `write_jar` export), `LoadProgress` shared atomics |
| `error` | `ArchiveError` (`Io` / `Parse`) |

## Usage sketch

```rust
use pedal_archive::jar::{JarArchive, LoadProgress};
use std::sync::Arc;

let progress = Arc::new(LoadProgress::new());
let p = progress.clone();
let task = std::thread::spawn(move || JarArchive::open_with_progress(path, &p));
let jar = task.join().unwrap()?;
let bytes = jar.get("META-INF/MANIFEST.MF").unwrap();
```

`LoadProgress` also implements `Default`, so callers may use
`Arc::new(LoadProgress::default())` when they prefer the standard constructor.

## Dependencies

`zip` (read/write), `sha2` (archive hash), `tabookit`.
