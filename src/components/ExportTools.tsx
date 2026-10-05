import { open, save } from "@tauri-apps/plugin-dialog";
import { useState, type ReactNode } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";
import type { ExportOptions, ExportResult, ExportSheet } from "../types";
import { Modal } from "./Modal";

export const EXPORT_SHEETS: Array<{ id: ExportSheet; label: string; personal?: boolean }> = [
  { id: "roster", label: "학급명렬표", personal: true },
  { id: "notes", label: "기록 원문", personal: true },
  { id: "observations", label: "학생 관찰", personal: true },
  { id: "drafts", label: "학생부 초안", personal: true },
  { id: "tasks", label: "할 일" },
  { id: "events", label: "일정·학사력" },
  { id: "duties", label: "담당업무" },
  { id: "duty_logs", label: "업무 기록" },
  { id: "timetable", label: "시간표" },
];

const pad = (n: number) => String(n).padStart(2, "0");

export function todayText(): string {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 폴더를 고른다. 자동 시험에서는 창 대신 미리 정해 둔 폴더를 쓴다. */
export async function pickFolder(title: string): Promise<string | null> {
  const testDir = (window as unknown as { __tnTestPickPath?: string }).__tnTestPickPath;
  if (testDir) return testDir;
  const p = await open({ title, directory: true, multiple: false });
  return typeof p === "string" ? p : null;
}

/** 저장 위치를 고른다. 자동 시험에서는 창 대신 미리 정해 둔 폴더를 쓴다. */
async function pickTarget(format: ExportOptions["format"], defaultName: string): Promise<string | null> {
  const testDir = (window as unknown as { __tnTestPickPath?: string }).__tnTestPickPath;
  if (testDir) return format === "xlsx" ? `${testDir}\\${defaultName}.xlsx` : testDir;
  if (format === "xlsx") {
    const p = await save({ title: "엑셀 파일로 저장", defaultPath: `${defaultName}.xlsx`, filters: [{ name: "엑셀 파일", extensions: ["xlsx"] }] });
    return p ?? null;
  }
  const p = await open({ title: "CSV 파일을 넣을 폴더 고르기", directory: true, multiple: false });
  return typeof p === "string" ? p : null;
}

function ExportDone({ result, onClose }: { result: ExportResult; onClose: () => void }) {
  const { toast } = useStore();
  const total = result.sheets.reduce((n, s) => n + s.rows, 0);
  return (
    <Modal
      title="내보냈습니다"
      onClose={onClose}
      footer={
        <>
          <button onClick={() => void api.revealExport(result.path).catch((e) => toast(errorText(e), "error"))}>탐색기에서 보기</button>
          <button className="primary" onClick={onClose}>
            닫기
          </button>
        </>
      }
    >
      <p className="export-path">
        <code>{result.path}</code>
      </p>
      <ul className="export-counts">
        {result.sheets.map((s) => (
          <li key={s.title}>
            {s.title} <b>{s.rows}</b>줄
          </li>
        ))}
      </ul>
      {total === 0 && <p className="hint">아직 내보낼 내용이 없어 머리글만 들어 있습니다.</p>}
      <p className="hint warn">이 파일에는 암호가 없습니다. 학생 정보가 들어 있다면 USB·메일로 옮길 때 주의하고, 다 쓴 파일은 지워 주세요.</p>
    </Modal>
  );
}

/** 저장 위치 고르기 → 내보내기 → 결과 창 */
export function useExporter(): { run: (opts: Omit<ExportOptions, "path">, defaultName: string) => Promise<void>; busy: boolean; modal: ReactNode } {
  const { toast } = useStore();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ExportResult | null>(null);
  const run = async (opts: Omit<ExportOptions, "path">, defaultName: string) => {
    let path: string | null = null;
    try {
      path = await pickTarget(opts.format, defaultName);
    } catch (e) {
      toast(errorText(e), "error");
    }
    if (!path) return;
    setBusy(true);
    try {
      setResult(await api.exportData({ ...opts, path }));
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return { run, busy, modal: result ? <ExportDone result={result} onClose={() => setResult(null)} /> : null };
}

/** 설정 화면: 엑셀·CSV로 내보내기 */
export function ExportPanel() {
  const [picked, setPicked] = useState<Set<ExportSheet>>(() => new Set(EXPORT_SHEETS.map((s) => s.id)));
  const [format, setFormat] = useState<ExportOptions["format"]>("xlsx");
  const [mask, setMask] = useState(false);
  const { run, busy, modal } = useExporter();
  const toggle = (id: ExportSheet) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const all = picked.size === EXPORT_SHEETS.length;
  return (
    <section className="panel" id="set-export">
      <h2>엑셀·CSV로 내보내기</h2>
      <p className="small muted">보관·열람·인쇄용입니다. 프로그램 데이터를 예전 상태로 되돌리는 것은 아래 ‘데이터’의 백업으로 합니다.</p>
      <fieldset className="export-sheets">
        <legend>
          내보낼 항목{" "}
          <button className="link" onClick={() => setPicked(all ? new Set() : new Set(EXPORT_SHEETS.map((s) => s.id)))}>
            {all ? "모두 해제" : "모두 선택"}
          </button>
        </legend>
        {EXPORT_SHEETS.map((s) => (
          <label key={s.id} className="check">
            <input type="checkbox" checked={picked.has(s.id)} onChange={() => toggle(s.id)} /> {s.label}
          </label>
        ))}
      </fieldset>
      <div className="seg" role="radiogroup" aria-label="파일 형식">
        <button className={format === "xlsx" ? "on" : ""} onClick={() => setFormat("xlsx")} aria-pressed={format === "xlsx"}>
          엑셀 파일 (.xlsx) — 항목마다 시트
        </button>
        <button className={format === "csv" ? "on" : ""} onClick={() => setFormat("csv")} aria-pressed={format === "csv"}>
          CSV — 항목마다 파일
        </button>
      </div>
      <label className="check">
        <input type="checkbox" checked={mask} onChange={(e) => setMask(e.target.checked)} /> 학급명렬표의 전화번호·보호자 연락처·주소 가리기
      </label>
      <p className="hint warn">
        내보낸 파일에는 암호가 없습니다.
        {[...picked].some((id) => EXPORT_SHEETS.find((s) => s.id === id)?.personal) && " 학생 이름·관찰 내용 같은 개인정보가 들어갑니다."}
        {" "}학교 규정에 맞는 곳에 저장해 주세요.
      </p>
      <div className="row gap">
        <button
          className="primary"
          disabled={picked.size === 0 || busy}
          onClick={() =>
            void run(
              { format, sheets: EXPORT_SHEETS.filter((s) => picked.has(s.id)).map((s) => s.id), mask_contacts: mask, class_id: null, roster_columns: null },
              `교무수첩 ${todayText()}`,
            )
          }
        >
          {busy ? "내보내는 중…" : format === "xlsx" ? "엑셀 파일로 저장…" : "CSV 파일로 저장…"}
        </button>
      </div>
      {modal}
    </section>
  );
}
