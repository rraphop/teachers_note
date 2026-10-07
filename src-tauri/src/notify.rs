//! Windows 알림(화면 오른쪽 아래)과 자동 시작.
//!
//! 설치 프로그램 없이 실행하는 Portable 앱은 알림 발신자(AppUserModelID)를 현재 사용자 레지스트리
//! (HKCU\Software\Classes\AppUserModelId)에 등록해야 알림이 표시된다. 관리자 권한은 필요 없다.
//! 실패하면 Windows 가 알려 준 실제 오류를 그대로 돌려준다.

pub const AUMID: &str = "teachers_note";
/// 1.0 첫 배포까지 쓰던 알림 발신자 이름 (등록할 때 지운다)
const LEGACY_AUMIDS: [&str; 1] = ["LocalTeacherNotebook.V2"];
pub const DISPLAY_NAME: &str = "로컬 AI 교무수첩";
const RUN_VALUE: &str = "teachers_note";
/// 1.0 첫 배포까지 쓰던 시작 프로그램 값 이름 (켤 때 새 이름으로 옮긴다)
const LEGACY_RUN_VALUES: [&str; 1] = ["LocalTeacherNotebookV2"];

/// 시작 프로그램에 적는 명령: Windows 에 로그인하면 창을 띄운 채로 켠다
/// (0.9 이전에는 `--background` 를 붙여 알림 영역에만 켰다 — 프로그램을 켤 때 이 모양으로 고쳐 쓴다)
pub fn autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

/// 시작 프로그램에 적힌 명령에서 실행 파일 경로만 꺼낸다 (`"C:\…\교무수첩_1.0.0.exe" --background` → `C:\…\교무수첩_1.0.0.exe`)
pub fn autostart_exe(cmd: &str) -> String {
    let t = cmd.trim();
    match t.strip_prefix('"').and_then(|r| r.split_once('"')) {
        Some((p, _)) => p.to_string(),
        None => t.trim_end_matches("--background").trim().to_string(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows::core::{HSTRING, PCWSTR};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set_string(key: HKEY, name: &str, value: &str) -> Result<(), String> {
        let v = wide(value);
        let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2) };
        let n = wide(name);
        let r = unsafe { RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_SZ, Some(bytes)) };
        if r != ERROR_SUCCESS {
            return Err(format!("레지스트리 값을 쓰지 못했습니다 (코드 {})", r.0));
        }
        Ok(())
    }

    fn create_key(path: &str) -> Result<HKEY, String> {
        let p = wide(path);
        let mut key = HKEY::default();
        let r = unsafe {
            RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None)
        };
        if r != ERROR_SUCCESS {
            return Err(format!("레지스트리 키를 만들지 못했습니다 (코드 {})", r.0));
        }
        Ok(key)
    }

    pub fn register(icon: Option<&std::path::Path>) -> Result<(), String> {
        for old in LEGACY_AUMIDS {
            let p = wide(&format!("Software\\Classes\\AppUserModelId\\{old}"));
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
            }
        }
        let key = create_key(&format!("Software\\Classes\\AppUserModelId\\{AUMID}"))?;
        let res = (|| {
            set_string(key, "DisplayName", DISPLAY_NAME)?;
            if let Some(i) = icon {
                set_string(key, "IconUri", &i.display().to_string())?;
            }
            set_string(key, "IconBackgroundColor", "FF1F5F4A")
        })();
        unsafe {
            let _ = RegCloseKey(key);
        }
        res
    }

    pub fn unregister() {
        let p = wide(&format!("Software\\Classes\\AppUserModelId\\{AUMID}"));
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
        }
    }

    pub fn show(title: &str, body: &str) -> Result<(), String> {
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::{NotificationSetting, ToastNotification, ToastNotificationManager};
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
            .map_err(|e| format!("Windows 알림을 준비하지 못했습니다: {} (0x{:08X})", e.message(), e.code().0))?;
        if let Ok(setting) = notifier.Setting() {
            let blocked = match setting {
                NotificationSetting::Enabled => None,
                NotificationSetting::DisabledForApplication => {
                    Some("Windows 설정에서 이 프로그램의 알림이 꺼져 있습니다. 설정 > 시스템 > 알림에서 '로컬 AI 교무수첩'을 켜 주세요.")
                }
                NotificationSetting::DisabledForUser => Some("Windows 알림이 모두 꺼져 있습니다. 설정 > 시스템 > 알림을 켜 주세요."),
                NotificationSetting::DisabledByGroupPolicy => Some("학교 PC 관리 정책(그룹 정책)으로 알림이 막혀 있습니다."),
                NotificationSetting::DisabledByManifest => Some("이 프로그램의 알림 등록 정보가 올바르지 않습니다."),
                _ => None,
            };
            if let Some(b) = blocked {
                return Err(b.to_string());
            }
        }
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual><audio src=\"ms-winsoundevent:Notification.Reminder\"/></toast>",
            xml_escape(title),
            xml_escape(body)
        );
        let doc = XmlDocument::new().map_err(|e| e.message().to_string())?;
        doc.LoadXml(&HSTRING::from(xml)).map_err(|e| format!("알림 내용을 만들지 못했습니다: {}", e.message()))?;
        let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.message().to_string())?;
        notifier
            .Show(&toast)
            .map_err(|e| format!("Windows 가 알림 표시를 거부했습니다: {} (0x{:08X})", e.message(), e.code().0))
    }

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    /// 자동 시험(paths::is_test_run)에서는 실제 시작 프로그램 목록 대신 이 키에 쓴다
    const TEST_RUN_KEY: &str = "Software\\teachers_note-Test\\Run";

    fn run_key() -> &'static str {
        if crate::paths::is_test_run() { TEST_RUN_KEY } else { RUN_KEY }
    }

    fn read_value(name: &str) -> Option<String> {
        let p = wide(run_key());
        let n = wide(name);
        let mut key = HKEY::default();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, KEY_READ, &mut key) != ERROR_SUCCESS {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let mut size = (buf.len() * 2) as u32;
            let r = RegQueryValueExW(key, PCWSTR(n.as_ptr()), None, None, Some(buf.as_mut_ptr() as *mut u8), Some(&mut size));
            let _ = RegCloseKey(key);
            if r != ERROR_SUCCESS {
                return None;
            }
            let len = (size as usize / 2).saturating_sub(1).min(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    fn delete_value(name: &str) {
        let p = wide(run_key());
        let mut key = HKEY::default();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, KEY_WRITE, &mut key) == ERROR_SUCCESS {
                let n = wide(name);
                let _ = RegDeleteValueW(key, PCWSTR(n.as_ptr()));
                let _ = RegCloseKey(key);
            }
        }
    }

    /// 예전 이름(LocalTeacherNotebookV2)으로 적힌 자동 시작을 새 이름으로 옮긴다 (명령은 그대로)
    pub fn migrate_autostart_name() {
        for old in LEGACY_RUN_VALUES {
            if let Some(cmd) = read_value(old) {
                if read_value(RUN_VALUE).is_none() {
                    if let Ok(key) = create_key(run_key()) {
                        let _ = set_string(key, RUN_VALUE, &cmd);
                        unsafe {
                            let _ = RegCloseKey(key);
                        }
                    }
                }
                if read_value(RUN_VALUE).is_some() {
                    delete_value(old);
                }
            }
        }
    }

    pub fn set_autostart(enabled: bool, exe: &std::path::Path) -> Result<(), String> {
        for old in LEGACY_RUN_VALUES {
            delete_value(old);
        }
        if enabled {
            let key = create_key(run_key())?;
            let res = set_string(key, RUN_VALUE, &autostart_command(exe));
            unsafe {
                let _ = RegCloseKey(key);
            }
            res
        } else {
            let p = wide(run_key());
            let mut key = HKEY::default();
            unsafe {
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, KEY_WRITE, &mut key) == ERROR_SUCCESS {
                    let n = wide(RUN_VALUE);
                    let _ = RegDeleteValueW(key, PCWSTR(n.as_ptr()));
                    let _ = RegCloseKey(key);
                }
            }
            Ok(())
        }
    }

    pub fn autostart_target() -> Option<String> {
        read_value(RUN_VALUE)
    }
}

