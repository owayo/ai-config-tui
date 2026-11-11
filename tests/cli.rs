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
        run(&[
            "profile",
            "add",
            "--name",
            "Home",
            "--provider",
            "claude",
            "--directory",
            directory.path().join("claude-profile").to_str().unwrap()
        ])
        .status
        .success()
    );
    let grouped = run(&["profile", "list"]);
    let grouped = String::from_utf8_lossy(&grouped.stdout);
    assert_eq!(
        grouped
            .lines()
            .map(|line| line.split('\t').nth(1).unwrap())
            .collect::<Vec<_>>(),
        vec!["Claude Code", "Claude Code", "Codex", "Codex"]
    );
    assert!(
        run(&["profile", "set-provider", "Work", "--provider", "claude"])
            .status
            .success()
    );
    assert!(
        String::from_utf8_lossy(&run(&["profile", "list"]).stdout).contains("Work\tClaude Code")
    );
    assert!(
        run(&["profile", "set-provider", "Work", "--provider", "codex"])
            .status
            .success()
    );
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

fn isolated_cli(directory: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-config-tui"));
    command
        .env("HOME", directory)
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("XDG_STATE_HOME", directory.join("state"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .arg("--registry")
        .arg(directory.join("profiles.toml"));
    command
}

#[cfg(unix)]
#[test]
fn environment_symlink_aliases_do_not_persist_duplicate_profiles() {
    use std::{fs, os::unix::fs::symlink};

    for (variable, provider) in [("CLAUDE_CONFIG_DIR", "claude"), ("CODEX_HOME", "codex")] {
        for registered in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path();
            let profile = root.join(if registered {
                "work".to_owned()
            } else {
                format!(".{provider}")
            });
            fs::create_dir(&profile).unwrap();
            let alias = root.join("alias");
            symlink(&profile, &alias).unwrap();
            if registered {
                let added = isolated_cli(root)
                    .args([
                        "profile",
                        "add",
                        "--name",
                        "Work",
                        "--provider",
                        provider,
                        "--directory",
                    ])
                    .arg(&profile)
                    .output()
                    .unwrap();
                assert!(
                    added.status.success(),
                    "{}",
                    String::from_utf8_lossy(&added.stderr)
                );
            }

            let added = isolated_cli(root)
                .env(variable, &alias)
                .args([
                    "profile",
                    "add",
                    "--name",
                    "Home",
                    "--provider",
                    provider,
                    "--directory",
                ])
                .arg(root.join("home-profile"))
                .output()
                .unwrap();
            assert!(
                added.status.success(),
                "{}",
                String::from_utf8_lossy(&added.stderr)
            );
            let listed = isolated_cli(root)
                .env(variable, &alias)
                .args(["profile", "list"])
                .output()
                .unwrap();
            assert!(
                listed.status.success(),
                "{}",
                String::from_utf8_lossy(&listed.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&listed.stdout).lines().count(),
                if registered { 4 } else { 3 }
            );
            assert!(!String::from_utf8_lossy(&listed.stdout).contains(variable));
        }
    }
}

#[test]
fn environment_profile_names_do_not_collide_with_registered_names() {
    for (variable, provider, label) in [
        ("CLAUDE_CONFIG_DIR", "claude", "Claude Code"),
        ("CODEX_HOME", "codex", "Codex"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let name = format!("{label} · {variable}");
        for existing in [&name, &format!("{name} (2)")] {
            let added = isolated_cli(root)
                .args([
                    "profile",
                    "add",
                    "--name",
                    existing,
                    "--provider",
                    provider,
                    "--directory",
                ])
                .arg(root.join(existing))
                .output()
                .unwrap();
            assert!(
                added.status.success(),
                "{}",
                String::from_utf8_lossy(&added.stderr)
            );
        }
        let override_directory = root.join("override-profile");
        let added = isolated_cli(root)
            .env(variable, &override_directory)
            .args([
                "profile",
                "add",
                "--name",
                "Home",
                "--provider",
                provider,
                "--directory",
            ])
            .arg(root.join("home-profile"))
            .output()
            .unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        let listed = isolated_cli(root)
            .env(variable, &override_directory)
            .args(["profile", "list"])
            .output()
            .unwrap();
        assert!(
            listed.status.success(),
            "{}",
            String::from_utf8_lossy(&listed.stderr)
        );
        let output = String::from_utf8_lossy(&listed.stdout);
        assert!(output.contains(&format!("{name} (3)\t")));
        assert_eq!(output.lines().count(), 6);
    }
}
