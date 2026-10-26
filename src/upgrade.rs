use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use crate::client::older;
use crate::error::{Error, Result};

const REPO: &str = "vanducng/rexboard";
const FORMULA: &str = "vanducng/tap/rexboard";
const INSTALL_SH: &str = "https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh";
/// Fetches install.sh (`$1`) into a new private directory (mktemp -d: a random name, mode 0700),
/// so no other user can swap the script, runs it, and removes the directory.
const FETCH_AND_RUN: &str = r#"d=$(mktemp -d) && trap 'rm -rf "$d"' EXIT && curl -fsSL -o "$d/install.sh" "$1" && sh "$d/install.sh""#;

/// How this binary was installed, from its resolved path.
#[derive(Debug, PartialEq)]
pub enum Method {
    /// `brew upgrade`; the formula's binary after it is `installed`.
    Brew { installed: PathBuf },
    /// install.sh into the directory this binary is in.
    Script { dir: PathBuf },
    /// Neither: why upgrade leaves it alone, and what upgrades it.
    Other { why: String, run: String },
}

pub fn method(exe: &Path) -> Method {
    let dir = exe.parent().unwrap_or(Path::new("/")).to_owned();
    let name = exe.file_name().unwrap_or_default().to_string_lossy();
    if name != "rexboard" {
        return Method::Other {
            why: format!(
                "this rexboard is named {name} ({}), and only a binary named rexboard is replaced",
                exe.display()
            ),
            run: format!(
                "curl -fsSL {INSTALL_SH} | REXBOARD_INSTALL_DIR={} sh",
                dir.display()
            ),
        };
    }
    if let Some((prefix, _)) = exe.to_string_lossy().split_once("/Cellar/rexboard/") {
        return Method::Brew {
            installed: Path::new(prefix).join("opt/rexboard/bin/rexboard"),
        };
    }
    let parts: Vec<_> = dir.iter().collect();
    let cargo_build = parts.iter().position(|p| *p == "target").is_some_and(|at| {
        parts[at + 1..]
            .iter()
            .any(|p| *p == "debug" || *p == "release")
    });
    if cargo_build {
        return Method::Other {
            why: format!(
                "this rexboard is a cargo build ({}); upgrade it the same way",
                exe.display()
            ),
            run: "git pull && cargo build --release".to_owned(),
        };
    }
    if dir.ends_with(".cargo/bin") {
        return Method::Other {
            why: format!(
                "this rexboard was installed with cargo install ({}); upgrade it the same way",
                exe.display()
            ),
            run: format!("cargo install --locked --git https://github.com/{REPO}"),
        };
    }
    Method::Script { dir }
}

