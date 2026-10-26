//! Checks that keep the repo on the direction its docs set: every JSON sample in them parses,
//! every command and flag is in docs/cli.md, the public files name no private path or host, and
//! the decisions index lists every record.

use std::path::{Path, PathBuf};

use clap::CommandFactory;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file under `dir`, recursively, in a stable order.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// The Markdown a reader sees: README, CONTRIBUTING and docs/.
fn markdown() -> Vec<PathBuf> {
    let mut docs: Vec<PathBuf> = files(&root().join("docs"))
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    docs.extend(["README.md", "CONTRIBUTING.md"].map(|name| root().join(name)));
    docs
}

#[test]
fn every_json_sample_in_the_docs_parses() {
    let mut seen = 0;
    for path in markdown() {
        let text = read(&path);
        // A fence may name what it holds after the language: ```json request
        for fenced in text.split("```json").skip(1) {
            let body = fenced.split_once('\n').map_or("", |(_, body)| body);
            let sample = body.split("```").next().unwrap();
            serde_json::from_str::<serde_json::Value>(sample)
                .unwrap_or_else(|err| panic!("{}: {err}: {sample}", path.display()));
            seen += 1;
        }
    }
    assert!(seen >= 10, "only {seen} samples: did the fence change?");
}

#[test]
fn docs_cli_md_names_every_command_and_flag() {
    let doc = read(&root().join("docs/cli.md"));
    let mut missing = Vec::new();
    let mut walk = vec![(String::from("rexboard"), rexboard::Cli::command())];
    while let Some((path, command)) = walk.pop() {
        if path != "rexboard" && !doc.contains(&path) {
            missing.push(path.clone());
        }
        for arg in command.get_arguments() {
            if let Some(long) = arg.get_long()
                && !matches!(long, "help" | "version")
                && !doc.contains(&format!("--{long}"))
            {
                missing.push(format!("{path} --{long}"));
            }
        }
        for sub in command.get_subcommands() {
            if sub.get_name() != "help" {
                walk.push((format!("{path} {}", sub.get_name()), sub.clone()));
            }
        }
    }
    assert!(missing.is_empty(), "not in docs/cli.md: {missing:?}");
}

/// What a public file must not hold: a home directory, a tailnet host name, or a private key.
/// Generic on purpose; a name that is private to one machine does not belong in this list either.
const PRIVATE: &[&str] = &[
    "/Users/",
    "/home/",
    "C:\\Users\\",
    ".ts.net",
    "BEGIN OPENSSH PRIVATE KEY",
    "BEGIN RSA PRIVATE KEY",
];

#[test]
fn public_files_name_no_private_path_or_host() {
    let this = root().join("tests/direction.rs");
    let mut paths: Vec<PathBuf> = ["docs", "tests", "examples"]
        .into_iter()
        .flat_map(|dir| files(&root().join(dir)))
        .filter(|path| *path != this)
        .collect();
    paths.extend(["README.md", "CONTRIBUTING.md"].map(|name| root().join(name)));
    let mut found = Vec::new();
    for path in paths {
        // Images and other binary files carry no text to check.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for pattern in PRIVATE {
                if line.contains(pattern) {
                    let at = path.strip_prefix(root()).unwrap().display();
                    found.push(format!("{at}:{}: {pattern}", n + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_decisions_index_lists_every_record_and_each_has_a_status() {
    let dir = root().join("docs/decisions");
    let index = read(&dir.join("README.md"));
    let records: Vec<PathBuf> = files(&dir)
        .into_iter()
        .filter(|path| path.file_name().unwrap() != "README.md")
        .collect();
    assert!(records.len() >= 12, "{records:?}");
    for path in &records {
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.len() > 5
                && name[..4].bytes().all(|b| b.is_ascii_digit())
                && name.ends_with(".md"),
            "{name}: NNNN-name.md"
        );
        assert!(
            index.contains(&format!("({name})")),
            "the index does not link {name}"
        );
        assert!(
            read(path)
                .lines()
                .any(|line| line.starts_with("**Status:** ")),
            "{name} has no **Status:** line"
        );
    }
    for link in index.split("](").skip(1) {
        let target = link.split(')').next().unwrap();
        if target.ends_with(".md") {
            assert!(
                dir.join(target).exists(),
                "the index links {target}, which is gone"
            );
        }
    }
}
