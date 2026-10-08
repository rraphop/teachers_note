//! 앱에 포함된 llama.cpp(llama-server) sidecar 수명주기.
//!
//! Stopped → Starting → Ready → Stopping → Stopped, 실패하면 Failed.
//! - 앱당 sidecar 하나, 추론은 한 번에 하나
//! - 필요할 때 켜고(lazy), 오래 쓰지 않으면 내린다(unload)
//! - 127.0.0.1 에만 연결을 열고, 실행마다 새 임시 인증키를 쓴다
//! - Windows Job Object 로 앱이 비정상 종료해도 자식 프로세스가 함께 끝난다
//! - sidecar 출력은 메모리에만 잠시 두고 파일로 남기지 않는다 (학생 이름·원문 보호)

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailKind {
    RuntimeMissing,
    ModelMissing,
    ModelCorrupt,
    OutOfMemory,
    PortConflict,
    CpuUnsupported,
    Crashed,
    Timeout,
    Request,
}

impl FailKind {
    pub fn message(&self) -> &'static str {
        match self {
            FailKind::RuntimeMissing => {
                "AI 실행 구성요소(runtime 폴더)를 찾지 못했습니다. 압축을 풀 때 runtime 폴더까지 함께 풀었는지 확인해 주세요."
            }
            FailKind::ModelMissing => "선택한 AI 모델 파일이 없습니다. 설정 > AI 모델에서 다시 설치해 주세요.",
            FailKind::ModelCorrupt => {
                "AI 모델 파일이 손상되었거나 이 프로그램과 맞지 않습니다. 설정 > AI 모델에서 '다시 검사'하거나 다시 설치해 주세요."
            }
            FailKind::OutOfMemory => {
                "메모리가 부족해 AI 모델을 불러오지 못했습니다. 다른 프로그램을 닫거나 설정에서 더 작은 모델을 골라 주세요."
            }
            FailKind::PortConflict => "AI 엔진이 쓸 내부 연결을 열지 못했습니다(다른 프로그램과 충돌). 잠시 후 다시 시도해 주세요.",
            FailKind::CpuUnsupported => "이 PC의 CPU에서는 AI 엔진을 실행할 수 없습니다.",
            FailKind::Crashed => "AI 엔진이 예기치 않게 종료되었습니다. 다시 시도하면 새로 시작합니다.",
            FailKind::Timeout => "AI 모델을 불러오는 데 너무 오래 걸립니다. 더 작은 모델을 쓰거나 잠시 후 다시 시도해 주세요.",
            FailKind::Request => "AI 엔진과 통신하지 못했습니다.",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting { model_id: String },
    Ready { model_id: String, port: u16, pid: u32 },
    Stopping,
    Failed { kind: FailKind, message: String },
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub kind: FailKind,
    pub message: String,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl RuntimeError {
    fn new(kind: FailKind) -> Self {
        RuntimeError { kind, message: kind.message().to_string() }
    }
    fn with(kind: FailKind, extra: &str) -> Self {
        RuntimeError { kind, message: format!("{} ({extra})", kind.message()) }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub model_id: String,
    pub model_path: PathBuf,
    pub size_bytes: u64,
    pub ctx: u32,
    pub threads: u32,
    /// AI 계산 장치: 자동(쓸 만한 그래픽카드가 있으면 GPU) 또는 CPU 만
    pub device: DeviceMode,
}

/// AI 계산 장치 설정
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeviceMode {
    /// 쓸 만한 그래픽카드(Vulkan)가 있으면 GPU, 없거나 실패하면 CPU
    #[default]
    Auto,
    /// 늘 CPU
    Cpu,
}

/// 엔진이 알려 준 그래픽카드 하나 (`llama-server --list-devices`)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GpuDevice {
    /// 엔진 안 이름 ("Vulkan0")
    pub id: String,
    pub name: String,
    pub total_mib: u64,
    pub free_mib: u64,
    /// CPU 에 붙은 내장 그래픽 (작은 모델은 CPU 보다 느릴 수 있어 고르지 않는다)
    pub integrated: bool,
}

/// `--list-devices` 출력에서 장치 목록을 읽는다 ("  Vulkan0: NVIDIA GeForce RTX 4070 (12282 MiB, 11000 MiB free)")
pub fn parse_devices(out: &str) -> Vec<GpuDevice> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*([A-Za-z]+\d+):\s*(.+?)\s*\((\d+)\s*MiB,\s*(\d+)\s*MiB free\)\s*$").unwrap());
    out.lines()
        .filter_map(|l| RE.captures(l))
        .map(|c| {
            let name = c[2].trim().to_string();
            GpuDevice { id: c[1].to_string(), integrated: is_integrated_gpu(&name), name, total_mib: c[3].parse().unwrap_or(0), free_mib: c[4].parse().unwrap_or(0) }
        })
        .collect()
}

/// 이름으로 본 내장 그래픽·소프트웨어 장치 (모델 번호 없이 "Graphics" 로 끝나는 것 등)
pub fn is_integrated_gpu(name: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)uhd graphics|iris|hd graphics|intel\(r\) graphics|radeon\(tm\) graphics|radeon graphics|vega \d+ graphics|arc\(tm\) graphics|microsoft basic|llvmpipe|swiftshader|lavapipe").unwrap()
    });
    RE.is_match(name.trim())
}

