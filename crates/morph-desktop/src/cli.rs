//! Command-line arguments: `morph [--version] [--help] [URL]`.

pub const USAGE: &str = "usage: morph [URL]\n       morph --version\n\nData lives in $MORPH_HOME, else ~/.morph.";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Address or search to open in the first tab (`%u` from the desktop file).
    pub open: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Args),
    Print(String),
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut out = Args::default();
    for arg in args {
        match arg.as_str() {
            "--version" | "-V" => return Ok(Command::Print(format!("morph {}", env!("CARGO_PKG_VERSION")))),
            "--help" | "-h" => return Ok(Command::Print(USAGE.to_string())),
            flag if flag.starts_with('-') && flag.len() > 1 => return Err(format!("unknown option {flag}")),
            _ if out.open.is_some() => return Err("only one URL can be opened".into()),
            _ => out.open = Some(arg),
        }
    }
    Ok(Command::Run(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn version_and_help_print() {
        assert_eq!(parse_strs(&["--version"]), Ok(Command::Print(format!("morph {}", env!("CARGO_PKG_VERSION")))));
        assert!(matches!(parse_strs(&["-h"]), Ok(Command::Print(text)) if text.contains("MORPH_HOME")));
    }

    #[test]
    fn one_url_opens_and_unknown_flags_fail() {
        assert_eq!(parse_strs(&[]), Ok(Command::Run(Args::default())));
        assert_eq!(parse_strs(&["example.com"]), Ok(Command::Run(Args { open: Some("example.com".into()) })));
        assert!(parse_strs(&["--nope"]).is_err());
        assert!(parse_strs(&["a.test", "b.test"]).is_err());
    }
}
