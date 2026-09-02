use std::{ffi::c_void, fmt::Display, path::PathBuf, str::FromStr};

use clap::{ArgGroup, Parser, Subcommand};
use windows::Win32::Foundation::HWND;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Emit machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Clone, Debug)]
pub enum Commands {
    /// Enumerate top level windows
    EnumWindows {
        /// Title search string
        #[arg(short, long)]
        title: Option<String>,
    },
    /// Enumerate displays
    EnumDisplays,
    /// Capture a window
    CaptureWindow(CaptureWindowArgs),
    /// Capture a display
    CaptureDisplay(CaptureDisplayArgs),
}

#[derive(clap::Args, Clone, Debug)]
#[command(group(
    ArgGroup::new("window_selector")
        .required(true)
        .multiple(false)
        .args(["title", "handle"])
))]
pub struct CaptureWindowArgs {
    /// Exact title match
    #[arg(short, long)]
    pub title: Option<String>,

    /// Window handle
    #[arg(long)]
    pub handle: Option<WindowHandle>,

    /// Output file
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(clap::Args, Clone, Debug)]
#[command(group(
    ArgGroup::new("display_selector")
        .required(true)
        .multiple(false)
        .args(["monitor", "primary"])
))]
pub struct CaptureDisplayArgs {
    /// Capture a monitor by number (starts at 1)
    #[arg(short, long)]
    pub monitor: Option<usize>,

    /// Capture the primary monitor
    #[arg(short, long)]
    pub primary: bool,

    /// Output file
    #[arg(short, long)]
    pub output: PathBuf,
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct WindowHandle(pub HWND);

unsafe impl Send for WindowHandle {}
unsafe impl Sync for WindowHandle {}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowHandleParseError(pub String);

impl FromStr for WindowHandle {
    type Err = WindowHandleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pointer = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            usize::from_str_radix(hex, 16).map(|value| value as *mut c_void)
        } else {
            s.parse::<isize>().map(|value| value as *mut c_void)
        };
        pointer
            .map(|pointer| WindowHandle(HWND(pointer)))
            .map_err(|error| WindowHandleParseError(error.to_string()))
    }
}

impl Display for WindowHandleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for WindowHandleParseError {}

impl Args {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_window_handle() {
        let handle: WindowHandle = "394708".parse().unwrap();

        assert_eq!(handle.0.0 as usize, 394708);
    }

    #[test]
    fn parses_hexadecimal_window_handle() {
        let lowercase: WindowHandle = "0x605d4".parse().unwrap();
        let uppercase: WindowHandle = "0X605D4".parse().unwrap();

        assert_eq!(lowercase.0.0 as usize, 0x605d4);
        assert_eq!(uppercase.0.0 as usize, 0x605d4);
    }

    #[test]
    fn rejects_invalid_window_handle() {
        assert!("not-a-handle".parse::<WindowHandle>().is_err());
        assert!("0xnot-hex".parse::<WindowHandle>().is_err());
    }
}