/// 모델을 통째로 올릴 수 있는 외장 그래픽카드 중 여유 메모리가 가장 큰 것 (없으면 CPU)
pub fn pick_gpu(devs: &[GpuDevice], model_bytes: u64) -> Option<GpuDevice> {
    let need_mib = (model_bytes as f64 * 1.2 / 1_048_576.0) as u64 + 1536;
    devs.iter().filter(|d| !d.integrated && d.free_mib >= need_mib).max_by_key(|d| d.free_mib).cloned()
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub port: u16,
    pub api_key: String,
    pub pid: u32,
}

/// sidecar 출력에서 실패 원인을 고른다
pub fn classify_failure(log: &[String], exit_code: Option<i32>) -> FailKind {
    let all = log.join("\n").to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| all.contains(n));
    match exit_code {
        Some(-1073741515) => return FailKind::RuntimeMissing, // STATUS_DLL_NOT_FOUND
        Some(-1073741795) => return FailKind::CpuUnsupported, // STATUS_ILLEGAL_INSTRUCTION
        _ => {}
    }
    if has(&["couldn't bind", "could not bind", "failed to bind", "address already in use", "bind failed", "error while binding"]) {
        FailKind::PortConflict
    } else if has(&["out of memory", "failed to allocate", "bad_alloc", "cannot allocate", "not enough memory", "unable to allocate"]) {
        FailKind::OutOfMemory
    } else if has(&[
        "failed to load model",
        "error loading model",
        "invalid magic",
        "gguf_init",
        "not within the file bounds",
        "unable to load model",
        "unknown model architecture",
        "failed to read",
        "corrupted",
    ]) {
        FailKind::ModelCorrupt
    } else {
        FailKind::Crashed
    }
}

/// `llama-server --list-devices` 로 그래픽카드를 묻는다 (창 없이, 15초 안에)
fn probe_devices(dir: &std::path::Path) -> Vec<GpuDevice> {
    let exe = dir.join(crate::paths::server_exe_name());
    let mut cmd = Command::new(&exe);
    cmd.current_dir(dir).arg("--list-devices").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = cmd.spawn() else { return vec![] };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < Duration::from_secs(15) => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return vec![];
            }
        }
    }
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut o, &mut out);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut e, &mut out);
    }
    parse_devices(&out)
}

fn free_port() -> Option<u16> {
    TcpListener::bind(("127.0.0.1", 0)).ok().and_then(|l| l.local_addr().ok()).map(|a| a.port())
}

fn new_api_key() -> String {
    let mut b = [0u8; 24];
    if getrandom::fill(&mut b).is_err() {
        // 난수를 못 얻는 경우는 거의 없지만, 시각 기반 값이라도 쓴다
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b[..16].copy_from_slice(&t.to_le_bytes());
    }
    hex::encode(b)
}

