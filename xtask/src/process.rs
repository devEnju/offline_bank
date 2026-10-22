//! Other programs: cargo, git, ctrtool, gh. Each runs in the project folder;
//! what one needs in its environment is set on that one child only.

use crate::{root, Result};
use std::process::{Command, Stdio};

/// A command in the project folder.
pub fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.current_dir(root());
    command
}

/// Cargo of `toolchain` ("stable", or the nightly that builds the ARM code).
pub fn cargo(toolchain: &str) -> Command {
    let mut command = command("cargo");
    command.arg(format!("+{toolchain}"));
    command
}

/// Runs `command` with its output shown; fails, naming `what`, unless it
/// succeeds.
pub fn run(what: &str, command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|e| format!("{what}: cannot start {:?}: {e}", command.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed: {status}"))
    }
}

/// Runs `command` and returns what it writes to stdout; its errors are shown.
pub fn output(what: &str, command: &mut Command) -> Result<String> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{what}: cannot start {:?}: {e}", command.get_program()))?;
    if !output.status.success() {
        return Err(format!("{what} failed: {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{what}: output is not UTF-8"))
}

/// Git in the project folder; its output without the final line break.
pub fn git(args: &[&str]) -> Result<String> {
    let text = output(
        &format!("git {}", args.join(" ")),
        command("git").args(args),
    )?;
    Ok(text.trim_end().to_owned())
}

/// The nightly that builds the ARM code: the one `rust-toolchain.toml` pins.
pub fn pinned_toolchain() -> Result<String> {
    let path = root().join("rust-toolchain.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    text.lines()
        .find_map(|line| {
            let value = line
                .trim()
                .strip_prefix("channel")?
                .trim_start()
                .strip_prefix('=')?;
            Some(
                value
                    .trim()
                    .strip_prefix('"')?
                    .strip_suffix('"')?
                    .to_owned(),
            )
        })
        .ok_or_else(|| "rust-toolchain.toml names no channel.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_pinned_toolchain_is_a_nightly() {
        assert!(super::pinned_toolchain().unwrap().starts_with("nightly-"));
    }
}
