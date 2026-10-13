import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { ErrorBox, ReminderPicker } from "../components/Fields";
import { IconDownload, IconFolder, IconRefresh, IconShield, IconTrash } from "../components/Icons";
import { ExportPanel, pickFolder } from "../components/ExportTools";
import { YearPanel } from "../components/YearTools";
import { SchoolPanel } from "../components/SchoolSettings";
import { confirmDialog, Modal } from "../components/Modal";
import { bytes, fmtDateTime, gib, percent } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { AppSettings, BackupCounts, BackupFile, CatalogEntry, DeviceInfo, DlProgress, MirrorStatus, RuntimeStatus } from "../types";

function backupKind(kind: string): string {
  if (kind.startsWith("auto")) return "자동";
  if (kind.startsWith("before-restore")) return "되돌리기 전";
  if (kind.startsWith("new-year")) return "새 학년도 전";
  if (kind.startsWith("before-update")) return "업데이트 전";
  return "직접";
}

/** 두 번째 백업 위치 (1.0.1): 다른 드라이브·폴더에도 백업 사본을 둔다 */
function BackupMirror({ onChanged }: { onChanged: () => void }) {
  const { toast } = useStore();
  const st = useLoad(() => api.backupMirrorStatus(), []);
  const [busy, setBusy] = useState(false);
  const set = async (dir: string | null) => {
    setBusy(true);
    try {
      const r: MirrorStatus = await api.setBackupMirror(dir);
      toast(dir ? `두 번째 백업 위치를 정하고 백업 ${r.count}개를 복사했습니다.` : "두 번째 백업 위치를 그만 씁니다. 이미 복사한 파일은 그대로 둡니다.");
      st.reload();
      onChanged();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const pick = async () => {
    const dir = await pickFolder("두 번째 백업 위치 고르기 (D 드라이브 같은 다른 드라이브 권장)").catch(() => null);
    if (dir) await set(dir);
  };
  const m = st.data;
  return (
    <div className="mirror-box" data-testid="backup-mirror">
      <h3>두 번째 백업 위치</h3>
      <p className="small muted">
        기본 백업은 이 PC의 사용자 폴더(보통 C 드라이브)에 저장됩니다. 학교 PC처럼 드라이브가 나뉘어 있으면 D 드라이브 같은 다른 곳을 정해 두세요. 백업할 때마다 그곳에도
        사본을 저장하고(자동 백업은 최근 14개), C 드라이브가 초기화돼도 아래 목록에서 그 백업으로 되돌릴 수 있습니다.
      </p>
      {m?.dir ? (
        <p className="small">
          지금 위치: <code>{m.folder}</code> · 백업 {m.count}개{m.last_ok && ` · 마지막 복사 ${fmtDateTime(m.last_ok)}`}
        </p>
      ) : (
        <p className="small">정하지 않음</p>
      )}
      {m?.dir && !m.available && <p className="hint warn">그 위치를 찾지 못했습니다. 드라이브나 USB가 연결돼 있는지 확인해 주세요. 연결되면 다음 백업 때 다시 복사합니다.</p>}
      {m?.dir && m.available && m.last_error && <p className="hint warn">마지막 복사가 실패했습니다: {m.last_error}</p>}
      {m?.same_drive && <p className="hint warn">데이터와 같은 드라이브입니다. 드라이브가 고장 나거나 초기화되면 함께 사라지므로 다른 드라이브를 권합니다.</p>}
      <div className="row gap wrap">
        <button onClick={() => void pick()} disabled={busy}>
          <IconFolder size={15} /> {m?.dir ? "위치 바꾸기" : "폴더 고르기"}
        </button>
        {m?.dir && (
          <>
            <button onClick={() => void api.openFolder("mirror").catch((e) => toast(errorText(e), "error"))}>열기</button>
            <button
              onClick={() =>
                void confirmDialog("두 번째 백업 위치를 그만 쓸까요? 이미 복사한 백업 파일은 지우지 않습니다.", { okText: "그만 쓰기" }).then((ok) => {
                  if (ok) void set(null);
                })
              }
              disabled={busy}
            >
              그만 쓰기
            </button>
          </>
        )}
      </div>
    </div>
  );
}

/** 백업 되돌리기 확인: 백업 안의 양을 보여 주고, 지금 데이터는 먼저 백업된다는 것을 알린다 */
function RestoreDialog({ backup, onClose }: { backup: BackupFile; onClose: () => void }) {
  const [counts, setCounts] = useState<BackupCounts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api
      .inspectBackup(backup.name, backup.location)
      .then(setCounts)
      .catch((e) => setError(errorText(e)));
  }, [backup.name, backup.location]);
  const go = async () => {
    setBusy(true);
    try {
      await api.requestRestore(backup.name, backup.location);
      await api.restartApp();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };
  return (
    <Modal
      title="백업으로 되돌리기"
      onClose={onClose}
      footer={
        <>
          <button onClick={onClose}>취소</button>
          <button className="danger" onClick={() => void go()} disabled={!counts || busy}>
            {busy ? "다시 시작하는 중…" : "되돌리고 다시 시작"}
          </button>
        </>
      }
    >
      <p>
        <b>{backup.taken_at}</b> ({backupKind(backup.kind)}) 백업으로 되돌립니다.
      </p>
      {!counts && !error && <p className="muted">백업 파일을 검사하는 중…</p>}
      {error && <p className="error-box">{error}</p>}
      {counts && (
        <ul className="export-counts">
          <li>
            기록 <b>{counts.notes}</b>개
          </li>
          <li>
            학생 <b>{counts.students}</b>명
          </li>
          <li>
            학생 관찰 <b>{counts.observations}</b>개
          </li>
          <li>
            할 일 <b>{counts.tasks}</b>개
          </li>
          <li>
            일정 <b>{counts.events}</b>개
          </li>
          <li>
            시간표 칸 <b>{counts.timetable_cells}</b>개
          </li>
        </ul>
      )}
      <p className="small muted">
        이 백업 뒤에 적은 내용은 화면에서 사라집니다. 하지만 지금 데이터는 되돌리기 직전에 <b>‘되돌리기 전’ 백업</b>으로 먼저 저장되므로, 마음이 바뀌면 그 백업으로 다시 돌아올 수 있습니다. 프로그램이 다시 시작됩니다.
      </p>
    </Modal>
  );
}

function useLiveProgress(): Record<string, DlProgress> {
  const [m, setM] = useState<Record<string, DlProgress>>({});
  useEffect(() => {
    let un: (() => void) | null = null;
    let alive = true;
    void listen<DlProgress>("model-download", (e) => setM((x) => ({ ...x, [e.payload.model_id]: e.payload }))).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, []);
  return m;
}

function DeviceCard({ device, onRefresh, busy }: { device: DeviceInfo | null; onRefresh: () => void; busy: boolean }) {
  return (
    <div className="device-card">
      <div className="row gap">
        <h3 className="grow">이 PC 사양</h3>
        <button onClick={onRefresh} disabled={busy}>
          <IconRefresh size={15} /> {busy ? "읽는 중…" : "기기정보 다시 읽기"}
        </button>
      </div>
      {!device ? (
        <p className="muted">처음 실행이라 사양을 읽고 있습니다…</p>
      ) : (
        <>
          <dl className="spec">
            <dt>CPU</dt>
            <dd>
              {device.cpu_name} ({device.physical_cores}코어 / {device.logical_cores}스레드)
            </dd>
            <dt>메모리</dt>
            <dd>
              {gib(device.ram_total_bytes)} (확인 당시 여유 {gib(device.ram_available_bytes)})
            </dd>
            <dt>그래픽</dt>
            <dd>
              {device.gpus.length === 0
                ? "확인되지 않음"
                : device.gpus.map((g) => `${g.name}${g.integrated ? " (내장)" : ` (전용 ${gib(g.vram_bytes)})`}`).join(", ")}
            </dd>
            <dt>여유 공간</dt>
            <dd>{device.disk_free_bytes !== null ? bytes(device.disk_free_bytes) : "확인되지 않음"}</dd>
            <dt>운영체제</dt>
            <dd>{device.os}</dd>
          </dl>
          <p className="hint">확인 시각 {fmtDateTime(device.checked_at)} · 설정 화면을 열 때마다 다시 읽지 않습니다. 이 버전은 AI를 CPU로 실행합니다(그래픽카드 가속 미사용).</p>
          {device.problems.length > 0 && (
            <ul className="problems">
              {device.problems.map((p, i) => (
                <li key={i}>{p}</li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}

function ModelCard({ e, live, onChanged }: { e: CatalogEntry; live: DlProgress | undefined; onChanged: () => void }) {
  const { toast } = useStore();
  const m = e.model;
  const p = live ?? e.progress ?? null;
  const active = p && (p.status === "downloading" || p.status === "verifying" || p.status === "copying");
  const act = async (f: () => Promise<unknown>, ok?: string) => {
    try {
      await f();
      if (ok) toast(ok);
      onChanged();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const del = async () => {
    if (!(await confirmDialog(`${m.name} 모델 파일(${bytes(m.size_bytes)})을 지울까요?${e.loaded ? " 지금 쓰는 모델이라 AI 엔진을 멈춥니다." : ""}`, { okText: "지우기", danger: true }))) return;
    await act(() => api.deleteModel(m.id), "모델을 지웠습니다.");
  };
  const verify = async () => {
    try {
      const r = await api.verifyModel(m.id);
      toast(r.message, r.ok ? "ok" : "error");
      onChanged();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const status = e.installed ? (e.selected ? "사용 중" : "설치됨") : active ? "받는 중" : e.part_bytes > 0 || p?.status === "paused" ? "일시정지" : "설치 안 됨";
  return (
    <div className={`model-card ${e.selected ? "selected" : ""} ${e.recommended ? "recommended" : ""}`} data-model={m.id}>
      <div className="mc-head">
        <span className="tier">{m.tier_label}</span>
        {e.recommended && <span className="tag ok">이 PC 권장</span>}
        {e.fit === "too_big" && <span className="tag warn">메모리 부족 가능</span>}
        {e.fit === "tight" && <span className="tag">빠듯함</span>}
        <span className="grow" />
        <span className={`mc-status st-${status}`}>{status}</span>
      </div>
      <div className="mc-name">
        {m.name} <small className="muted">{m.params} · {m.quant} · {bytes(m.size_bytes)}</small>
      </div>
      <p className="small">{m.description}</p>
      <p className="tiny muted">
        권장 메모리 {m.recommended_ram_gb}GB · {m.publisher} · 라이선스 {m.license}
        {e.verified_at && ` · 검증 ${fmtDateTime(e.verified_at)}`}
      </p>
      {p && p.status !== "done" && p.status !== "idle" && (
        <div className="progress-wrap">
          <div className="progress">
            <div className="bar" style={{ width: `${percent(p.received, p.total)}%` }} />
          </div>
          <small>
            {p.status === "verifying"
              ? `검증 중 ${percent(p.received, p.total)}%`
              : p.status === "copying"
                ? `복사 중 ${percent(p.received, p.total)}%`
                : p.status === "paused"
                  ? `일시정지 ${bytes(p.received)} / ${bytes(p.total)}`
                  : p.status === "failed"
                    ? `실패: ${p.message ?? ""}`
                    : `${bytes(p.received)} / ${bytes(p.total)} · ${bytes(p.bytes_per_sec)}/초`}
            {p.status === "downloading" && p.message && ` — ${p.message}`}
          </small>
        </div>
      )}
      <div className="mc-actions">
        {e.installed ? (
          <>
            {!e.selected && (
              <button className="primary" onClick={() => void act(() => api.selectModel(m.id), `${m.name}을(를) 씁니다.`)}>
                이 모델 쓰기
              </button>
            )}
            <button onClick={() => void verify()} disabled={!!active}>
              다시 검사
            </button>
            <button className="ghost danger" onClick={() => void del()}>
              <IconTrash size={15} /> 삭제
            </button>
          </>
        ) : active ? (
          <>
            {p?.status === "downloading" && <button onClick={() => void act(() => api.pauseDownload(m.id))}>일시정지</button>}
            <button className="ghost danger" onClick={() => void act(() => api.cancelDownload(m.id), "다운로드를 취소했습니다.")}>
              취소
            </button>
          </>
        ) : (
          <>
            <button className="primary" onClick={() => void act(() => api.startDownload(m.id))}>
              <IconDownload size={15} /> {e.part_bytes > 0 ? "이어받기" : p?.status === "failed" ? "다시 시도" : "받기"}
            </button>
            {e.part_bytes > 0 && (
              <button className="ghost danger" onClick={() => void act(() => api.cancelDownload(m.id), "받던 파일을 지웠습니다.")}>
                받던 파일 지우기
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function RuntimePanel({ settings, onSettings }: { settings: AppSettings; onSettings: (s: AppSettings) => void }) {
  const { toast } = useStore();
  const [st, setSt] = useState<RuntimeStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => api.runtimeStatus().then(setSt).catch(() => undefined);
  useEffect(() => {
    void load();
    let un: (() => void) | null = null;
    let alive = true;
    void listen("runtime-state", () => void load()).then((u) => (alive ? (un = u) : u()));
    const t = window.setInterval(() => void load(), 4000);
    return () => {
      alive = false;
      un?.();
      window.clearInterval(t);
    };
  }, []);
  const s = st?.state;
  const label =
    !s ? "확인 중" : s.state === "ready" ? "준비됨" : s.state === "starting" ? "켜는 중 (모델 불러오는 중)" : s.state === "stopping" ? "끄는 중" : s.state === "failed" ? "문제 있음" : "꺼짐 (필요할 때 자동으로 켜짐)";
  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    try {
      await f();
      await load();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="runtime">
      <div className="row gap wrap">
        <span className={`rt-dot rt-${s?.state ?? "stopped"}`} />
        <strong className="grow">AI 엔진: {label}</strong>
        <button onClick={() => void run(api.runtimeStart)} disabled={busy || s?.state === "ready" || s?.state === "starting"}>
          지금 켜기
        </button>
        <button onClick={() => void run(api.runtimeStop)} disabled={busy || s?.state !== "ready"}>
          끄기 (메모리 돌려주기)
        </button>
      </div>
      {s?.state === "failed" && <p className="error-box">{s.message}</p>}
      {st?.device && s?.state === "ready" && (
        <p className="hint" data-testid="ai-device">
          계산 장치: <b>{st.device === "CPU" ? "CPU" : `GPU — ${st.device}`}</b>
          {st.device === "CPU" && settings.ai_device !== "cpu" && st.gpus.length > 0 && " (쓸 만한 외장 그래픽카드가 없거나 메모리가 모자라 CPU 로 돕니다)"}
        </p>
      )}
      {st && !st.runtime_found && <p className="error-box">AI 실행 구성요소(runtime 폴더)를 찾지 못했습니다. 압축을 풀 때 runtime 폴더까지 함께 풀었는지 확인해 주세요.</p>}
      {st && st.local_only !== null && (
        <p className={`hint ${st.local_only ? "ok" : "warn"}`}>
          <IconShield size={14} /> {st.local_only ? "AI 엔진 연결 확인: 이 PC 안에서만 접속할 수 있습니다." : `주의: AI 엔진이 외부 접속이 가능한 주소를 열었습니다 (${st.listening.join(", ")}).`}
        </p>
      )}
      <div className="row gap wrap">
        <label className="field">
          <span>쓰지 않으면 메모리에서 내리기</span>
          <select value={settings.idle_unload_minutes} onChange={(e) => onSettings({ ...settings, idle_unload_minutes: Number(e.target.value) })}>
            {[5, 10, 20, 30, 60, 0].map((m) => (
              <option key={m} value={m}>
                {m === 0 ? "내리지 않음" : `${m}분 뒤`}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>AI 계산 장치</span>
          <select
            value={settings.ai_device ?? "auto"}
            onChange={(e) => onSettings({ ...settings, ai_device: e.target.value === "cpu" ? "cpu" : "auto" })}
            title="자동: 그래픽카드(외장)가 있고 메모리가 넉넉하면 GPU 로 빠르게, 없거나 실패하면 CPU 로 돕니다. 바꾸면 다음에 AI 엔진을 켤 때부터 적용됩니다."
          >
            <option value="auto">자동 — 그래픽카드가 있으면 GPU (권장)</option>
            <option value="cpu">CPU 만</option>
          </select>
        </label>
        <label className="field">
          <span>AI 계산에 쓸 CPU 스레드</span>
          <select value={settings.threads ?? 0} onChange={(e) => onSettings({ ...settings, threads: Number(e.target.value) || null })}>
            <option value={0}>자동 (권장)</option>
            {[2, 3, 4, 6, 8, 12].map((n) => (
              <option key={n} value={n}>
                {n}개
              </option>
            ))}
          </select>
        </label>
      </div>
      {st && st.diagnostics.length > 0 && s?.state === "failed" && (
        <details>
          <summary>기술 정보 (문제 해결용)</summary>
          <pre className="diag">{st.diagnostics.join("\n")}</pre>
        </details>
      )}
    </div>
  );
}

/** 라이선스 및 저작권 — LICENSE.txt / THIRD_PARTY_NOTICES.txt 전문을 그대로 보여 준다 */
function LicensePanel() {
  const { toast } = useStore();
  const texts = useLoad(() => api.licenseTexts(), []);
  const [view, setView] = useState<"license" | "notices" | null>(null);
  return (
    <section className="panel" id="set-license">
      <h2>라이선스 및 저작권</h2>
      <p className="small">
        Copyright (c) 2026 차일영. 모든 권리 보유. 무료로 사용할 수 있지만 오픈소스가 아니며, 분해·역설계·수정·재배포·상업적 이용은 허락하지 않습니다. 문의: rraphop@me.com
      </p>
      <div className="row gap wrap">
        <button onClick={() => setView("license")} disabled={!texts.data}>
          라이선스 전문 보기
        </button>
        <button onClick={() => setView("notices")} disabled={!texts.data}>
          오픈소스·AI 모델 고지 보기
        </button>
        <button onClick={() => void api.openFolder("licenses").catch((e) => toast(errorText(e), "error"))}>
          <IconFolder size={15} /> 라이선스 폴더 열기
        </button>
      </div>
      {view && texts.data && (
        <Modal title={view === "license" ? "라이선스 (LICENSE.txt)" : "제3자 구성요소 고지 (THIRD_PARTY_NOTICES.txt)"} onClose={() => setView(null)} wide>
          <pre className="license-text">{view === "license" ? texts.data.license : texts.data.notices}</pre>
        </Modal>
      )}
    </section>
  );
}

export function SettingsPage() {
  const { toast, params, version, bump } = useStore();
  const info = useLoad(() => api.appInfo(), []);
  const catalog = useLoad(() => api.modelCatalog(), []);
  const live = useLiveProgress();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [deviceBusy, setDeviceBusy] = useState(false);
  const [testMsg, setTestMsg] = useState<string | null>(null);
  const [wipeText, setWipeText] = useState("");
  const [wipePending, setWipePending] = useState(false);
  const [restoring, setRestoring] = useState<BackupFile | null>(null);
  const backups = useLoad(() => api.listBackups(), []);

  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => toast(errorText(e), "error"));
  }, [toast, version]);
  useEffect(() => {
    if (info.data) setWipePending(info.data.wipe_pending);
  }, [info.data]);
  useEffect(() => {
    if (params.section) document.getElementById(`set-${params.section}`)?.scrollIntoView({ behavior: "smooth" });
  }, [params.section]);
  // 처음 실행이면 기기정보가 없을 수 있다 — 한 번만 읽는다
  useEffect(() => {
    if (catalog.data && catalog.data.device === null && !deviceBusy) {
      setDeviceBusy(true);
      api
        .deviceInfo(false)
        .then(() => catalog.reload())
        .finally(() => setDeviceBusy(false));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalog.data?.device]);

  const saveSettings = async (s: AppSettings) => {
    setSettings(s);
    try {
      setSettings(await api.updateSettings(s));
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const refreshDevice = async () => {
    setDeviceBusy(true);
    try {
      await api.deviceInfo(true);
      catalog.reload();
      toast("기기정보를 다시 읽었습니다.");
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setDeviceBusy(false);
    }
  };
  const importFile = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "GGUF 모델 파일", extensions: ["gguf"] }] });
    if (!path || Array.isArray(path)) return;
    toast("파일을 검사하고 복사합니다. 큰 파일은 몇 분 걸릴 수 있습니다.", "info");
    try {
      const id = await api.importModelFile(path);
      toast(`모델을 설치했습니다 (${id}).`);
      catalog.reload();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const testNotify = async () => {
    try {
      setTestMsg(await api.testNotification());
    } catch (e) {
      setTestMsg(`알림 실패: ${errorText(e)}`);
    }
  };
  const wipe = async () => {
    try {
      const ok = await api.requestWipe(wipeText);
      if (ok) {
        setWipePending(true);
        setWipeText("");
      }
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const toggleAutostart = async (on: boolean) => {
    try {
      await api.setAutostart(on);
      info.reload();
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  const entries = catalog.data?.entries ?? [];
  return (
    <div className="page settings">
      <div className="page-head">
        <h1>설정</h1>
      </div>

      {/* 넓은 화면에서는 두 단으로 (좁으면 한 단으로 차례대로) */}
      <div className="settings-cols">
        <div className="settings-col">
          <section className="panel privacy" id="set-privacy">
            <h2>
              <IconShield /> 개인정보 보호
            </h2>
            <p>기록과 AI 처리는 이 PC 안에서만 이루어집니다. 학생정보를 외부 AI 서비스로 보내지 않으며, 사용 기록을 수집하지 않습니다.</p>
            <p className="small muted">인터넷은 AI 모델 파일을 받을 때와, 학교를 정했을 때 나이스 교육정보 개방 포털에서 시간표·급식·학사일정을 받을 때만 씁니다. 이때도 학생 정보는 보내지 않고 나이스에는 학교 코드만 보냅니다.</p>
            {settings && (
              <label className="check">
                <input
                  type="checkbox"
                  checked={settings.store_rrn}
                  onChange={(e) => {
                    const on = e.target.checked;
                    const go = on ? Promise.resolve(true) : confirmDialog("저장을 끄면 이미 저장한 주민등록번호도 모두 지웁니다. 계속할까요?", { okText: "끄고 지우기", danger: true });
                    void go.then((ok) => {
                      if (ok) void saveSettings({ ...settings, store_rrn: on });
                    });
                  }}
                />
                나이스 명렬표를 가져올 때 주민등록번호도 저장 (기본 꺼짐 · 켜도 뒷자리는 가린 채 저장하고, 학생 상세에서만 보입니다)
              </label>
            )}
          </section>

          {settings && <SchoolPanel settings={settings} onSave={saveSettings} />}

          <section className="panel" id="set-models">
            <h2>AI 모델</h2>
            {catalog.error && <ErrorBox error={catalog.error} onRetry={catalog.reload} />}
            <DeviceCard device={catalog.data?.device ?? null} onRefresh={() => void refreshDevice()} busy={deviceBusy} />
            <div className="model-grid">
              {entries.map((e) => (
                <ModelCard key={e.model.id} e={e} live={live[e.model.id]} onChanged={catalog.reload} />
              ))}
            </div>
            <div className="row gap wrap">
              <button onClick={() => void importFile()}>파일로 설치 (USB 등으로 받은 .gguf)</button>
              <button onClick={() => void api.openFolder("models")}>
                <IconFolder size={15} /> 모델 파일 위치 열기
              </button>
              <small className="muted">모델 파일은 크기와 SHA-256을 확인한 뒤에만 설치됩니다.</small>
            </div>
          </section>

          {settings && (
            <section className="panel" id="set-engine">
              <h2>AI 엔진</h2>
              <RuntimePanel settings={settings} onSettings={(s) => void saveSettings(s)} />
              <label className="check">
                <input type="checkbox" checked={settings.ai_auto_classify} onChange={(e) => void saveSettings({ ...settings, ai_auto_classify: e.target.checked })} />
                기록을 저장하면 AI가 자동으로 정리 (끄면 ‘AI로 분류’를 눌렀을 때만 정리)
              </label>
            </section>
          )}

        </div>
        <div className="settings-col">
          {settings && (
            <section className="panel" id="set-notify">
              <h2>알림</h2>
              <p className="small">새로 생기는 할 일·업무의 기본 알림입니다. 항목마다 따로 바꿀 수 있습니다.</p>
              <ReminderPicker value={settings.remind_default} onChange={(r) => void saveSettings({ ...settings, remind_default: { ...r, custom_at: null } })} hasDate />
              <div className="row gap wrap">
                <button onClick={() => void testNotify()}>테스트 알림 보내기</button>
                {testMsg && <span className={testMsg.startsWith("알림 실패") ? "err" : "ok"}>{testMsg}</span>}
              </div>
              <p className="hint">
                창의 X 를 눌러도 꺼지지 않고 알림 영역(시계 옆)에 남아 알림과 AI 정리를 계속합니다. 아이콘을 두 번 누르면 창이 열리고, 아이콘 메뉴에서 ‘기록하기’로 작은 입력창만 띄우거나
                ‘종료하기’로 완전히 끌 수 있습니다.
              </p>
              <label className="check">
                <input type="checkbox" checked={info.data?.autostart ?? false} onChange={(e) => void toggleAutostart(e.target.checked)} />
                Windows에 로그인하면 교무수첩을 자동으로 실행 (왼쪽 메뉴 ‘설정’ 아래에서도 켜고 끌 수 있습니다)
              </label>
              <p className="hint warn">
                알림은 교무수첩이 실행 중일 때만 표시됩니다. 프로그램이 완전히 꺼져 있던 동안의 알림은 다음 실행 때 ‘놓친 알림’으로 한 번에 알려 드립니다. 창을 닫아도 계속 실행되게 하거나 자동 시작을 켜면 알림을 놓치지 않습니다.
              </p>
            </section>
          )}

          <YearPanel />

          <ExportPanel />

          <section className="panel" id="set-data">
            <h2>데이터</h2>
            {info.data && (
              <dl className="spec">
                <dt>사용자 데이터</dt>
                <dd>
                  <code>{info.data.data_dir}</code>
                </dd>
                <dt>프로그램 위치</dt>
                <dd>
                  {info.data.launcher_path ? (
                    <>
                      <code>{info.data.launcher_path}</code>
                      <br />
                      <small className="muted">파일 하나 버전 — 이 파일을 새 버전으로 덮어쓰면 업데이트됩니다. 실행할 때 풀어 쓰는 위치: {info.data.exe_dir}</small>
                    </>
                  ) : (
                    <code>{info.data.exe_dir}</code>
                  )}
                </dd>
              </dl>
            )}
            <p className="small muted">프로그램 폴더를 옮기거나 새 버전으로 바꿔도 기록과 모델은 사용자 데이터 위치에 그대로 남습니다.</p>
            <div className="row gap wrap">
              <button onClick={() => void api.openFolder("data")}>
                <IconFolder size={15} /> 사용자 데이터 위치 열기
              </button>
              <button onClick={() => void api.openFolder("backups")}>백업 폴더 열기</button>
              <button
                onClick={() =>
                  void api
                    .backupNow()
                    .then((p) => {
                      toast(`백업했습니다: ${p}`);
                      backups.reload();
                    })
                    .catch((e) => toast(errorText(e), "error"))
                }
              >
                지금 백업
              </button>
              <button onClick={() => void api.openFolder("logs")}>기술 로그 열기</button>
            </div>
            <BackupMirror onChanged={backups.reload} />
            {backups.data && backups.data.length > 0 && (
              <details className="backup-list">
                <summary>
                  백업 {backups.data.length}개 — 되돌리기 (자동 백업은 하루 한 번, 최근 14개 보관)
                </summary>
                <table className="backup-table">
                  <thead>
                    <tr>
                      <th>백업 시각</th>
                      <th>종류</th>
                      <th>크기</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {backups.data.map((b) => (
                      <tr key={b.name}>
                        <td>{b.taken_at || b.name}</td>
                        <td>
                          {backupKind(b.kind)}
                          {b.location === "mirror" && <span className="tag">두 번째 위치</span>}
                        </td>
                        <td>{bytes(b.size)}</td>
                        <td>
                          <button onClick={() => setRestoring(b)}>되돌리기</button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </details>
            )}
            {restoring && <RestoreDialog backup={restoring} onClose={() => setRestoring(null)} />}
            <div className="danger-zone">
              <h3>모든 로컬 데이터 삭제</h3>
              {wipePending ? (
                <div className="row gap wrap">
                  <span className="err">삭제가 예약되었습니다. 프로그램을 다시 시작하면 지워집니다.</span>
                  <button className="danger" onClick={() => void api.restartApp()}>
                    지금 다시 시작해서 지우기
                  </button>
                  <button
                    onClick={() =>
                      void api.cancelWipe().then(() => {
                        setWipePending(false);
                        toast("삭제 예약을 취소했습니다.");
                      })
                    }
                  >
                    예약 취소
                  </button>
                </div>
              ) : (
                <>
                  <p className="small">
                    기록, 학생, 일정, 학생부 자료, AI 모델, 백업을 이 PC에서 모두 지웁니다. 확인 문구 <b>모두 삭제</b>를 입력한 뒤, Windows 확인 창에서 한 번 더 승인해야 합니다.
                  </p>
                  <div className="row gap">
                    <input value={wipeText} onChange={(e) => setWipeText(e.target.value)} placeholder="모두 삭제" />
                    <button className="danger" onClick={() => void wipe()} disabled={wipeText.trim() !== "모두 삭제"}>
                      삭제 예약
                    </button>
                  </div>
                </>
              )}
            </div>
          </section>

          <section className="panel" id="set-about">
            <h2>정보</h2>
            <p className="small">로컬 AI 교무수첩 {info.data?.version} · AI 엔진 llama.cpp</p>
          </section>
        </div>
      </div>
      {/* 라이선스 및 저작권은 두 단 아래, 설정 맨 아래 */}
      <LicensePanel />
    </div>
  );
}
