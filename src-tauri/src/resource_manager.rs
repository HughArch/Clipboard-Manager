#[cfg(target_os = "windows")]
use winapi::um::wingdi::{DeleteObject, DeleteDC};
#[cfg(target_os = "windows")]
use winapi::um::winuser::{ReleaseDC, DestroyIcon};

use crate::icon_cache::cleanup_icon_cache;
use crate::window_info::get_last_window_info;

// 资源清理守护者，确保线程退出时清理资源
pub struct ClipboardCleanupGuard;

impl ClipboardCleanupGuard {
    pub fn new() -> Self {
        tracing::info!("剪贴板监听器线程启动，资源守护者已创建");
        Self
    }
}

impl Drop for ClipboardCleanupGuard {
    fn drop(&mut self) {
        tracing::info!("剪贴板监听器线程退出，开始清理资源...");
        
        // 清理图标缓存
        cleanup_icon_cache();
        
        // 清理窗口信息缓存
        if let Ok(mut guard) = get_last_window_info().write() {
            guard.1 = None;
        }
        
        // 强制内存清理
        #[cfg(target_os = "windows")]
        unsafe {
            use winapi::um::winbase::SetProcessWorkingSetSize;
            use winapi::um::processthreadsapi::GetCurrentProcess;
            let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
        
        tracing::info!("剪贴板监听器线程资源清理完成");
    }
}

// Windows资源管理器 - 确保所有Windows API资源正确释放
#[cfg(target_os = "windows")]
pub struct WindowsResourceManager {
    handles: Vec<winapi::shared::windef::HGDIOBJ>,
    dcs: Vec<winapi::shared::windef::HDC>,
    icons: Vec<winapi::shared::windef::HICON>,
}

#[cfg(target_os = "windows")]
impl WindowsResourceManager {
    pub fn new() -> Self {
        Self {
            handles: Vec::new(),
            dcs: Vec::new(),
            icons: Vec::new(),
        }
    }
    
    pub fn track_handle(&mut self, handle: winapi::shared::windef::HGDIOBJ) {
        self.handles.push(handle);
    }
    
    pub fn track_dc(&mut self, dc: winapi::shared::windef::HDC) {
        if !dc.is_null() {
            self.dcs.push(dc);
        }
    }
    
    pub fn track_icon(&mut self, icon: winapi::shared::windef::HICON) {
        if !icon.is_null() {
            self.icons.push(icon);
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsResourceManager {
    fn drop(&mut self) {
        // 清理所有GDI对象
        for &handle in &self.handles {
            if !handle.is_null() {
                unsafe {
                    let result = DeleteObject(handle);
                    if result == 0 {
                        tracing::info!("警告: 删除GDI对象失败: {:?}", handle);
                    }
                }
            }
        }
        
        // 清理所有DC
        for &dc in &self.dcs {
            if !dc.is_null() {
                unsafe {
                    let result = DeleteDC(dc);
                    if result == 0 {
                        // 尝试ReleaseDC
                        let release_result = ReleaseDC(std::ptr::null_mut(), dc);
                        if release_result == 0 {
                            tracing::info!("警告: 释放DC失败: {:?}", dc);
                        }
                    }
                }
            }
        }
        
        // 清理所有图标
        for &icon in &self.icons {
            if !icon.is_null() {
                unsafe {
                    let result = DestroyIcon(icon);
                    if result == 0 {
                        tracing::info!("警告: 销毁图标失败: {:?}", icon);
                    }
                }
            }
        }
        
        tracing::info!("Windows资源管理器清理完成: {} handles, {} DCs, {} icons", 
                self.handles.len(), self.dcs.len(), self.icons.len());
    }
} 