fn local_client(timeout: Duration) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .no_proxy() // 이 PC 안의 AI 엔진과만 통신하므로 프록시를 거치지 않는다
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("http client")
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows::core::PCWSTR;

    /// 이 핸들이 닫히면 (앱이 비정상 종료해도) 안의 프로세스가 함께 끝난다
    pub struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn attach(child: &std::process::Child) -> Option<Job> {
            unsafe {
                let h = CreateJobObjectW(None, PCWSTR::null()).ok()?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok.is_err() {
                    let _ = CloseHandle(h);
                    return None;
                }
                if AssignProcessToJobObject(h, HANDLE(child.as_raw_handle())).is_err() {
                    let _ = CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(not(windows))]
mod job {
    pub struct Job;
    impl Job {
        pub fn attach(_c: &std::process::Child) -> Option<Job> {
            Some(Job)
        }
    }
}

struct Proc {
    child: Child,
    _job: Option<job::Job>,
    port: u16,
    api_key: String,
    model_id: String,
}

pub struct Runtime {
    /// 엔진이 알려 준 그래픽카드 (처음 켤 때 한 번 묻는다)
    devices: Mutex<Option<Vec<GpuDevice>>>,
    /// GPU 로 켜다 실패했으면 이번 실행 동안은 CPU 만 쓴다
    gpu_failed: std::sync::atomic::AtomicBool,
    /// 지금 엔진이 쓰는 장치 ("CPU" 또는 그래픽카드 이름)
    active_device: Mutex<Option<String>>,
    state: Mutex<RuntimeState>,
    proc: Mutex<Option<Proc>>,
    infer: Mutex<()>,
    last_used: Mutex<Instant>,
    log_tail: Arc<Mutex<VecDeque<String>>>,
    on_change: Box<dyn Fn(&RuntimeState) + Send + Sync>,
    runtime_dir: Mutex<Option<PathBuf>>,
}

const LOG_TAIL: usize = 120;

impl Runtime {
    pub fn new(runtime_dir: Option<PathBuf>, on_change: impl Fn(&RuntimeState) + Send + Sync + 'static) -> Runtime {
        Runtime {
            devices: Mutex::new(None),
            gpu_failed: std::sync::atomic::AtomicBool::new(false),
            active_device: Mutex::new(None),
            state: Mutex::new(RuntimeState::Stopped),
            proc: Mutex::new(None),
            infer: Mutex::new(()),
            last_used: Mutex::new(Instant::now()),
            log_tail: Arc::new(Mutex::new(VecDeque::new())),
            on_change: Box::new(on_change),
            runtime_dir: Mutex::new(runtime_dir),
        }
    }

    pub fn runtime_dir(&self) -> Option<PathBuf> {
        self.runtime_dir.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn state(&self) -> RuntimeState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_state(&self, s: RuntimeState) {
        {
            let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if *g == s {
                return;
            }
            *g = s.clone();
        }
        (self.on_change)(&s);
    }

    /// 지금 엔진이 쓰는 계산 장치 ("CPU" 또는 그래픽카드 이름, 엔진이 꺼져 있으면 None)
    pub fn active_device(&self) -> Option<String> {
        if self.proc.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            return None;
        }
        self.active_device.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 엔진에게 쓸 수 있는 그래픽카드를 묻는다 (한 번만, 15초 안에 답이 없으면 없음으로 본다)
    pub fn devices(&self) -> Vec<GpuDevice> {
        if let Some(d) = self.devices.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return d;
        }
        let found = self.runtime_dir().map(|dir| probe_devices(&dir)).unwrap_or_default();
        crate::logging::info(&format!(
            "AI 계산 장치: {}",
            if found.is_empty() { "그래픽카드 없음 (CPU)".to_string() } else { found.iter().map(|d| format!("{} {}MiB{}", d.name, d.free_mib, if d.integrated { " 내장" } else { "" })).collect::<Vec<_>>().join(", ") }
        ));
        *self.devices.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
        found
    }

    /// 이미 물어본 그래픽카드 목록 (묻지 않았으면 빈 목록 — 상태 표시용, 엔진을 새로 띄우지 않는다)
    pub fn cached_devices(&self) -> Vec<GpuDevice> {
        self.devices.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
    }

    /// 이번에 켤 장치: 자동이면 모델을 올릴 수 있는 외장 그래픽카드, 아니면 CPU
    fn choose_gpu(&self, cfg: &LaunchConfig) -> Option<GpuDevice> {
        if cfg.device == DeviceMode::Cpu || self.gpu_failed.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        pick_gpu(&self.devices(), cfg.size_bytes)
    }

    /// 엔진이 추론 중에 죽었을 때: GPU 를 쓰고 있었으면 이번 실행 동안은 CPU 로 켠다
    fn note_gpu_crash(&self) {
        let dev = self.active_device.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if dev.as_deref().is_some_and(|d| d != "CPU") {
            crate::logging::warn("GPU 로 돌던 AI 엔진이 멈춰 다음부터 CPU 로 켭니다.");
            self.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn loaded_model(&self) -> Option<String> {
        self.proc.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| p.model_id.clone())
    }

    pub fn pid(&self) -> Option<u32> {
        self.proc.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| p.child.id())
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    /// 필요하면 sidecar 를 켜고, 준비되면 연결 정보를 돌려준다.
    pub fn ensure_ready(&self, cfg: &LaunchConfig) -> Result<Endpoint, RuntimeError> {
        let mut guard = self.proc.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = guard.as_mut() {
            let alive = matches!(p.child.try_wait(), Ok(None));
            if alive && p.model_id == cfg.model_id {
                self.touch();
                return Ok(Endpoint { port: p.port, api_key: p.api_key.clone(), pid: p.child.id() });
            }
        }
        // 다른 모델이 올라와 있거나 엔진이 비정상 종료했으면 정리하고 새로 시작한다
        if let Some(mut old) = guard.take() {
            let _ = old.child.kill();
            let _ = old.child.wait();
        }
        let mut gpu = self.choose_gpu(cfg);
        let mut port_retry = true;
        let e = loop {
            match self.spawn(cfg, gpu.as_ref()) {
                Ok(p) => {
                    let ep = Endpoint { port: p.port, api_key: p.api_key.clone(), pid: p.child.id() };
                    *guard = Some(p);
                    let dev = gpu.as_ref().map(|g| g.name.clone()).unwrap_or_else(|| "CPU".into());
                    crate::logging::info(&format!("AI 엔진 시작: {dev}"));
                    *self.active_device.lock().unwrap_or_else(|e| e.into_inner()) = Some(dev);
                    self.touch();
                    self.set_state(RuntimeState::Ready { model_id: cfg.model_id.clone(), port: ep.port, pid: ep.pid });
                    return Ok(ep);
                }
                // 포트 충돌은 다른 포트로 한 번 더 시도한다
                Err(e) if e.kind == FailKind::PortConflict && port_retry => port_retry = false,
                Err(e) => {
                    // GPU 로 켜다 실패하면(드라이버·메모리 등) 이번 실행 동안은 CPU 로 바꿔 다시 켠다
                    if !matches!(e.kind, FailKind::ModelMissing | FailKind::RuntimeMissing) {
                        if let Some(g) = gpu.take() {
                            crate::logging::warn(&format!("GPU({})로 AI 엔진을 켜지 못해 CPU 로 다시 켭니다: {}", g.name, e.message));
                            self.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed);
                            continue;
                        }
                    }
                    break e;
                }
            }
        };
        self.set_state(RuntimeState::Failed { kind: e.kind, message: e.message.clone() });
        Err(e)
    }

    fn spawn(&self, cfg: &LaunchConfig, gpu: Option<&GpuDevice>) -> Result<Proc, RuntimeError> {
        let Some(dir) = self.runtime_dir() else { return Err(RuntimeError::new(FailKind::RuntimeMissing)) };
        let exe = dir.join(crate::paths::server_exe_name());
        if !exe.is_file() {
            return Err(RuntimeError::new(FailKind::RuntimeMissing));
        }
        if !cfg.model_path.is_file() {
            return Err(RuntimeError::new(FailKind::ModelMissing));
        }
        self.set_state(RuntimeState::Starting { model_id: cfg.model_id.clone() });
        self.log_tail.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let port = free_port().ok_or_else(|| RuntimeError::new(FailKind::PortConflict))?;
        let api_key = new_api_key();
        let mut cmd = Command::new(&exe);
        cmd.current_dir(&dir)
            .arg("--model")
            .arg(&cfg.model_path)
            .args(["--host", "127.0.0.1"])
            .args(["--port", &port.to_string()])
            .args(["--api-key", &api_key])
            .args(["--ctx-size", &cfg.ctx.to_string()])
            .args(["--parallel", "1"])
            .args(["--threads", &cfg.threads.to_string()])
            // GPU: 모델 전체를 그 그래픽카드 하나에 올린다 / CPU: 그래픽카드를 아예 쓰지 않는다
            .args(match gpu {
                Some(g) => ["--device".to_string(), g.id.clone(), "--n-gpu-layers".to_string(), "999".to_string()],
                None => ["--device".to_string(), "none".to_string(), "--n-gpu-layers".to_string(), "0".to_string()],
            })
            .args(["--cache-ram", "0"])
            .args(["--reasoning", "off"])
            .arg("--no-webui")
            .arg("--no-slots")
            .arg("--offline")
            .arg("--no-warmup")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| RuntimeError::with(FailKind::RuntimeMissing, &e.to_string()))?;
        let job = job::Job::attach(&child);
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)]
            .into_iter()
            .flatten()
        {
            let tail = self.log_tail.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.split(b'\n').map_while(Result::ok) {
                    let line = String::from_utf8_lossy(&line).trim_end().to_string();
                    let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    if t.len() >= LOG_TAIL {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }
        let mut proc = Proc { child, _job: job, port, api_key, model_id: cfg.model_id.clone() };

        // 준비될 때까지 기다린다 (모델 크기에 따라 넉넉히)
        let gb = (cfg.size_bytes as f64 / 1e9).ceil() as u64;
        let limit = Duration::from_secs((60 + 20 * gb).min(300));
        let started = Instant::now();
        let client = local_client(Duration::from_secs(3));
        let url = format!("http://127.0.0.1:{port}/health");
        loop {
            if let Ok(Some(status)) = proc.child.try_wait() {
                std::thread::sleep(Duration::from_millis(200));
                let log: Vec<String> = self.log_tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
                let kind = classify_failure(&log, status.code());
                return Err(RuntimeError::new(kind));
            }
            if let Ok(resp) = client.get(&url).bearer_auth(&proc.api_key).send() {
                if resp.status().is_success() {
                    return Ok(proc);
                }
            }
            if started.elapsed() > limit {
                let _ = proc.child.kill();
                let _ = proc.child.wait();
                return Err(RuntimeError::new(FailKind::Timeout));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// OpenAI 호환 대화 API 호출. 추론은 한 번에 하나만 한다.
    pub fn chat(&self, cfg: &LaunchConfig, body: serde_json::Value, timeout: Duration) -> Result<String, RuntimeError> {
        let _one = self.infer.lock().unwrap_or_else(|e| e.into_inner());
        let ep = self.ensure_ready(cfg)?;
        let client = local_client(timeout);
        let res = client
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", ep.port))
            .bearer_auth(&ep.api_key)
            .json(&body)
            .send();
        self.touch();
        let resp = match res {
            Ok(r) => r,
            Err(e) => {
                // 엔진이 죽었는지 확인
                let dead = {
                    let mut g = self.proc.lock().unwrap_or_else(|e| e.into_inner());
                    match g.as_mut() {
                        Some(p) => !matches!(p.child.try_wait(), Ok(None)),
                        None => true,
                    }
                };
                if dead {
                    self.note_gpu_crash();
                    let log: Vec<String> = self.log_tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
                    let kind = classify_failure(&log, None);
                    self.stop();
                    let err = RuntimeError::new(kind);
                    self.set_state(RuntimeState::Failed { kind, message: err.message.clone() });
                    return Err(err);
                }
                let what = if e.is_timeout() { "응답 시간이 초과되었습니다" } else { "연결 오류" };
                return Err(RuntimeError::with(FailKind::Request, what));
            }
        };
        let status = resp.status();
        let v: serde_json::Value = resp.json().map_err(|_| RuntimeError::with(FailKind::Request, "응답 형식 오류"))?;
        if !status.is_success() {
            let too_long = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.to_lowercase().contains("context"));
            let what = if too_long { "기록이 너무 길어 한 번에 처리하지 못했습니다".to_string() } else { format!("HTTP {}", status.as_u16()) };
            return Err(RuntimeError::with(FailKind::Request, &what));
        }
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| RuntimeError::with(FailKind::Request, "빈 응답"))
    }

    /// `chat` 과 같되 답을 조금씩(스트리밍) 받으며, 지금까지 받은 글자 수를 `on_chars` 로 알린다 (AI 정리 진행 표시용)
    pub fn chat_stream(&self, cfg: &LaunchConfig, mut body: serde_json::Value, timeout: Duration, on_chars: &dyn Fn(usize)) -> Result<String, RuntimeError> {
        use std::io::BufRead;
        let _one = self.infer.lock().unwrap_or_else(|e| e.into_inner());
        let ep = self.ensure_ready(cfg)?;
        body["stream"] = serde_json::Value::Bool(true);
        let client = local_client(timeout);
        let res = client
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", ep.port))
            .bearer_auth(&ep.api_key)
            .json(&body)
            .send();
        self.touch();
        let resp = match res {
            Ok(r) => r,
            Err(e) => {
                let dead = {
                    let mut g = self.proc.lock().unwrap_or_else(|e| e.into_inner());
                    match g.as_mut() {
                        Some(p) => !matches!(p.child.try_wait(), Ok(None)),
                        None => true,
                    }
                };
                if dead {
                    self.note_gpu_crash();
                    let log: Vec<String> = self.log_tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
                    let kind = classify_failure(&log, None);
                    self.stop();
                    let err = RuntimeError::new(kind);
                    self.set_state(RuntimeState::Failed { kind, message: err.message.clone() });
                    return Err(err);
                }
                let what = if e.is_timeout() { "응답 시간이 초과되었습니다" } else { "연결 오류" };
                return Err(RuntimeError::with(FailKind::Request, what));
            }
        };
        let status = resp.status();
        if !status.is_success() {
            let v: serde_json::Value = resp.json().unwrap_or_default();
            let too_long = v.pointer("/error/message").and_then(|m| m.as_str()).is_some_and(|m| m.to_lowercase().contains("context"));
            let what = if too_long { "기록이 너무 길어 한 번에 처리하지 못했습니다".to_string() } else { format!("HTTP {}", status.as_u16()) };
            return Err(RuntimeError::with(FailKind::Request, &what));
        }
        // 서버가 보내는 줄: "data: {…delta…}" … "data: [DONE]"
        let mut out = String::new();
        let mut chars = 0usize;
        for line in std::io::BufReader::new(resp).lines() {
            let line = line.map_err(|e| {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    RuntimeError::with(FailKind::Request, "응답 시간이 초과되었습니다")
                } else {
                    RuntimeError::with(FailKind::Request, "응답을 읽다 끊겼습니다")
                }
            })?;
            let Some(data) = line.trim().strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(piece) = v.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
                out.push_str(piece);
                chars += piece.chars().count();
                on_chars(chars);
            }
            self.touch();
        }
        if out.trim().is_empty() {
            return Err(RuntimeError::with(FailKind::Request, "빈 응답"));
        }
        Ok(out)
    }

    pub fn stop(&self) {
        let taken = self.proc.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(mut p) = taken {
            self.set_state(RuntimeState::Stopping);
            let _ = p.child.kill();
            let _ = p.child.wait();
        }
        self.set_state(RuntimeState::Stopped);
    }

    /// 오래 쓰지 않았으면 메모리에서 내린다
    pub fn unload_if_idle(&self, idle: Duration) -> bool {
        if idle.is_zero() {
            return false;
        }
        let Ok(_busy) = self.infer.try_lock() else { return false };
        let loaded = self.proc.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let elapsed = self.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
        if loaded && elapsed > idle {
            drop(_busy);
            self.stop();
            return true;
        }
        false
    }

    /// 최근 엔진 출력 (진단용, 화면에만 보여 주고 저장하지 않는다)
    pub fn diagnostics(&self) -> Vec<String> {
        self.log_tail.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().take(20).rev().cloned().collect()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(mut p) = self.proc.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = p.child.kill();
            let _ = p.child.wait();
        }
    }
}

