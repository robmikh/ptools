mod capture;
mod cli;
mod d3d;
mod display_info;
mod handle;
mod wic;
mod window_info;

use cli::Args;
use wic::create_wic_factory;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTOPRIMARY, MonitorFromWindow};
use windows::Win32::Graphics::Imaging::{
    GUID_ContainerFormatPng, GUID_ContainerFormatWmp, GUID_WICPixelFormat32bppBGRA,
    GUID_WICPixelFormat64bppRGBAHalf, IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::Storage::FileSystem::GetFullPathNameW;
use windows::Win32::System::Com::{STGM_CREATE, STGM_READWRITE};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::WinRT::{
    Graphics::Capture::IGraphicsCaptureItemInterop, RO_INIT_MULTITHREADED, RoInitialize,
};
use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetWindowThreadProcessId};
use windows::core::{HSTRING, IInspectable, Interface, PCWSTR, PWSTR, Result};

use capture::enumerate_capturable_windows;
use display_info::enumerate_displays;
use std::path::Path;
use std::sync::mpsc::channel;
use window_info::WindowInfo;

use crate::handle::AutoHandle;

fn create_capture_item_for_window(window_handle: HWND) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe { interop.CreateForWindow(window_handle) }
}

fn create_capture_item_for_monitor(monitor_handle: HMONITOR) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe { interop.CreateForMonitor(monitor_handle) }
}

fn main() -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }

    let args = Args::parse_args();
    let (item, output) = match args.command {
        cli::Commands::EnumWindows { title } => {
            show_window_query(&title);
            std::process::exit(0);
        }
        cli::Commands::CaptureWindow {
            title,
            handle,
            output,
        } => {
            let item: GraphicsCaptureItem = if let Some(title) = title {
                // Find an exact title match.
                let windows = find_exact_window(&title);
                if windows.is_empty() {
                    println!("No window matched title!");
                    std::process::exit(1);
                } else if windows.len() > 1 {
                    println!(
                        "More than one window ({}) matched the given title! Use the `enum-windows` subcommand to find the desired window handle instead.",
                        windows.len()
                    );
                    std::process::exit(1);
                } else {
                    let handle = windows[0].handle;
                    create_capture_item_for_window(handle)?
                }
            } else if let Some(handle) = handle {
                create_capture_item_for_window(handle.0)?
            } else {
                println!(
                    "Must specify either an exact title (--title) or a window handle (--handle)!"
                );
                std::process::exit(1);
            };

            (item, output)
        }
        cli::Commands::CaptureDisplay {
            monitor,
            primary,
            output,
        } => {
            let item: GraphicsCaptureItem = if let Some(monitor) = monitor {
                let displays = enumerate_displays()?;
                if monitor == 0 {
                    println!("Invalid input, ids start with 1.");
                    std::process::exit(1);
                }
                let index = monitor - 1;
                if index >= displays.len() {
                    println!("Invalid input, id is higher than the number of displays!");
                    std::process::exit(1);
                }
                let display = &displays[index];
                create_capture_item_for_monitor(display.handle)?
            } else if primary {
                let monitor_handle =
                    unsafe { MonitorFromWindow(GetDesktopWindow(), MONITOR_DEFAULTTOPRIMARY) };
                create_capture_item_for_monitor(monitor_handle)?
            } else {
                println!(
                    "Must specify either a monitor number (--monitor) or the primary (--primary)!"
                );
                std::process::exit(1);
            };

            (item, output)
        }
    };

    // Validate path and derive pixel format
    let pixel_format = if let Some(pixel_format) = validate_path(&output) {
        pixel_format
    } else {
        println!("Invalid file extension! Expecting 'png' or 'jxr'.");
        std::process::exit(1);
    };

    // Initialize D3D11
    let d3d_device = d3d::create_d3d_device()?;
    let d3d_context = unsafe { d3d_device.GetImmediateContext()? };

    // Initialize WIC
    let wic_factory = create_wic_factory()?;

    let texture = take_screenshot(&item, pixel_format, &d3d_device, &d3d_context)?;
    save_texture(
        &d3d_context,
        &texture,
        &wic_factory,
        output.to_str().unwrap(),
    )?;

    Ok(())
}

