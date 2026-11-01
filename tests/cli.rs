use std::process::Command;

#[test]
fn cli_registers_and_removes_profiles_without_touching_their_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let registry = directory.path().join("registry/profiles.toml");
    let profile = directory.path().join("profile");
    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ai-config-tui"))
            .env("HOME", directory.path())
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .env("XDG_STATE_HOME", directory.path().join("state"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .args(["--registry", registry.to_str().unwrap()])
            .args(arguments)
            .output()
            .unwrap()
    };
    let added = run(&[
        "profile",
        "add",
        "--name",
        "Work",
        "--provider",
        "codex",
        "--directory",
        profile.to_str().unwrap(),
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(!profile.exists());
    let listed = run(&["profile", "list"]);
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Work\tCodex"));
    assert!(
        !run(&[
            "profile",
            "add",
            "--name",
            "Work",
            "--provider",
            "codex",
            "--directory",
            profile.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(run(&["profile", "remove", "Work"]).status.success());
    assert!(!String::from_utf8_lossy(&run(&["profile", "list"]).stdout).contains("Work\tCodex"));
    assert!(!profile.exists());
}

#[test]
fn noninteractive_start_explains_that_a_terminal_is_required() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai-config-tui"))
        .env("HOME", directory.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .arg("--registry")
        .arg(directory.path().join("profiles.toml"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("対話端末"));
    assert!(!directory.path().join("profiles.toml").exists());
}