#[cfg(windows)]
pub use win::{autostart_target, migrate_autostart_name, register, set_autostart, show, unregister};

#[cfg(not(windows))]
pub fn register(_icon: Option<&std::path::Path>) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn unregister() {}
#[cfg(not(windows))]
pub fn show(_t: &str, _b: &str) -> Result<(), String> {
    Err("이 운영체제에서는 아직 알림을 지원하지 않습니다.".into())
}
#[cfg(not(windows))]
pub fn set_autostart(_e: bool, _p: &std::path::Path) -> Result<(), String> {
    Err("이 운영체제에서는 아직 자동 시작을 지원하지 않습니다.".into())
}
#[cfg(not(windows))]
pub fn autostart_target() -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn migrate_autostart_name() {}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_xml() {
        assert_eq!(super::xml_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }

    #[test]
    fn autostart_command_shows_window_and_reads_old_form() {
        let exe = std::path::Path::new(r"C:\Users\teacher\Desktop\교무수첩_1.0.0.exe");
        // 새 모양: 경로만 (창을 띄운 채로 켠다)
        assert_eq!(super::autostart_command(exe), r#""C:\Users\teacher\Desktop\교무수첩_1.0.0.exe""#);
        assert_eq!(super::autostart_exe(&super::autostart_command(exe)), exe.display().to_string());
        // 예전 모양(알림 영역에만 켜기)도 경로를 읽는다
        assert_eq!(super::autostart_exe(r#""C:\a b\교무수첩_0.9.0.exe" --background"#), r"C:\a b\교무수첩_0.9.0.exe");
        assert_eq!(super::autostart_exe(r"C:\a\teachers_note.exe --background"), r"C:\a\teachers_note.exe");
    }
}