fn take_screenshot(
    item: &GraphicsCaptureItem,
    pixel_format: DirectXPixelFormat,
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
) -> Result<ID3D11Texture2D> {
    let item_size = item.Size()?;

    let device = d3d::create_direct3d_device(d3d_device)?;
    let frame_pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&device, pixel_format, 1, item_size)?;
    let session = frame_pool.CreateCaptureSession(item)?;

    let (sender, receiver) = channel();
    frame_pool.FrameArrived(
        &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
            move |frame_pool, _| {
                let frame_pool = frame_pool.as_ref().unwrap();
                let frame = frame_pool.TryGetNextFrame()?;
                sender.send(frame).unwrap();
                Ok(())
            }
        }),
    )?;
    session.StartCapture()?;

    let texture = unsafe {
        let frame = receiver.recv().unwrap();

        let source_texture: ID3D11Texture2D =
            d3d::get_d3d_interface_from_object(&frame.Surface()?)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut desc);
        desc.BindFlags = 0;
        desc.MiscFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let copy_texture = {
            let mut texture = None;
            d3d_device.CreateTexture2D(&desc, None, Some(&mut texture))?;
            texture.unwrap()
        };

        d3d_context.CopyResource(Some(&copy_texture.cast()?), Some(&source_texture.cast()?));

        session.Close()?;
        frame_pool.Close()?;

        copy_texture
    };

    Ok(texture)
}

fn get_bytes_from_texture(
    d3d_context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
) -> Result<(Vec<u8>, u32)> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc as *mut _);

        let bytes_per_pixel = match desc.Format {
            DXGI_FORMAT_B8G8R8A8_UNORM => 4,
            DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
            _ => {
                return Err(windows::core::Error::new(
                    E_INVALIDARG,
                    "Unsupported pixel format!",
                ));
            }
        };

        let resource: ID3D11Resource = texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(
            Some(&resource.clone()),
            0,
            D3D11_MAP_READ,
            0,
            Some(&mut mapped),
        )?;

        // Get a slice of bytes
        let slice: &[u8] = {
            std::slice::from_raw_parts(
                mapped.pData as *const _,
                (desc.Height * mapped.RowPitch) as usize,
            )
        };

        let mut bytes = vec![0u8; (desc.Width * desc.Height * bytes_per_pixel) as usize];
        for row in 0..desc.Height {
            let data_begin = (row * (desc.Width * bytes_per_pixel)) as usize;
            let data_end = ((row + 1) * (desc.Width * bytes_per_pixel)) as usize;
            let slice_begin = (row * mapped.RowPitch) as usize;
            let slice_end = slice_begin + (desc.Width * bytes_per_pixel) as usize;
            bytes[data_begin..data_end].copy_from_slice(&slice[slice_begin..slice_end]);
        }

        d3d_context.Unmap(Some(&resource), 0);

        Ok((bytes, bytes_per_pixel))
    }
}

