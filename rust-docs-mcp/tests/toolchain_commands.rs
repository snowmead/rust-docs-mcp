//! Exercise direct compiler binaries without rustup on PATH.

#[cfg(unix)]
#[test]
fn native_toolchain_generates_docs_and_dependency_metadata() -> anyhow::Result<()> {
    use std::{
        env,
        fs,
        os::unix::fs::symlink,
        path::PathBuf,
        process::Command, //
    };

    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    anyhow::ensure!(sysroot.status.success());
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim());
    let bin = tempfile::tempdir()?;
    for program in ["cargo", "rustc", "rustdoc"] {
        let direct = sysroot.join("bin").join(program);
        let direct = if direct.exists() {
            direct
        } else {
            env::split_paths(&env::var_os("PATH").unwrap_or_default())
                .map(|path| path.join(program))
                .find(|path| path.is_file())
                .ok_or_else(|| anyhow::anyhow!("Missing {program}"))?
                .canonicalize()?
        };
        symlink(direct, bin.path().join(program))?;
    }
    // Keep the linker and platform tools, but remove every rustup directory.
    let mut paths = vec![bin.path().to_owned()];
    paths.extend(
        env::split_paths(&env::var_os("PATH").unwrap_or_default())
            .filter(|path| !path.join("rustup").exists()),
    );
    let path = env::join_paths(paths)?;
    let source = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::create_dir(source.path().join("src"))?;
    fs::write(
        source.path().join("Cargo.toml"),
        "[package]\nname = \"native-toolchain-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        source.path().join("src/lib.rs"),
        "/// A fixture.\npub fn answer() -> u32 { 42 }\n",
    )?;
    let binary = env!("CARGO_BIN_EXE_rust-docs-mcp");
    let params = serde_json::json!({
        "crate_name": "native-toolchain-fixture", "source_type": "local",
        "path": source.path(), "features": []
    });
    let output = Command::new(binary)
        .env("PATH", &path)
        .env_remove("RUST_DOCS_MCP_TOOLCHAIN")
        .arg("--cache-dir")
        .arg(cache.path())
        .args(["call", "cache-crate", "--params", &params.to_string()])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let params = serde_json::json!({"crate_name": "native-toolchain-fixture", "version": "0.1.0", "features": []});
    let output = Command::new(binary)
        .env("PATH", &path)
        .env_remove("RUST_DOCS_MCP_TOOLCHAIN")
        .arg("--cache-dir")
        .arg(cache.path())
        .args(["call", "get-dependencies", "--params", &params.to_string()])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // Check the diagnostic reports the actual selected command source.
    let doctor = Command::new(binary)
        .env("PATH", path)
        .env_remove("RUST_DOCS_MCP_TOOLCHAIN")
        .args(["doctor"])
        .output()?;
    let stdout = String::from_utf8(doctor.stdout)?;
    assert!(stdout.contains("selected: PATH"), "{stdout}");
    Ok(())
}
