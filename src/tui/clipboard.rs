//! `y` and `Y`: text onto the clipboard of the machine the board runs on. Rex Beta does not pass
//! OSC 52 from a pane to the Mac's clipboard (checked on a pane off screen), so the board runs the
//! platform's own tool instead.

use std::io::Write;
use std::process::{Command, Stdio};

/// The command that takes text on stdin for the clipboard: pbcopy on macOS, wl-copy under
/// Wayland, else xclip; None when there is none.
pub fn tool(
    os: &str,
    wayland: bool,
    has: impl Fn(&str) -> bool,
) -> Option<&'static [&'static str]> {
    match os {
        "macos" => Some(&["pbcopy"]),
        "linux" if wayland && has("wl-copy") => Some(&["wl-copy"]),
        "linux" if has("xclip") => Some(&["xclip", "-selection", "clipboard"]),
        _ => None,
    }
}

pub fn copy(text: &str) -> Result<(), String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let argv = tool(std::env::consts::OS, wayland, on_path).ok_or("no clipboard to copy to")?;
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = crate::rex::spawn(&mut cmd).map_err(|err| format!("{}: {err}", argv[0]))?;
    let wrote = child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(text.as_bytes());
    let status = child.wait().map_err(|err| format!("{}: {err}", argv[0]))?;
    match (wrote, status.success()) {
        (Ok(()), true) => Ok(()),
        (Err(err), _) => Err(format!("{}: {err}", argv[0])),
        (_, false) => Err(format!("{} failed ({status})", argv[0])),
    }
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .any(|dir| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_picks_the_clipboard_tool_by_platform() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        assert_eq!(tool("macos", false, none), Some(&["pbcopy"][..]));
        assert_eq!(tool("linux", true, all), Some(&["wl-copy"][..]));
        let xclip = Some(&["xclip", "-selection", "clipboard"][..]);
        assert_eq!(tool("linux", false, all), xclip, "no Wayland");
        assert_eq!(
            tool("linux", true, |name| name == "xclip"),
            xclip,
            "Wayland without wl-copy"
        );
        assert_eq!(tool("linux", true, none), None, "nothing installed");
        assert_eq!(tool("freebsd", false, all), None);
    }
}