/// What upgrading touches, so tests never run brew, curl or install.sh.
pub trait System {
    /// The latest release's version, without the `v`.
    fn latest(&mut self) -> Result<String>;
    /// Runs a command with its output on stderr, failing unless it exits 0.
    fn run(&mut self, program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<()>;
    fn writable(&self, dir: &Path) -> bool;
    /// What `exe --version` says, without the `rexboard `.
    fn version_of(&mut self, exe: &Path) -> Option<String>;
    /// Stops serve, so the next command starts the new one; false when none ran.
    fn stop_serve(&mut self) -> Result<bool>;
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Checked {
        current: String,
        latest: String,
    },
    Latest {
        current: String,
        latest: String,
    },
    Upgraded {
        from: String,
        to: String,
        method: &'static str,
        serve_stopped: bool,
    },
}

pub fn upgrade(sys: &mut impl System, exe: &Path, current: &str, check: bool) -> Result<Outcome> {
    let latest = sys.latest()?;
    if check {
        return Ok(Outcome::Checked {
            current: current.to_owned(),
            latest,
        });
    }
    if !older(current, &latest) {
        return Ok(Outcome::Latest {
            current: current.to_owned(),
            latest,
        });
    }
    let (installed, how) = match method(exe) {
        Method::Other { why, run } => return Err(Error::upgrade(why, &[&run])),
        Method::Brew { installed } => {
            sys.run("brew", &["upgrade", FORMULA], &[])?;
            (installed, "brew")
        }
        Method::Script { dir } => {
            install_sh(sys, &dir, &latest)?;
            (dir.join("rexboard"), "install.sh")
        }
    };
    let to = sys.version_of(&installed).unwrap_or_default();
    if !older(current, &to) {
        if how == "brew" && to == current {
            return Err(Error::upgrade(
                format!(
                    "after brew upgrade, rexboard is still {current}: the tap may not have \
                     {latest} yet; it follows a release by a few minutes"
                ),
                &["brew update", "rexboard upgrade"],
            ));
        }
        let said = if to.is_empty() { "no version" } else { &to };
        return Err(Error::upgrade(
            format!(
                "after {how}, {} reports {said}, not {latest}",
                installed.display()
            ),
            &[&format!("{} --version", installed.display())],
        ));
    }
    let serve_stopped = sys.stop_serve().unwrap_or_else(|err| {
        eprintln!("warning: could not stop serve ({err}); the next command replaces it");
        false
    });
    Ok(Outcome::Upgraded {
        from: current.to_owned(),
        to,
        method: how,
        serve_stopped,
    })
}

/// Runs the release's own install.sh into `dir`, the directory this binary is in.
fn install_sh(sys: &mut impl System, dir: &Path, latest: &str) -> Result<()> {
    let shown = dir.display();
    if !sys.writable(dir) {
        return Err(Error::upgrade(
            format!("cannot write {shown}, where this rexboard is, and rexboard never runs sudo"),
            &[
                &format!("curl -fsSL {INSTALL_SH} | sudo REXBOARD_INSTALL_DIR={shown} sh"),
                &format!("sudo rm {shown}/rexboard && brew install {FORMULA}"),
            ],
        ));
    }
    let tag = format!("v{latest}");
    let url = format!("https://raw.githubusercontent.com/{REPO}/{tag}/install.sh");
    sys.run(
        "sh",
        &["-c", FETCH_AND_RUN, "sh", &url],
        &[
            ("REXBOARD_INSTALL_DIR", &shown.to_string()),
            ("REXBOARD_VERSION", &tag),
        ],
    )
    .map_err(|_| {
        Error::upgrade(
            format!(
                "install.sh did not install {latest}: its archives may still be uploading; \
                 retry in a few minutes"
            ),
            &["rexboard upgrade"],
        )
    })
}

/// What `--check`, and an upgrade with nothing to do, say.
fn standing(current: &str, latest: &str) -> String {
    if older(current, latest) {
        format!("{latest} is out (you have {current}): run rexboard upgrade")
    } else if current == latest {
        format!("rexboard {current} is the latest")
    } else {
        format!("rexboard {current} (the latest release is {latest})")
    }
}

/// The version in the URL `releases/latest` redirects to, as in `.../releases/tag/v0.4.0`.
pub fn tag_version(url: &str) -> Option<String> {
    let version = url.trim().rsplit('/').next()?.strip_prefix('v')?;
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_owned())
}

struct Real;

impl System for Real {
    fn latest(&mut self) -> Result<String> {
        let url = format!("https://github.com/{REPO}/releases/latest");
        let out = Command::new("curl")
            .args(["-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}", &url])
            .output()
            .map_err(|err| Error::upgrade(format!("could not run curl: {err}"), &[]))?;
        let effective = String::from_utf8_lossy(&out.stdout);
        tag_version(&effective)
            .filter(|_| out.status.success())
            .ok_or_else(|| {
                Error::upgrade(
                    format!("could not find the latest release at {url}"),
                    &[&url],
                )
            })
    }

