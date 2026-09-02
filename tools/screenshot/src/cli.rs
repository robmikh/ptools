use std::{ffi::c_void, fmt::Display, path::PathBuf, str::FromStr};

use clap::{Parser, Subcommand};
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
        title: String,
    },
    /// Enumerate displays
    EnumDisplays,
    /// Capture a window
    CaptureWindow {
        /// Exact title match
        #[arg(short, long, conflicts_with = "handle")]
        title: Option<String>,
        /// Window handle
        #[arg(long, conflicts_with = "title")]
        handle: Option<WindowHandle>,
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Capture a display
    CaptureDisplay {
        /// Capture a monitor by number (starts at 1)
        #[arg(short, long, conflicts_with = "primary")]
        monitor: Option<usize>,
        /// Capture the primary monitor
        #[arg(short, long, conflicts_with = "monitor")]
        primary: bool,
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
    },
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
        let pointer: isize = match s.parse() {
            Ok(value) => value,
            Err(error) => return Err(WindowHandleParseError(error.to_string())),
        };
        Ok(WindowHandle(HWND(pointer as *mut c_void)))
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
