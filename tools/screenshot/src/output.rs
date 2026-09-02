use std::io::{self, Write};

use serde::Serialize;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
pub enum OutputFormat {
    Table,
    Json,
}

impl OutputFormat {
    pub fn from_json(json: bool) -> Self {
        if json { Self::Json } else { Self::Table }
    }
}

pub trait TableOutput {
    fn write_table(&self, writer: &mut dyn Write) -> io::Result<()>;
}

pub fn render_output<T>(format: OutputFormat, output: &T) -> Result<()>
where
    T: Serialize + TableOutput,
{
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    write_output(&mut writer, format, output)
}

fn write_output<T>(writer: &mut dyn Write, format: OutputFormat, output: &T) -> Result<()>
where
    T: Serialize + TableOutput,
{
    match format {
        OutputFormat::Table => output.write_table(writer)?,
        OutputFormat::Json => {
            serde_json::to_writer(&mut *writer, output)?;
            writeln!(writer)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct WindowsOutput {
    schema_version: u32,
    query: Option<String>,
    pub windows: Vec<WindowOutput>,
}

impl WindowsOutput {
    pub fn new(query: Option<String>, windows: Vec<WindowOutput>) -> Self {
        Self {
            schema_version: 1,
            query,
            windows,
        }
    }
}

#[derive(Serialize)]
pub struct WindowOutput {
    pub hwnd: String,
    pub pid: u32,
    pub process_name: Option<String>,
    pub title: String,
}

impl TableOutput for WindowsOutput {
    fn write_table(&self, writer: &mut dyn Write) -> io::Result<()> {
        if self.windows.is_empty() {
            return Ok(());
        }

        if let Some(query) = &self.query {
            writeln!(
                writer,
                "{} windows found matching '{}':",
                self.windows.len(),
                query
            )?;
        } else {
            writeln!(writer, "{} windows found:", self.windows.len())?;
        }
        writeln!(
            writer,
            "  HWND                  PID         Process Name                  Window Title"
        )?;
        for window in &self.windows {
            writeln!(
                writer,
                "  {:<20}  {:>6}      {:<25}     {}",
                window.hwnd,
                window.pid,
                window.process_name.as_deref().unwrap_or("<Unknown>"),
                window.title
            )?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct DisplaysOutput {
    schema_version: u32,
    pub displays: Vec<DisplayOutput>,
}

impl DisplaysOutput {
    pub fn new(displays: Vec<DisplayOutput>) -> Self {
        Self {
            schema_version: 1,
            displays,
        }
    }
}

#[derive(Serialize)]
pub struct DisplayOutput {
    pub id: usize,
    pub hmonitor: String,
    pub primary: bool,
    pub resolution: ResolutionOutput,
    pub position: PositionOutput,
    pub refresh_hz: u32,
    pub hdr_enabled: Option<bool>,
    pub device_name: String,
    pub display_name: String,
}

#[derive(Serialize)]
pub struct ResolutionOutput {
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize)]
pub struct PositionOutput {
    pub x: i32,
    pub y: i32,
}

impl TableOutput for DisplaysOutput {
    fn write_table(&self, writer: &mut dyn Write) -> io::Result<()> {
        writeln!(writer, "{} displays found:", self.displays.len())?;
        writeln!(
            writer,
            "  ID  HMONITOR              Primary  Resolution    Position          Refresh  HDR      Device          Display Name"
        )?;
        for display in &self.displays {
            let resolution = format!("{}x{}", display.resolution.width, display.resolution.height);
            let position = format!("({}, {})", display.position.x, display.position.y);
            let primary = if display.primary { "Yes" } else { "No" };
            let hdr = match display.hdr_enabled {
                Some(true) => "Yes",
                Some(false) => "No",
                None => "Unknown",
            };
            writeln!(
                writer,
                "  {:>2}  {:<20}  {:<7}  {:<12}  {:<16}  {:>3} Hz  {:<7}  {:<14}  {}",
                display.id,
                display.hmonitor,
                primary,
                resolution,
                position,
                display.refresh_hz,
                hdr,
                display.device_name,
                display.display_name
            )?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct CaptureOutput {
    schema_version: u32,
    output: String,
    width: u32,
    height: u32,
    format: String,
}

impl CaptureOutput {
    pub fn new(output: String, width: u32, height: u32, format: String) -> Self {
        Self {
            schema_version: 1,
            output,
            width,
            height,
            format,
        }
    }
}

impl TableOutput for CaptureOutput {
    fn write_table(&self, writer: &mut dyn Write) -> io::Result<()> {
        writeln!(
            writer,
            "Screenshot saved to '{}' ({}x{}, {}).",
            self.output, self.width, self.height, self.format
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn windows_output_serializes_handles_as_strings() {
        let output = WindowsOutput::new(
            Some("terminal".to_string()),
            vec![WindowOutput {
                hwnd: "9007199254740993".to_string(),
                pid: 42,
                process_name: None,
                title: "Terminal".to_string(),
            }],
        );
        let mut bytes = Vec::new();

        write_output(&mut bytes, OutputFormat::Json, &output).unwrap();

        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["windows"][0]["hwnd"], "9007199254740993");
        assert!(json["windows"][0]["process_name"].is_null());
    }

    #[test]
    fn displays_output_renders_table() {
        let output = DisplaysOutput::new(vec![DisplayOutput {
            id: 1,
            hmonitor: "1234".to_string(),
            primary: true,
            resolution: ResolutionOutput {
                width: 2560,
                height: 1440,
            },
            position: PositionOutput { x: 0, y: 0 },
            refresh_hz: 120,
            hdr_enabled: Some(false),
            device_name: r"\\.\DISPLAY1".to_string(),
            display_name: "Example Display".to_string(),
        }]);
        let mut bytes = Vec::new();

        write_output(&mut bytes, OutputFormat::Table, &output).unwrap();

        let table = String::from_utf8(bytes).unwrap();
        assert!(table.contains("1 displays found:"));
        assert!(table.contains("2560x1440"));
        assert!(table.contains("Example Display"));
    }
}
