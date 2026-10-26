use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use crate::client::older;
use crate::error::{Error, Result};

const REPO: &str = "vanducng/rexboard";
const FORMULA: &str = "vanducng/tap/rexboard";

/// How this binary was installed, from its resolved path.
#[derive(Debug, PartialEq)]
pub enum Method {
    /// `brew upgrade`; the formula's binary after it is `installed`.
    Brew { installed: PathBuf },
    /// install.sh into the directory this binary is in.
    Script { dir: PathBuf },
    /// Neither: how it was installed, and what upgrades it.
    Other { how: &'static str, run: String },
}

pub fn method(exe: &Path) -> Method {
    let text = exe.to_string_lossy();
    if let Some((prefix, _)) = text.split_once("/Cellar/rexboard/") {
        return Method::Brew {
            installed: Path::new(prefix).join("opt/rexboard/bin/rexboard"),
        };
    }
    let dir = exe.parent().unwrap_or(Path::new("/")).to_owned();
    if dir.ends_with("target/debug") || dir.ends_with("target/release") {
        return Method::Other {
            how: "a cargo build",
            run: "git pull && cargo build --release".to_owned(),
        };
    }
    if dir.ends_with(".cargo/bin") {
        return Method::Other {
            how: "installed with cargo install",
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
        });
    }
    let (installed, how) = match method(exe) {
        Method::Other { how, run } => {
            return Err(Error::upgrade(
                format!(
                    "this rexboard is {how} ({}), so upgrade it the same way",
                    exe.display()
                ),
                &[&run],
            ));
        }
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
    if !sys.writable(dir) {
        return Err(Error::upgrade(
            format!("cannot write {}, where this rexboard is", dir.display()),
            &[&format!("brew install {FORMULA}")],
        ));
    }
    let tag = format!("v{latest}");
    let script = std::env::temp_dir().join(format!("rexboard-install-{}.sh", std::process::id()));
    let script = script.to_string_lossy();
    let url = format!("https://raw.githubusercontent.com/{REPO}/{tag}/install.sh");
    sys.run("curl", &["-fsSL", "-o", &script, &url], &[])?;
    let ran = sys.run(
        "sh",
        &[&script],
        &[
            ("REXBOARD_INSTALL_DIR", &dir.to_string_lossy()),
            ("REXBOARD_VERSION", &tag),
        ],
    );
    let _ = std::fs::remove_file(&*script);
    ran
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
        Outcome::Checked { current, latest } => {
            let newer = older(&current, &latest);
            let line = if newer {
                format!("rexboard {current}; {latest} is out: rexboard upgrade")
            } else {
                format!("rexboard {current} is the latest")
            };
            (
                line,
                json!({ "ok": true, "current": current, "latest": latest, "newer": newer }),
            )
        }
        Outcome::Latest { current } => (
            format!("already the latest ({current})"),
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
        ran: Vec<String>,
        stops: usize,
    }

    fn fake(latest: &'static str, after: Option<&'static str>) -> Fake {
        Fake {
            latest,
            after,
            writable: true,
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
        for (exe, how) in [
            ("/work/rexboard/target/release/rexboard", "a cargo build"),
            ("/work/rexboard/target/debug/rexboard", "a cargo build"),
            (
                "/Users/u/.cargo/bin/rexboard",
                "installed with cargo install",
            ),
        ] {
            assert!(
                matches!(method(path(exe)), Method::Other { how: h, .. } if h == how),
                "{exe}"
            );
        }
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
                    current: current.into()
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
        let [curl, sh] = sys.ran.as_slice() else {
            panic!("{:?}", sys.ran);
        };
        let script = curl
            .strip_prefix("curl -fsSL -o ")
            .and_then(|rest| {
                rest.strip_suffix(
                    " https://raw.githubusercontent.com/vanducng/rexboard/v0.4.0/install.sh",
                )
            })
            .unwrap_or_else(|| panic!("{curl}"));
        assert_eq!(
            sh,
            &format!(
                "REXBOARD_INSTALL_DIR=/Users/u/.local/bin REXBOARD_VERSION=v0.4.0 sh {script}"
            )
        );
        assert!(!sh.contains("sudo"));
        assert_eq!(sys.stops, 1);
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
                "after brew, /opt/homebrew/opt/rexboard/bin/rexboard reports 0.3.0, not 0.4.0"
            ),
            "{err}"
        );
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
