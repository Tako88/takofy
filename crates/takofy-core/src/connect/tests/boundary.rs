use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("a crate sources directory");
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn only_one_module_speaks_the_device_wire() {
    let put_state = concat!("put_connect_", "state");
    let delete_state = concat!("delete_connect_", "state");
    let state_path = concat!("connect-state/v1/", "devices");
    let transfer = concat!(".", "transfer(");
    let transfer_path = concat!("connect/", "transfer");
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let skipped = crates.join("takofy-core").join("src").join("watch.rs");

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("the crates directory") {
        let crate_dir = entry.expect("a crate directory").path();
        for root in ["src", "tests", "examples", "benches"] {
            let source = crate_dir.join(root);
            if source.is_dir() {
                rust_files(&source, &mut files);
            }
        }
    }

    let mut scanned = 0;
    for path in files {
        if path == skipped {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a readable source file");
        scanned += 1;
        for (pattern, reason) in [
            (put_state, "may send its own device state"),
            (delete_state, "may delete the account's device"),
            (state_path, "may write a device state directly"),
            (transfer, "may move playback itself"),
            (transfer_path, "may move playback itself"),
        ] {
            assert!(!source.contains(pattern), "{} {reason}", path.display());
        }
    }

    assert!(
        scanned > 20,
        "scanned only {scanned} files, so the walk missed the crates"
    );
}
