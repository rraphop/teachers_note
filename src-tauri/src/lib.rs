pub mod analysis;
pub mod app;
pub mod classify;
pub mod commands;
pub mod dates;
pub mod db;
pub mod device;
pub mod duties;
pub mod error;
pub mod llm;
pub mod logging;
pub mod models;
pub mod notes;
pub mod notify;
pub mod paths;
pub mod records;
pub mod reminders;
pub mod runtime;
pub mod schedule;
pub mod search;
pub mod settings;
pub mod students;
pub mod text;
pub mod util;

use app::AppState;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const APP_ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// 시작 전에 생긴 치명적 오류는 창이 없으므로 Windows 메시지 상자로 알린다
fn fatal(msg: &str) -> ! {
    logging::error(msg);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::HSTRING;
        MessageBoxW(None, &HSTRING::from(msg), &HSTRING::from("로컬 AI 교무수첩"), MB_OK | MB_ICONERROR);
    }
    eprintln!("{msg}");
    std::process::exit(1);
}

#[derive(serde::Serialize)]
struct SelfTestReport {
    ok: bool,
    version: String,
    exe_dir: String,
    assets_embedded: bool,
    frontend_loaded: bool,
    runtime_found: bool,
    runtime_dir: Option<String>,
    data_root: String,
    db_ok: bool,
    fts5_ok: bool,
    data_dir_is_outside_exe_dir: bool,
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    // 패키징 단계에서 모델 카탈로그를 파일로 내보낸다 (창을 띄우지 않음)
    if let Some(i) = args.iter().position(|a| a == "--export-catalog") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "model-catalog.json".into());
        let body = serde_json::to_string_pretty(&json!({ "version": 1, "models": models::CATALOG })).unwrap_or_default();
        let code = if std::fs::write(&out, body).is_ok() { 0 } else { 1 };
        std::process::exit(code);
    }
    let self_test_out: Option<String> = args.iter().position(|a| a == "--self-test").map(|i| args.get(i + 1).cloned().unwrap_or_default());
    let background = args.iter().any(|a| a == "--background");

    let paths = match paths::AppPaths::resolve() {
        Ok(p) => p,
        Err(e) => fatal(&e.to_string()),
    };
    // 전체 삭제 예약이 있으면 DB 를 열기 전에 지운다
    if paths.wipe_marker.exists() {
        match paths::perform_wipe(&paths) {
            Ok(_) => {
                notify::unregister();
                let _ = notify::set_autostart(false, std::path::Path::new(""));
            }
            Err(e) => fatal(&format!("데이터를 지우지 못했습니다: {e}")),
        }
    }
    if let Err(e) = paths.ensure() {
        fatal(&format!("사용자 데이터 폴더를 만들지 못했습니다 ({}): {e}", paths.root.display()));
    }
    logging::init(paths.logs_dir.clone());
    logging::info(&format!("시작: 버전 {}", env!("CARGO_PKG_VERSION")));
    let db = match db::Db::open(&paths.db_path) {
        Ok(d) => Arc::new(d),
        Err(e) => fatal(&format!("{e}\n\n데이터 위치: {}", paths.db_path.display())),
    };

    let paths_for_setup = paths.clone();
    let db_for_setup = db.clone();
    let state_slot: Arc<std::sync::Mutex<Option<Arc<AppState>>>> = Arc::new(std::sync::Mutex::new(None));
    let state_slot_setup = state_slot.clone();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let emit: app::Emitter = Arc::new(move |ev: &str, payload: serde_json::Value| {
                let _ = handle.emit(ev, payload);
            });
            let e1 = emit.clone();
            let runtime = Arc::new(runtime::Runtime::new(paths::runtime_dir(), move |s| e1("runtime-state", json!(s))));
            let e2 = emit.clone();
            let downloader = Arc::new(models::Downloader::new(move |p| e2("model-download", json!(p))));
            let (tx, rx) = app::new_queue();
            let state = Arc::new(AppState {
                paths: paths_for_setup.clone(),
                db: db_for_setup.clone(),
                runtime,
                downloader,
                queue: std::sync::Mutex::new(tx),
                emit,
                queued: Arc::new(std::sync::Mutex::new(Default::default())),
                shutting_down: Arc::new(AtomicBool::new(false)),
            });
            *state_slot_setup.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            app.manage(state.clone());

            // 설치 기록과 파일을 맞추고, 기기 사양은 처음 한 번만 읽는다
            let _ = state.db.with(|c| models::reconcile(c, &state.paths.models_dir));
            let needs_probe = state.db.with(|c| device::cached(c)).ok().flatten().is_none();
            if needs_probe {
                let st = state.clone();
                std::thread::spawn(move || {
                    let info = device::probe(&st.paths.root);
                    let _ = st.db.with(|c| device::store(c, &info));
                    (st.emit)("device-info", json!(info));
                });
            }
            // 알림 발신자 등록 (현재 사용자 레지스트리, 관리자 권한 불필요)
            let icon = state.paths.root.join("app-icon.png");
            let _ = std::fs::write(&icon, APP_ICON_PNG);
            if let Err(e) = notify::register(Some(&icon)) {
                logging::warn(&format!("알림 등록 실패: {e}"));
            }

            app::spawn_worker(state.clone(), rx);
            app::spawn_ticker(state.clone());
            if let Err(e) = app::recover_on_start(&state) {
                logging::warn(&format!("시작 복구 실패: {e}"));
            }

            let visible = !background && self_test_out.is_none();
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("로컬 AI 교무수첩")
                .inner_size(1280.0, 820.0)
                .min_inner_size(960.0, 620.0)
                .center()
                .visible(visible)
                .data_directory(state.paths.webview_dir.clone())
                .build()?;
            let _ = win;

            // 알림 영역(트레이) 아이콘: 창을 닫아도 알림을 받으려면 여기서 다시 열 수 있다
            let open = MenuItem::with_id(app, "open", "교무수첩 열기", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main-tray").tooltip("로컬 AI 교무수첩").menu(&menu).show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, ev| match ev.id().as_ref() {
                "open" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, ev| {
                if let tauri::tray::TrayIconEvent::DoubleClick { .. } = ev {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            // 패키징 검증: 내장 화면이 실제로 뜨는지 확인하고 결과를 파일로 남긴 뒤 종료한다
            if let Some(out) = self_test_out.clone() {
                let h = app.handle().clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let assets = h.asset_resolver().get("index.html".into()).is_some();
                    let started = std::time::Instant::now();
                    while !commands::FRONTEND_READY.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(40) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    let fe = commands::FRONTEND_READY.load(Ordering::SeqCst);
                    let fts5_ok = st
                        .db
                        .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"교무수첩\"'", [], |r| r.get::<_, i64>(0))?))
                        .is_ok();
                    let exe_dir = paths::exe_dir();
                    let report = SelfTestReport {
                        ok: assets && fe && fts5_ok,
                        version: env!("CARGO_PKG_VERSION").into(),
                        exe_dir: exe_dir.display().to_string(),
                        assets_embedded: assets,
                        frontend_loaded: fe,
                        runtime_found: st.runtime.runtime_dir().is_some(),
                        runtime_dir: st.runtime.runtime_dir().map(|p| p.display().to_string()),
                        data_root: st.paths.root.display().to_string(),
                        db_ok: true,
                        fts5_ok,
                        data_dir_is_outside_exe_dir: !st.paths.root.starts_with(&exe_dir),
                    };
                    let body = serde_json::to_string_pretty(&report).unwrap_or_default();
                    if !out.is_empty() {
                        let _ = std::fs::write(&out, &body);
                    }
                    println!("{body}");
                    h.exit(if report.ok { 0 } else { 2 });
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app
                    .try_state::<Arc<AppState>>()
                    .and_then(|s| s.db.with(|c| settings::load(c)).ok())
                    .map(|s| s.close_to_tray)
                    .unwrap_or(false);
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_note,
            commands::list_notes,
            commands::get_note_detail,
            commands::update_note_raw,
            commands::delete_note,
            commands::reclassify_note,
            commands::reclassify_unclassified,
            commands::suggest_plan,
            commands::save_plan,
            commands::resolve_date_expr,
            commands::list_students,
            commands::list_classes,
            commands::get_student_detail,
            commands::create_student,
            commands::update_student,
            commands::delete_student,
            commands::merge_students,
            commands::import_roster,
            commands::record_overview,
            commands::unassigned_observations,
            commands::create_observation,
            commands::update_observation,
            commands::delete_observation,
            commands::create_draft_template,
            commands::generate_draft_ai,
            commands::update_draft,
            commands::delete_draft,
            commands::check_draft_text,
            commands::list_duties,
            commands::create_duty,
            commands::update_duty,
            commands::delete_duty,
            commands::duty_board,
            commands::create_duty_log,
            commands::update_duty_log,
            commands::set_duty_log_done,
            commands::delete_duty_log,
            commands::get_duty_log,
            commands::list_tasks,
            commands::create_task,
            commands::update_task,
            commands::set_task_done,
            commands::delete_task,
            commands::get_task,
            commands::calendar,
            commands::get_event,
            commands::create_event,
            commands::update_event,
            commands::delete_event,
            commands::today_overview,
            commands::search,
            commands::model_catalog,
            commands::start_download,
            commands::pause_download,
            commands::cancel_download,
            commands::select_model,
            commands::delete_model,
            commands::verify_model,
            commands::import_model_file,
            commands::ai_available,
            commands::runtime_status,
            commands::runtime_start,
            commands::runtime_stop,
            commands::device_info,
            commands::app_info,
            commands::get_settings,
            commands::update_settings,
            commands::set_autostart,
            commands::test_notification,
            commands::open_folder,
            commands::list_backups,
            commands::backup_now,
            commands::request_wipe,
            commands::cancel_wipe,
            commands::restart_app,
            commands::frontend_ready,
            commands::show_main_window,
        ]);

    let app = match builder.build(tauri::generate_context!()) {
        Ok(a) => a,
        Err(e) => fatal(&format!("화면을 시작하지 못했습니다: {e}\n\nMicrosoft Edge WebView2 런타임이 설치되어 있는지 확인해 주세요.")),
    };
    app.run(move |_app, event| {
        if let RunEvent::Exit = event {
            if let Some(st) = state_slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                st.shutting_down.store(true, Ordering::SeqCst);
                let _ = st.queue.lock().unwrap_or_else(|e| e.into_inner()).send(app::Job::Shutdown);
                st.downloader.pause_all();
                st.runtime.stop();
                logging::info("종료: AI 엔진 정리 완료");
            }
        }
    });
}
