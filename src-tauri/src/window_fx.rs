//! 창 투명도 (왼쪽 메뉴 ‘항상 위에 표시’ 아래)
//!
//! Windows 계층 창(WS_EX_LAYERED)의 알파로 창 전체를 비쳐 보이게 한다. 창이 사라져 보이지 않게 되는 일을 막으려고
//! 투명도는 0~70% 까지만 받는다. 0% 로 되돌리면 알파 255(불투명)로 둔다(계층 스타일을 떼면 다시 그리기 전까지 깜박인다).

use std::sync::atomic::{AtomicU8, Ordering};

/// 받을 수 있는 가장 큰 투명도(%)
pub const MAX_TRANSPARENCY: u8 = 70;

/// 지금 창에 적용한 투명도 (창 틀이 스타일을 다시 쓸 때 되살리려고 기억)
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> u8 {
    CURRENT.load(Ordering::Relaxed)
}

pub fn clamp(t: u8) -> u8 {
    t.min(MAX_TRANSPARENCY)
}

/// 투명도(%) → 알파(0~255, 반올림)
pub fn alpha_for(t: u8) -> u8 {
    (((100 - clamp(t) as u32) * 255 + 50) / 100) as u8
}

/// 창 핸들(정수)에 투명도를 적용한다. 창을 만든 스레드(메인)에서 부른다.
#[cfg(windows)]
pub fn apply(raw_hwnd: isize, transparency: u8) -> Result<(), String> {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW, WS_EX_LAYERED};
    let hwnd = HWND(raw_hwnd as *mut core::ffi::c_void);
    CURRENT.store(clamp(transparency), Ordering::Relaxed);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED.0 as isize;
        if ex & layered == 0 {
            // 처음부터 불투명이면 계층 창으로 바꿀 필요가 없다
            if transparency == 0 {
                return Ok(());
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | layered);
        }
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha_for(transparency), LWA_ALPHA).map_err(|e| format!("창 투명도를 바꾸지 못했습니다: {}", e.message()))
    }
}

/// 창 틀(tao)은 ‘항상 위’·최대화 등을 바꿀 때 확장 스타일을 다시 쓰면서 계층 창 표시를 지운다.
/// 투명도를 쓰는 중인데 계층 창 표시가 없으면 다시 적용한다 (메인 스레드에서).
#[cfg(windows)]
pub fn ensure(raw_hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_LAYERED};
    let t = current();
    if t == 0 {
        return;
    }
    let ex = unsafe { GetWindowLongPtrW(HWND(raw_hwnd as *mut core::ffi::c_void), GWL_EXSTYLE) };
    if ex & (WS_EX_LAYERED.0 as isize) == 0 {
        let _ = apply(raw_hwnd, t);
    }
}

#[cfg(not(windows))]
pub fn ensure(_raw_hwnd: isize) {}

#[cfg(not(windows))]
pub fn apply(_raw_hwnd: isize, _transparency: u8) -> Result<(), String> {
    Err("이 운영체제에서는 아직 창 투명도를 지원하지 않습니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparency_is_capped_and_mapped_to_alpha() {
        assert_eq!(alpha_for(0), 255);
        assert_eq!(alpha_for(30), 179);
        assert_eq!(alpha_for(70), 77);
        // 70% 넘게는 받지 않는다 (창이 거의 안 보이게 되는 일 막기)
        assert_eq!(clamp(95), 70);
        assert_eq!(alpha_for(95), 77);
    }
}