    fn run(&mut self, program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<()> {
        let stdout = std::io::stderr().as_fd().try_clone_to_owned()?;
        let status = Command::new(program)
            .args(args)
            .envs(envs.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .status()
            .map_err(|err| Error::upgrade(format!("could not run {program}: {err}"), &[]))?;
        if status.success() {
            return Ok(());
        }
        Err(Error::upgrade(
            format!("{program} {} failed ({status})", args.join(" ")),
            &[],
        ))
    }

    fn writable(&self, dir: &Path) -> bool {
        let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else {
            return false;
        };
        unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
    }

    fn version_of(&mut self, exe: &Path) -> Option<String> {
        let out = Command::new(exe).arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        Some(text.trim().strip_prefix("rexboard ")?.to_owned())
    }

    fn stop_serve(&mut self) -> Result<bool> {
        crate::client::stop(&crate::state::Paths::find()?)
    }
}

pub fn run(check: bool, as_json: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let outcome = upgrade(&mut Real, &exe, &crate::serve::version(), check)?;
    let (line, body) = match outcome {
        Outcome::Checked { current, latest } => (
            standing(&current, &latest),
            json!({ "ok": true, "current": current, "latest": latest, "newer": older(&current, &latest) }),
        ),
        Outcome::Latest { current, latest } => (
            standing(&current, &latest),
            json!({ "ok": true, "from": current, "to": current, "upgraded": false }),
        ),
        Outcome::Upgraded {
            from,
            to,
            method,
            serve_stopped,
        } => {
            let mut line = format!("upgraded rexboard {from} -> {to}");
            if serve_stopped {
                line.push_str("; serve stopped, the next command starts the new one");
            }
            line.push_str("; reopen an open board");
            (
                line,
                json!({ "ok": true, "from": from, "to": to, "upgraded": true, "method": method, "serve_stopped": serve_stopped }),
            )
        }
    };
    if as_json {
        println!("{body}");
    } else {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    struct Fake {
        latest: &'static str,
        after: Option<&'static str>,
        writable: bool,
        fails: bool,
        ran: Vec<String>,
        stops: usize,
    }

    fn fake(latest: &'static str, after: Option<&'static str>) -> Fake {
        Fake {
            latest,
            after,
            writable: true,
            fails: false,
            ran: Vec::new(),
            stops: 0,
        }
    }

    impl System for Fake {
        fn latest(&mut self) -> Result<String> {
            Ok(self.latest.to_owned())
        }

        fn run(&mut self, program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<()> {
            let envs = envs.iter().map(|(k, v)| format!("{k}={v} "));
            self.ran.push(format!(
                "{}{program} {}",
                envs.collect::<String>(),
                args.join(" ")
            ));
            if self.fails {
                return Err(Error::upgrade(format!("{program} failed"), &[]));
            }
            Ok(())
        }

        fn writable(&self, _: &Path) -> bool {
            self.writable
        }

        fn version_of(&mut self, _: &Path) -> Option<String> {
            self.after.map(str::to_owned)
        }

        fn stop_serve(&mut self) -> Result<bool> {
            self.stops += 1;
            Ok(true)
        }
    }

    #[test]
    fn the_install_method_follows_where_the_binary_is() {
        let path = Path::new;
        assert_eq!(
            method(path("/opt/homebrew/Cellar/rexboard/0.3.0/bin/rexboard")),
            Method::Brew {
                installed: path("/opt/homebrew/opt/rexboard/bin/rexboard").to_owned()
            }
        );
        assert_eq!(
            method(path(
                "/home/linuxbrew/.linuxbrew/Cellar/rexboard/0.3.0/bin/rexboard"
            )),
            Method::Brew {
                installed: path("/home/linuxbrew/.linuxbrew/opt/rexboard/bin/rexboard").to_owned()
            }
        );
        assert_eq!(
            method(path("/Users/u/.local/bin/rexboard")),
            Method::Script {
                dir: path("/Users/u/.local/bin").to_owned()
            }
        );
        for (exe, why) in [
            ("/work/rexboard/target/release/rexboard", "a cargo build"),
            ("/work/rexboard/target/debug/rexboard", "a cargo build"),
            (
                "/work/rexboard/target/aarch64-apple-darwin/release/rexboard",
                "a cargo build",
            ),
            (
                "/Users/u/.cargo/bin/rexboard",
                "installed with cargo install",
            ),
            ("/Users/u/.local/bin/rexboard-0.3", "named rexboard-0.3"),
            (
                "/opt/homebrew/Cellar/rexboard/0.3.0/bin/rb",
                "only a binary named rexboard",
            ),
        ] {
            assert!(
                matches!(method(path(exe)), Method::Other { why: w, .. } if w.contains(why)),
                "{exe}: {:?}",
                method(path(exe))
            );
        }
        assert!(
            matches!(
                method(path("/tmp/build/release/rexboard")),
                Method::Script { .. }
            ),
            "a release dir outside a target dir is an ordinary install"
        );
    }

    #[test]
    fn the_latest_version_comes_from_the_release_tag_url() {
        let url = "https://github.com/vanducng/rexboard/releases/tag/v0.4.0\n";
        assert_eq!(tag_version(url).as_deref(), Some("0.4.0"));
        assert_eq!(
            tag_version("https://github.com/vanducng/rexboard/releases"),
            None,
            "no release yet"
        );
        assert_eq!(tag_version("https://github.com/x/releases/tag/vnext"), None);
    }

    #[test]
    fn check_only_reports() {
        let mut sys = fake("0.4.0", None);
        let exe = Path::new("/opt/homebrew/Cellar/rexboard/0.3.0/bin/rexboard");
        assert_eq!(
            upgrade(&mut sys, exe, "0.3.0", true).unwrap(),
            Outcome::Checked {
                current: "0.3.0".into(),
                latest: "0.4.0".into()
            }
        );
        assert!(sys.ran.is_empty());
        assert_eq!(sys.stops, 0);
    }

    #[test]
    fn the_latest_or_a_newer_build_is_left_alone() {
        let exe = Path::new("/Users/u/.local/bin/rexboard");
        for current in ["0.4.0", "0.5.0"] {
            let mut sys = fake("0.4.0", None);
            assert_eq!(
                upgrade(&mut sys, exe, current, false).unwrap(),
                Outcome::Latest {
                    current: current.into(),
                    latest: "0.4.0".into()
                }
            );
            assert!(sys.ran.is_empty(), "{current}");
            assert_eq!(sys.stops, 0, "{current}");
        }
    }

    #[test]
    fn a_homebrew_install_upgrades_with_brew_and_stops_serve_once() {
        let mut sys = fake("0.4.0", Some("0.4.0"));
        let exe = Path::new("/opt/homebrew/Cellar/rexboard/0.3.0/bin/rexboard");
        assert_eq!(
            upgrade(&mut sys, exe, "0.3.0", false).unwrap(),
            Outcome::Upgraded {
                from: "0.3.0".into(),
                to: "0.4.0".into(),
                method: "brew",
                serve_stopped: true
            }
        );
        assert_eq!(sys.ran, ["brew upgrade vanducng/tap/rexboard"]);
        assert_eq!(sys.stops, 1);
    }

    #[test]
    fn any_other_install_runs_install_sh_of_the_release_into_its_own_dir() {
        let mut sys = fake("0.4.0", Some("0.4.0"));
        let exe = Path::new("/Users/u/.local/bin/rexboard");
        let outcome = upgrade(&mut sys, exe, "0.3.0", false).unwrap();
        assert!(matches!(
            outcome,
            Outcome::Upgraded {
                method: "install.sh",
                ..
            }
        ));
        assert_eq!(
            sys.ran,
            [format!(
                "REXBOARD_INSTALL_DIR=/Users/u/.local/bin REXBOARD_VERSION=v0.4.0 sh -c {FETCH_AND_RUN} \
                 sh https://raw.githubusercontent.com/vanducng/rexboard/v0.4.0/install.sh"
            )]
        );
        assert!(
            FETCH_AND_RUN.contains("mktemp -d") && !FETCH_AND_RUN.contains("https://"),
            "a private dir, and the URL as an argument"
        );
        assert!(!sys.ran[0].contains("sudo"));
        assert_eq!(sys.stops, 1);
    }

    #[test]
    fn install_sh_runs_from_a_private_dir_that_is_removed_after() {
        let dir = tempfile::tempdir().unwrap();
        let (script, out) = (dir.path().join("install.sh"), dir.path().join("out"));
        std::fs::write(
            &script,
            r#"here=$(dirname "$0"); ls -ld "$here" | cut -c1-10 > "$OUT"; echo "$here $REXBOARD_INSTALL_DIR" >> "$OUT""#,
        )
        .unwrap();
        let url = format!("file://{}", script.display());
        let status = Command::new("sh")
            .args(["-c", FETCH_AND_RUN, "sh", &url])
            .env("OUT", &out)
            .env("REXBOARD_INSTALL_DIR", "/x/bin")
            .status()
            .unwrap();
        assert!(status.success());
        let text = std::fs::read_to_string(&out).unwrap();
        let (mode, rest) = text.split_once('\n').unwrap();
        let (here, target) = rest.trim().split_once(' ').unwrap();
        assert_eq!(mode, "drwx------", "only this user can reach the script");
        assert_eq!(target, "/x/bin");
        assert!(!Path::new(here).exists(), "{here} is removed after");
    }

    #[test]
    fn what_check_and_nothing_to_do_say() {
        assert_eq!(
            standing("0.3.0", "0.4.0"),
            "0.4.0 is out (you have 0.3.0): run rexboard upgrade"
        );
        assert_eq!(standing("0.4.0", "0.4.0"), "rexboard 0.4.0 is the latest");
        assert_eq!(
            standing("0.5.0", "0.4.0"),
            "rexboard 0.5.0 (the latest release is 0.4.0)"
        );
    }

    #[test]
    fn a_cargo_build_or_a_dir_it_cannot_write_says_what_to_run_and_runs_nothing() {
        let mut sys = fake("0.4.0", Some("0.4.0"));
        let err = upgrade(
            &mut sys,
            Path::new("/work/rexboard/target/release/rexboard"),
            "0.3.0",
            false,
        )
        .unwrap_err();
        assert_eq!(err.code(), "UPGRADE");
        assert!(err.to_string().contains("a cargo build"), "{err}");
        assert_eq!(err.next_steps(), ["git pull && cargo build --release"]);
        sys.writable = false;
        let err = upgrade(
            &mut sys,
            Path::new("/usr/local/bin/rexboard"),
            "0.3.0",
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("cannot write /usr/local/bin"),
            "{err}"
        );
        assert_eq!(
            err.next_steps(),
            [
                "curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh \
                 | sudo REXBOARD_INSTALL_DIR=/usr/local/bin sh",
                "sudo rm /usr/local/bin/rexboard && brew install vanducng/tap/rexboard",
            ],
            "the same installer into the same dir, with the user's own sudo"
        );
        assert!(sys.ran.is_empty());
        assert_eq!(sys.stops, 0);
    }

    #[test]
    fn an_upgrade_that_did_not_take_fails_and_keeps_serve() {
        let mut sys = fake("0.4.0", Some("0.3.0"));
        let exe = Path::new("/opt/homebrew/Cellar/rexboard/0.3.0/bin/rexboard");
        let err = upgrade(&mut sys, exe, "0.3.0", false).unwrap_err();
        assert!(
            err.to_string().contains(
                "rexboard is still 0.3.0: the tap may not have 0.4.0 yet; it follows a release by a few minutes"
            ),
            "{err}"
        );
        assert_eq!(err.next_steps(), ["brew update", "rexboard upgrade"]);
        assert_eq!(sys.stops, 0);
        let mut sys = fake("0.4.0", None);
        sys.fails = true;
        let err = upgrade(
            &mut sys,
            Path::new("/Users/u/.local/bin/rexboard"),
            "0.3.0",
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("its archives may still be uploading; retry in a few minutes"),
            "{err}"
        );
        assert_eq!(err.next_steps(), ["rexboard upgrade"]);
        assert_eq!(sys.stops, 0);
    }

    #[test]
    fn update_is_a_hidden_alias() {
        let cli = crate::Cli::try_parse_from(["rexboard", "update", "--check"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(crate::cli::Command::Upgrade { check: true })
        ));
    }
}
