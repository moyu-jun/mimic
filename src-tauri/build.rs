use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // 标准的 tauri build
    tauri_build::build();

    // 复制 extra/ 目录内容到 target/{debug,release}/
    // 目的：让 interception.dll 在 exe 同级，driver/install-interception.exe 也到位
    copy_extra_to_target();
}

fn copy_extra_to_target() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap(); // "debug" or "release"

    let extra_dir = Path::new(&manifest_dir).parent().unwrap().join("extra");
    let target_dir = Path::new(&manifest_dir).join("target").join(&profile);

    if !extra_dir.exists() {
        println!("cargo:warning=extra/ directory not found, skipping copy");
        return;
    }

    if !target_dir.exists() {
        println!(
            "cargo:warning=target/{} directory not found, skipping copy",
            profile
        );
        return;
    }

    // 递归复制 extra/ 的所有内容到 target/{profile}/
    if let Err(e) = copy_dir_all(&extra_dir, &target_dir) {
        println!(
            "cargo:warning=Failed to copy extra/ to target/{}: {}",
            profile, e
        );
    } else {
        println!("cargo:warning=Copied extra/ to target/{}", profile);
    }

    // 触发重新构建的条件：extra/ 目录内容变化
    println!("cargo:rerun-if-changed=../extra");
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_all(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}
