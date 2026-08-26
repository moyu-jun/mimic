use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const INSTALLER_SHA256: &str = "e137863a79da797f08e7a137280ff2a123809044a888fd75ce9c973198915abe";
const HELPER_RELATIVE: &str = "driver/mimic-elevated-helper.exe";
const MAX_HELPER_BYTES: u64 = 20 * 1024 * 1024;
const REQUIRED_RESOURCES: &[&str] = &[
    "interception.dll",
    "driver/install-interception.exe",
    "audio/按键开启.wav",
    "audio/按键关闭.wav",
];

fn main() {
    println!("cargo:rerun-if-changed=../extra");
    println!("cargo:rerun-if-changed=../extra/{HELPER_RELATIVE}");
    tauri_build::build();
    copy_and_verify_resources();
}

fn copy_and_verify_resources() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"));
    let profile = env::var("PROFILE").expect("PROFILE missing");
    let source = manifest_dir
        .parent()
        .expect("src-tauri has no parent")
        .join("extra");
    let destination = manifest_dir.join("target").join(&profile);

    for relative in REQUIRED_RESOURCES {
        require_nonempty_file(&source.join(relative), relative);
    }
    verify_installer_hash(&source.join("driver/install-interception.exe"));

    let helper = source.join(HELPER_RELATIVE);
    if helper.exists() {
        let metadata = fs::metadata(&helper).expect("failed to inspect elevated helper");
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_HELPER_BYTES {
            panic!("elevated helper is empty, oversized, or not a regular file");
        }
        println!(
            "cargo:rustc-env=MIMIC_HELPER_SHA256={}",
            sha256_file(&helper)
        );
    } else if profile == "release" {
        panic!(
            "release build requires {HELPER_RELATIVE}; run scripts/build-release.ps1 so the helper is built before the application"
        );
    }

    fs::create_dir_all(&destination).expect("failed to create target profile directory");
    copy_dir_all(&source, &destination).expect("failed to copy required runtime resources");

    for relative in REQUIRED_RESOURCES {
        require_nonempty_file(&destination.join(relative), relative);
    }
    if profile == "release" {
        require_nonempty_file(&destination.join(HELPER_RELATIVE), HELPER_RELATIVE);
    }
}

fn require_nonempty_file(path: &Path, relative: &str) {
    let metadata = fs::metadata(path)
        .unwrap_or_else(|error| panic!("required resource {relative} missing: {error}"));
    if !metadata.is_file() || metadata.len() == 0 {
        panic!("required resource {relative} is empty or not a file");
    }
}

fn verify_installer_hash(path: &Path) {
    assert_eq!(
        sha256_file(path),
        INSTALLER_SHA256,
        "driver installer SHA-256 mismatch"
    );
}

fn sha256_file(path: &Path) -> String {
    let mut file = fs::File::open(path).expect("failed to open resource for hashing");
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).expect("failed to hash resource");
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("{:x}", hasher.finalize())
}

fn copy_dir_all(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}
