use std::collections::HashMap;

use windows::Win32::{
    Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
        DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    },
    Foundation::{LPARAM, RECT, WIN32_ERROR},
    Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
        GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    },
    UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
};
use windows::core::{BOOL, PCWSTR, Result};

#[derive(Clone)]
pub struct DisplayInfo {
    pub handle: HMONITOR,
    pub rect: RECT,
    pub device_name: String,
    pub display_name: String,
    pub frequency: u32,
    pub is_primary: bool,
    pub hdr_enabled: Option<bool>,
}

impl DisplayInfo {
    fn new(
        monitor_handle: HMONITOR,
        metadata_by_device: &HashMap<String, DisplayMetadata>,
    ) -> Result<Self> {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of_val(&info) as u32;

        unsafe {
            GetMonitorInfoW(monitor_handle, &mut info.monitorInfo).ok()?;
        }

        let device_name = utf16_to_string(&info.szDevice)?;
        let mut dev_mode = DEVMODEW::default();
        unsafe {
            EnumDisplaySettingsW(
                PCWSTR(info.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut dev_mode,
            )
            .ok()?;
        }

        let metadata = metadata_by_device.get(&device_name);
        Ok(Self {
            handle: monitor_handle,
            rect: info.monitorInfo.rcMonitor,
            device_name: device_name.clone(),
            display_name: metadata
                .map(|metadata| metadata.display_name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or(device_name),
            frequency: dev_mode.dmDisplayFrequency,
            is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            hdr_enabled: metadata.map(|metadata| metadata.hdr_enabled),
        })
    }
}

pub fn enumerate_displays() -> Result<Vec<DisplayInfo>> {
    let handles = get_all_display_handles()?;
    let metadata_by_device = build_device_metadata_map()?;
    handles
        .into_iter()
        .map(|handle| DisplayInfo::new(handle, &metadata_by_device))
        .collect()
}

fn get_all_display_handles() -> Result<Vec<HMONITOR>> {
    unsafe {
        let mut handles = Vec::new();
        EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor),
            LPARAM(&mut handles as *mut _ as isize),
        )
        .ok()?;
        Ok(handles)
    }
}

extern "system" fn enum_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, state: LPARAM) -> BOOL {
    unsafe {
        let handles = (state.0 as *mut Vec<HMONITOR>).as_mut().unwrap();
        handles.push(monitor);
    }
    true.into()
}

struct DisplayMetadata {
    display_name: String,
    hdr_enabled: bool,
}

fn build_device_metadata_map() -> Result<HashMap<String, DisplayMetadata>> {
    let mut metadata_by_device = HashMap::new();
    for path_info in get_display_config_path_infos()? {
        let mut source_name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                adapterId: path_info.sourceInfo.adapterId,
                id: path_info.sourceInfo.id,
            },
            ..Default::default()
        };
        unsafe {
            WIN32_ERROR(DisplayConfigGetDeviceInfo(&mut source_name.header) as u32).ok()?;
        }
        let device_name = utf16_to_string(&source_name.viewGdiDeviceName)?;

        let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                adapterId: path_info.targetInfo.adapterId,
                id: path_info.targetInfo.id,
            },
            ..Default::default()
        };
        unsafe {
            WIN32_ERROR(DisplayConfigGetDeviceInfo(&mut color_info.header) as u32).ok()?;
        }
        let hdr_enabled = unsafe {
            let advanced_color_enabled = color_info.Anonymous.Anonymous._bitfield & 0x2 != 0;
            let wide_color_enforced = color_info.Anonymous.Anonymous._bitfield & 0x4 != 0;
            advanced_color_enabled && !wide_color_enforced
        };

        let mut target_name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                size: std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                adapterId: path_info.targetInfo.adapterId,
                id: path_info.targetInfo.id,
            },
            ..Default::default()
        };
        unsafe {
            WIN32_ERROR(DisplayConfigGetDeviceInfo(&mut target_name.header) as u32).ok()?;
        }

        metadata_by_device.insert(
            device_name,
            DisplayMetadata {
                display_name: utf16_to_string(&target_name.monitorFriendlyDeviceName)?,
                hdr_enabled,
            },
        );
    }
    Ok(metadata_by_device)
}

fn get_display_config_path_infos() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    let mut num_paths = 0;
    let mut num_modes = 0;
    unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut num_paths, &mut num_modes).ok()?;
    }

    let mut path_infos = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
    let mut mode_infos = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];
    unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut num_paths,
            path_infos.as_mut_ptr(),
            &mut num_modes,
            mode_infos.as_mut_ptr(),
            None,
        )
        .ok()?;
    }
    path_infos.truncate(num_paths as usize);
    Ok(path_infos)
}

fn utf16_to_string(value: &[u16]) -> Result<String> {
    let len = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    Ok(String::from_utf16(&value[..len])?)
}