fn save_texture(
    d3d_context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    wic_factory: &IWICImagingFactory,
    path: &str,
) -> Result<()> {
    let (width, height, container_format, pixel_format) = unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc as *mut _);
        let (container_format, pixel_format) = match desc.Format {
            DXGI_FORMAT_B8G8R8A8_UNORM => (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA),
            DXGI_FORMAT_R16G16B16A16_FLOAT => {
                (GUID_ContainerFormatWmp, GUID_WICPixelFormat64bppRGBAHalf)
            }
            _ => {
                return Err(windows::core::Error::new(
                    E_INVALIDARG,
                    "Unsupported pixel format!",
                ));
            }
        };
        (desc.Width, desc.Height, container_format, pixel_format)
    };
    let (bytes, bytes_per_pixel) = get_bytes_from_texture(d3d_context, texture)?;
    let stride = bytes_per_pixel * width;

    let encoder = unsafe { wic_factory.CreateEncoder(&container_format, std::ptr::null())? };

    unsafe {
        let stream = {
            let path = HSTRING::from(path);
            SHCreateStreamOnFileEx(&path, (STGM_CREATE | STGM_READWRITE).0, 0, true, None)?
        };
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let (frame, props) = {
            let mut frame = None;
            let mut props = None;
            encoder.CreateNewFrame(&mut frame, &mut props)?;
            (frame.unwrap(), props.unwrap())
        };

        frame.Initialize(&props)?;
        frame.SetSize(width, height)?;
        let mut target_format = pixel_format;
        frame.SetPixelFormat(&mut target_format)?;
        if target_format != pixel_format {
            return Err(windows::core::Error::new(
                E_FAIL,
                "Unsupported WIC pixel format!",
            ));
        }

        // TODO: Metadata

        frame.WritePixels(height, stride, &bytes)?;
        frame.Commit()?;
        encoder.Commit()?;
    }

    Ok(())
}

fn show_window_query(query: &str) {
    let windows = find_window(query);
    if windows.is_empty() {
        println!("No window matching '{}' found!", query);
        std::process::exit(1);
    } else {
        println!("{} windows found matching '{}':", windows.len(), query);

        println!("  PID         Process Name                  Window Title");
        for window in &windows {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(window.handle, Some(&mut pid)) };
            let process_name = get_process_name(pid).unwrap_or("<Unknown>".to_string());
            println!(
                "  {:>6}      {:<25}     {}",
                pid, process_name, window.title
            );
        }
    }
}

fn find_window(window_name: &str) -> Vec<WindowInfo> {
    let window_list = enumerate_capturable_windows();
    let mut windows: Vec<WindowInfo> = Vec::new();
    for window_info in window_list.into_iter() {
        let title = window_info.title.to_lowercase();
        if title.contains(&window_name.to_string().to_lowercase()) {
            windows.push(window_info.clone());
        }
    }
    windows
}

fn find_exact_window(window_name: &str) -> Vec<WindowInfo> {
    let window_list = enumerate_capturable_windows();
    let mut windows: Vec<WindowInfo> = Vec::new();
    for window_info in window_list.into_iter() {
        if window_info.title == window_name {
            windows.push(window_info.clone());
        }
    }
    windows
}

fn get_process_name(pid: u32) -> Result<String> {
    let handle = unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
        AutoHandle(handle)
    };
    let path_buffer = unsafe {
        let mut buffer = vec![0u16; 2048];
        let mut len = buffer.len() as u32;
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )?;

        buffer.resize(len as usize + 1, 0);
        buffer
    };
    let file_name = unsafe {
        let mut buffer = vec![0u16; path_buffer.len()];
        let mut file_part: usize = 0;
        let len = GetFullPathNameW(
            PCWSTR(path_buffer.as_ptr()),
            Some(&mut buffer),
            Some(&mut file_part as *mut _ as *mut _),
        );

        buffer.resize(len as usize, 0);

        let index = if file_part != 0 {
            (file_part - buffer.as_ptr() as usize) / std::mem::size_of::<u16>()
        } else {
            0
        };
        let slice = &buffer[index..];
        match String::from_utf16(slice) {
            Ok(string) => string,
            Err(error) => return Err(error.into()),
        }
    };
    Ok(file_name)
}

fn validate_path<P: AsRef<Path>>(path: P) -> Option<DirectXPixelFormat> {
    let path = path.as_ref();
    let mut pixel_format = None;
    if let Some(extension) = path.extension() {
        if let Some(extension) = extension.to_str() {
            match extension {
                "png" => pixel_format = Some(DirectXPixelFormat::B8G8R8A8UIntNormalized),
                "jxr" => pixel_format = Some(DirectXPixelFormat::R16G16B16A16Float),
                _ => {}
            }
        }
    }
    pixel_format
}
