pub mod analysis;
pub mod app;
pub mod archive;
pub mod backup;
pub mod briefing;
pub mod classify;
pub mod commands;
pub mod dates;
pub mod db;
pub mod device;
pub mod duties;
pub mod error;
pub mod export;
pub mod llm;
pub mod logging;
pub mod memos;
pub mod models;
pub mod neis;
pub mod neis_roster;
pub mod notes;
pub mod notify;
pub mod paths;
pub mod pdf;
pub mod profile;
pub mod records;
pub mod reminders;
pub mod runtime;
pub mod schedule;
pub mod search;
pub mod seen;
pub mod settings;
pub mod students;
pub mod text;
pub mod timetable;
pub mod today_layout;
pub mod ttinput;
pub mod util;
pub mod window_fx;

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
    // 백업 되돌리기 예약이 있으면 DB 를 열기 전에 바꿔 넣는다 (실패하면 지금 데이터 그대로)
    if let Some(outcome) = backup::perform_restore(&paths) {
        *backup::LAST_RESTORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
    }
    let db = match db::Db::open_with_upgrade_backup(&paths.db_path, &paths.backups_dir) {
        Ok(d) => Arc::new(d),
        Err(e) => fatal(&format!("{e}\n\n데이터 위치: {}", paths.db_path.display())),
    };

    let paths_for_setup = paths.clone();
    let db_for_setup = db.clone();
    let state_slot: Arc<std::sync::Mutex<Option<Arc<AppState>>>> = Arc::new(std::sync::Mutex::new(None));
    let state_slot_setup = state_slot.clone();

    // 한 번만 실행: 두 번째로 실행하면 이미 열린 창을 앞으로 가져온다.
    // 자동 시험(TN_DATA_ROOT 로 따로 만든 데이터 폴더)에서는 교사가 쓰고 있는 창을 건드리지 않도록 끈다.
    let test_data_root = std::env::var_os("TN_DATA_ROOT").is_some_and(|v| !v.is_empty());
    let builder = tauri::Builder::default();
    let builder = if test_data_root {
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
    };
    let builder = builder
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
            // 나이스: 학교를 정했으면 하루 한 번 시간표·학사일정을 새로 받고 이번 주 급식을 받아 둔다 (학교 코드만 보낸다)
            if self_test_out.is_none() {
                let st = state.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    if crate::neis::auto_sync(&st.db, crate::util::now(), &crate::neis::http_get) {
                        st.changed("all");
                    }
                });
            }
            // 자동 시작을 켜 두었으면, 파일 하나 버전으로 켤 때마다 지금 켠 파일("교무수첩_1.0.0.exe")을 가리키게 한다
            // (새 버전 파일로 바꾸거나 지난 버전 파일을 지워도 자동 시작이 끊기지 않게).
            // 예전 모양(`--background`: 알림 영역에만 켜기)이면 창을 띄우는 모양으로 고쳐 쓴다.
            if let Some(cur) = notify::autostart_target() {
                let exe = paths::launcher_path().unwrap_or_else(|| std::path::PathBuf::from(notify::autostart_exe(&cur)));
                if notify::autostart_command(&exe).to_lowercase() != cur.trim().to_lowercase() {
                    let _ = notify::set_autostart(true, &exe);
                }
            }

            app::spawn_worker(state.clone(), rx);
            app::spawn_ticker(state.clone());
            if let Err(e) = app::recover_on_start(&state) {
                logging::warn(&format!("시작 복구 실패: {e}"));
            }

            let visible = !background && self_test_out.is_none();
            let (on_top, transparency) = state.db.with(|c| settings::load(c)).map(|s| (s.always_on_top, s.window_transparency)).unwrap_or((false, 0));
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("로컬 AI 교무수첩")
                .inner_size(1280.0, 820.0)
                .min_inner_size(960.0, 620.0)
                .center()
                .always_on_top(on_top)
                .visible(visible)
                .data_directory(state.paths.webview_dir.clone())
                .build()?;
            // 지난번에 정한 창 투명도 (여기는 창을 만든 메인 스레드)
            #[cfg(windows)]
            if transparency > 0 {
                if let Ok(h) = win.hwnd() {
                    if let Err(e) = window_fx::apply(h.0 as isize, transparency) {
                        logging::warn(&e);
                    }
                }
            }
            #[cfg(not(windows))]
            let _ = (win, transparency);

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
            // 최대화·최소화·포커스 등으로 창 틀이 스타일을 다시 쓰면 투명도가 풀린다 → 되살린다
            #[cfg(windows)]
            if matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_) | WindowEvent::ScaleFactorChanged { .. }) && window_fx::current() > 0 {
                if let Ok(h) = window.hwnd() {
                    window_fx::ensure(h.0 as isize);
                }
            }
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
            commands::import_neis_roster,
            commands::license_texts,
            commands::set_note_track,
            commands::neis_search_schools,
            commands::neis_status,
            commands::neis_sync,
            commands::get_meals,
            commands::timetable_colors,
            commands::set_timetable_color,
            commands::today_briefing,
            commands::refresh_briefing,
            commands::student_record,
            commands::record_status,
            commands::record_gather,
            commands::record_ai_draft,
            commands::save_record_text,
            commands::check_record_text,
            commands::export_record_finals,
            commands::unassigned_observations,
            commands::create_observation,
            commands::update_observation,
            commands::delete_observation,
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
            commands::set_student_field,
            commands::current_semester,
            commands::get_bell,
            commands::compute_bell,
            commands::save_bell,
            commands::get_timetable,
            commands::class_timetables,
            commands::set_timetable_cell,
            commands::add_school_row,
            commands::delete_timetable,
            commands::copy_semester,
            commands::today_timetable,
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
            commands::set_always_on_top,
            commands::resolve_profile_change,
            commands::answer_note_question,
            commands::set_student_contacts,
            commands::mark_seen,
            commands::new_since,
            commands::set_window_transparency,
            commands::list_memos,
            commands::create_memo,
            commands::update_memo,
            commands::delete_memo,
            commands::restore_memo,
            commands::shift_memo,
            commands::get_today_layout,
            commands::set_today_layout,
            commands::test_notification,
            commands::open_folder,
            commands::list_backups,
            commands::backup_now,
            commands::inspect_backup,
            commands::request_restore,
            commands::take_restore_outcome,
            commands::export_data,
            commands::reveal_export,
            commands::tt_conflicts,
            commands::resolve_tt_conflict,
            commands::school_years,
            commands::make_year_archive,
            commands::new_year_preview,
            commands::start_new_year,
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
                st.db.close();
                logging::info("종료: AI 엔진 정리·DB 닫기 완료");
            }
        }
    });
}
