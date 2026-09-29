import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { ErrorBox, ReminderPicker } from "../components/Fields";
import { IconDownload, IconFolder, IconRefresh, IconShield, IconTrash } from "../components/Icons";
import { confirmDialog } from "../components/Modal";
import { bytes, fmtDateTime, gib, percent } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { AppSettings, CatalogEntry, DeviceInfo, DlProgress, RuntimeStatus } from "../types";

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

export function SettingsPage() {
  const { toast, params, version } = useStore();
  const info = useLoad(() => api.appInfo(), []);
  const catalog = useLoad(() => api.modelCatalog(), []);
  const live = useLiveProgress();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [deviceBusy, setDeviceBusy] = useState(false);
  const [testMsg, setTestMsg] = useState<string | null>(null);
  const [wipeText, setWipeText] = useState("");
  const [wipePending, setWipePending] = useState(false);
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

      <section className="panel privacy" id="set-privacy">
        <h2>
          <IconShield /> 개인정보 보호
        </h2>
        <p>기록과 AI 처리는 이 PC 안에서만 이루어집니다. 학생정보를 외부 AI 서비스로 보내지 않으며, 사용 기록을 수집하지 않습니다.</p>
        <p className="small muted">인터넷은 AI 모델 파일을 받을 때만 씁니다. 이때도 학생 정보는 보내지 않습니다.</p>
      </section>

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

      {settings && (
        <section className="panel" id="set-notify">
          <h2>알림</h2>
          <p className="small">새로 생기는 할 일·업무의 기본 알림입니다. 항목마다 따로 바꿀 수 있습니다.</p>
          <ReminderPicker value={settings.remind_default} onChange={(r) => void saveSettings({ ...settings, remind_default: { ...r, custom_at: null } })} hasDate />
          <div className="row gap wrap">
            <button onClick={() => void testNotify()}>테스트 알림 보내기</button>
            {testMsg && <span className={testMsg.startsWith("알림 실패") ? "err" : "ok"}>{testMsg}</span>}
          </div>
          <label className="check">
            <input type="checkbox" checked={settings.close_to_tray} onChange={(e) => void saveSettings({ ...settings, close_to_tray: e.target.checked })} />
            창을 닫아도 알림 영역(시계 옆)에서 계속 실행
          </label>
          <label className="check">
            <input type="checkbox" checked={info.data?.autostart ?? false} onChange={(e) => void toggleAutostart(e.target.checked)} />
            Windows에 로그인하면 교무수첩을 알림 영역에서 자동 시작
          </label>
          <p className="hint warn">
            알림은 교무수첩이 실행 중일 때만 표시됩니다. 프로그램이 완전히 꺼져 있던 동안의 알림은 다음 실행 때 ‘놓친 알림’으로 한 번에 알려 드립니다. 창을 닫아도 계속 실행되게 하거나 자동 시작을 켜면 알림을 놓치지 않습니다.
          </p>
        </section>
      )}

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
              <code>{info.data.exe_dir}</code>
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
        {backups.data && backups.data.length > 0 && (
          <details>
            <summary>백업 {backups.data.length}개 (하루 한 번 자동 백업, 최근 14개 보관)</summary>
            <ul className="plain-list">
              {backups.data.map((b) => (
                <li key={b.name}>
                  {b.name} · {bytes(b.size)}
                </li>
              ))}
            </ul>
          </details>
        )}
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
        <p className="small">
          로컬 AI 교무수첩 {info.data?.version} · AI 엔진 llama.cpp (MIT) · 라이선스 전문은 프로그램 폴더의 licenses 폴더에 있습니다.
        </p>
      </section>
    </div>
  );
}
