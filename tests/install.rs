#![cfg(unix)]

use std::{fs, path::Path, process::Command};

fn make_install_command(root: &Path, target: &str, source: &Path, install_path: &Path) -> Command {
    let mut command = Command::new("make");
    command
        .current_dir(root)
        .arg("-f")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Makefile"))
        .args([target, "SYSTEM_TOOLS=1", "INSTALL_DEPS="])
        .arg(format!("BIN_SRC={}", source.display()))
        .env("INSTALL_PATH", install_path);
    command
}

#[test]
fn install_uses_the_literal_directory_without_shell_expansion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let source = root.join("source");
    fs::write(&source, "test-binary").unwrap();
    let install_path = root.join("bin`touch unexpected`");
    let output = make_install_command(root, "install", &source, &install_path)
        .output()
        .unwrap();
    assert!(!root.join("unexpected").exists());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(install_path.join("ai-config-tui")).unwrap(),
        b"test-binary"
    );
}

#[test]
fn uninstall_uses_the_literal_directory_without_shell_expansion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let install_path = root.join("bin`touch unexpected`");
    fs::create_dir(&install_path).unwrap();
    let binary = install_path.join("ai-config-tui");
    fs::write(&binary, "test-binary").unwrap();
    let output = make_install_command(root, "uninstall", &binary, &install_path)
        .output()
        .unwrap();
    assert!(!root.join("unexpected").exists());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!binary.exists());
}
