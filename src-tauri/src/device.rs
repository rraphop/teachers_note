//! 기기 사양 확인 (CPU, RAM, GPU/VRAM, 여유 저장 공간).
//! 처음 한 번 확인한 값을 DB 에 저장해 두고, 교사가 '기기정보 다시 읽기'를 누를 때만 다시 읽는다.

use crate::db;
use crate::error::AppResult;
use crate::util;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub vram_bytes: u64,
    pub shared_bytes: u64,
    pub integrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceInfo {
    pub cpu_name: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub ram_total_bytes: u64,
    pub ram_available_bytes: u64,
    pub gpus: Vec<GpuInfo>,
    pub disk_free_bytes: Option<u64>,
    pub os: String,
    pub checked_at: String,
    /// 읽지 못한 항목과 이유 (쉬운 문장)
    pub problems: Vec<String>,
    pub recommended_model: String,
}

const KEY: &str = "device_info";

fn read_gpus() -> Result<Vec<GpuInfo>, String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| format!("그래픽 정보를 읽지 못했습니다 ({e})"))?;
        let mut out = Vec::new();
        let mut i = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            let len = desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]).trim().to_string();
            let vram = desc.DedicatedVideoMemory as u64;
            if name.contains("Microsoft Basic Render") {
                continue;
            }
            out.push(GpuInfo {
                name,
                vram_bytes: vram,
                shared_bytes: desc.SharedSystemMemory as u64,
                // 전용 메모리가 2GB 미만이면 CPU 내장 그래픽으로 본다
                integrated: vram < 2 * 1024 * 1024 * 1024,
            });
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    {
        Ok(vec![])
    }
}

pub fn probe(data_dir: &Path) -> DeviceInfo {
    let mut problems = Vec::new();
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let cpu_name = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| {
        problems.push("CPU 이름을 읽지 못했습니다.".to_string());
        "알 수 없음".to_string()
    });
    let logical = sys.cpus().len() as u32;
    let physical = sysinfo::System::physical_core_count().map(|n| n as u32).unwrap_or_else(|| {
        problems.push("CPU 코어 수를 정확히 읽지 못해 논리 코어 수로 대신했습니다.".to_string());
        logical.max(1)
    });
    let total = sys.total_memory();
    if total == 0 {
        problems.push("메모리(RAM) 크기를 읽지 못했습니다. 권장 모델은 기본형으로 표시합니다.".to_string());
    }
    let gpus = match read_gpus() {
        Ok(g) => g,
        Err(e) => {
            problems.push(e);
            vec![]
        }
    };
    let disk = crate::models::free_space(data_dir);
    if disk.is_none() {
        problems.push("저장 공간을 읽지 못했습니다.".to_string());
    }
    let recommended = if total == 0 { "qwen3.5-2b" } else { crate::models::recommend(total) };
    DeviceInfo {
        cpu_name,
        physical_cores: physical,
        logical_cores: logical,
        ram_total_bytes: total,
        ram_available_bytes: sys.available_memory(),
        gpus,
        disk_free_bytes: disk,
        os: sysinfo::System::long_os_version().unwrap_or_else(|| "Windows".into()),
        checked_at: util::fmt_dt(util::now()),
        problems,
        recommended_model: recommended.to_string(),
    }
}

pub fn cached(conn: &Connection) -> AppResult<Option<DeviceInfo>> {
    Ok(db::get_setting(conn, KEY)?.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn store(conn: &Connection, info: &DeviceInfo) -> AppResult<()> {
    db::set_setting(conn, KEY, &serde_json::to_string(info)?)
}

/// AI 계산에 쓸 스레드 수: 물리 코어에서 하나를 남겨 화면이 멈추지 않게 한다
pub fn default_threads(info: Option<&DeviceInfo>) -> u32 {
    let cores = info.map(|i| i.physical_cores).filter(|c| *c > 0).unwrap_or_else(|| {
        sysinfo::System::physical_core_count().map(|n| n as u32).unwrap_or(4)
    });
    match cores {
        0..=2 => cores.max(1),
        3..=4 => cores - 1,
        _ => (cores - 1).min(8),
    }
}