/// sidecar 가 어떤 주소에서 연결을 기다리는지 확인한다 (127.0.0.1 전용인지 검증)
#[cfg(windows)]
pub fn listening_addresses(pid: u32) -> Vec<String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    let mut out = Vec::new();
    unsafe {
        for af in [AF_INET.0 as u32, AF_INET6.0 as u32] {
            let mut size = 0u32;
            let _ = GetExtendedTcpTable(None, &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0);
            if size == 0 {
                continue;
            }
            let mut buf = vec![0u8; size as usize + 64];
            if GetExtendedTcpTable(Some(buf.as_mut_ptr() as *mut _), &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0) != 0 {
                continue;
            }
            if af == AF_INET.0 as u32 {
                let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
                let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
                for r in rows.iter().filter(|r| r.dwOwningPid == pid) {
                    let ip = std::net::Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes());
                    let port = u16::from_be((r.dwLocalPort & 0xffff) as u16);
                    out.push(format!("{ip}:{port}"));
                }
            } else {
                let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
                let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
                for r in rows.iter().filter(|r| r.dwOwningPid == pid) {
                    let ip = std::net::Ipv6Addr::from(r.ucLocalAddr);
                    let port = u16::from_be((r.dwLocalPort & 0xffff) as u16);
                    out.push(format!("[{ip}]:{port}"));
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn listening_addresses(_pid: u32) -> Vec<String> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_kinds() {
        let l = |s: &str| vec![s.to_string()];
        assert_eq!(classify_failure(&l("llama_model_load: error loading model: invalid magic"), Some(1)), FailKind::ModelCorrupt);
        assert_eq!(classify_failure(&l("ggml_backend_cpu_buffer_type_alloc_buffer: failed to allocate buffer"), Some(1)), FailKind::OutOfMemory);
        assert_eq!(classify_failure(&l("couldn't bind HTTP server socket, hostname: 127.0.0.1"), Some(1)), FailKind::PortConflict);
        assert_eq!(classify_failure(&[], Some(-1073741515)), FailKind::RuntimeMissing);
        assert_eq!(classify_failure(&l("segfault"), Some(3)), FailKind::Crashed);
    }

    #[test]
    fn missing_runtime_is_reported() {
        let rt = Runtime::new(None, |_| {});
        let cfg = LaunchConfig { model_id: "x".into(), model_path: "nope.gguf".into(), size_bytes: 1, ctx: 2048, threads: 2, device: DeviceMode::Cpu };
        let e = rt.ensure_ready(&cfg).unwrap_err();
        assert_eq!(e.kind, FailKind::RuntimeMissing);
        assert!(matches!(rt.state(), RuntimeState::Failed { kind: FailKind::RuntimeMissing, .. }));
    }

    #[test]
    fn api_keys_are_random() {
        assert_ne!(new_api_key(), new_api_key());
        assert_eq!(new_api_key().len(), 48);
    }

    #[test]
    fn picks_discrete_gpu_with_room() {
        let out = "ggml_vulkan: Found 2 Vulkan devices:\nAvailable devices:\n  Vulkan0: NVIDIA GeForce RTX 4070 Ti SUPER (16061 MiB, 15293 MiB free)\n  Vulkan1: AMD Radeon(TM) Graphics (16171 MiB, 15363 MiB free)\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 2);
        assert!(!d[0].integrated && d[1].integrated, "내장 그래픽은 고르지 않는다");
        let four_b = 2_740_937_888;
        assert_eq!(pick_gpu(&d, four_b).map(|g| g.id), Some("Vulkan0".into()));
        // 여유 메모리가 모자라면 CPU
        let small = parse_devices("  Vulkan0: NVIDIA GeForce GTX 1050 (2048 MiB, 1800 MiB free)");
        assert_eq!(pick_gpu(&small, four_b), None);
        // 내장 그래픽뿐이면 CPU
        let igpu = parse_devices("  Vulkan0: Intel(R) UHD Graphics 770 (8000 MiB, 7900 MiB free)");
        assert!(igpu[0].integrated && pick_gpu(&igpu, four_b).is_none());
        assert!(parse_devices("Available devices:\n").is_empty());
    }
}
