import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";
import type { NeisImportResult } from "../types";
import { Modal } from "./Modal";

export const NEIS_ROSTER_TIP =
  "나이스 → 학적 → 기본학적관리 → 명렬표출력 → 학급별 조회 → 엑셀파일 다운로드로 받은 파일을 선택하세요. 여러 학급 파일을 한꺼번에 골라도 됩니다.";

const isExcel = (p: string) => /\.(xlsx|xls|xlsm)$/i.test(p);

/** 창에 엑셀 파일을 끌어다 놓으면 받는다. 끌고 있는 동안 true. */
export function useExcelDrop(onDrop: (paths: string[]) => void): boolean {
  const [over, setOver] = useState(false);
  const cb = useRef(onDrop);
  cb.current = onDrop;
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter") setOver(p.paths.some(isExcel));
        else if (p.type === "leave") setOver(false);
        else if (p.type === "drop") {
          setOver(false);
          const xs = p.paths.filter(isExcel);
          if (xs.length) cb.current(xs);
        }
      })
      .then((f) => {
        if (alive) unlisten = f;
        else f();
      })
      .catch(() => {
        /* 끌어다 놓기를 못 써도 파일 고르기로 가져올 수 있다 */
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return over;
}

export function DropOverlay() {
  return (
    <div className="drop-overlay" aria-hidden>
      <div>놓으면 나이스 명렬표를 가져옵니다</div>
    </div>
  );
}

/** 나이스 명렬표 가져오기 창. incoming 이 바뀌면(끌어다 놓기) 바로 가져온다. */
export function NeisImportDialog({ incoming, onClose }: { incoming: { paths: string[]; seq: number }; onClose: () => void }) {
  const { toast, bump } = useStore();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<NeisImportResult | null>(null);
  const run = async (paths: string[]) => {
    if (!paths.length) return;
    setBusy(true);
    try {
      const r = await api.importNeisRoster(paths);
      setResult(r);
      bump();
      const ok = r.files.filter((f) => !f.error);
      if (ok.length) toast(`명렬표 ${ok.length}개를 가져왔습니다.`);
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    // 끌어다 놓을 때마다(seq) 한 번씩 가져온다
    if (incoming.paths.length) void run(incoming.paths);
  }, [incoming.seq]);
  const pick = async () => {
    const r = await open({ multiple: true, directory: false, title: "나이스 명렬표 엑셀 파일 고르기", filters: [{ name: "나이스 명렬표 (엑셀)", extensions: ["xlsx", "xls"] }] });
    if (!r) return;
    void run(Array.isArray(r) ? r : [r]);
  };
  const rrnSkipped = result?.files.some((f) => f.rrn_skipped);
  return (
    <Modal
      title="나이스 명렬표 가져오기"
      onClose={onClose}
      footer={
        <>
          <button onClick={onClose}>닫기</button>
          <button className="primary" onClick={() => void pick()} disabled={busy}>
            {busy ? "가져오는 중…" : result ? "다른 파일 더 가져오기" : "파일 고르기"}
          </button>
        </>
      }
    >
      <p className="hint">{NEIS_ROSTER_TIP}</p>
      <div className="drop-zone">
        엑셀 파일을 이 창에 끌어다 놓아도 됩니다. 기본 5열(학년·반·번호·성명·비고)과 전체 열 파일 모두 읽습니다.
      </div>
      <p className="small muted">
        같은 학생은 학생개인번호, 없으면 학급·번호·이름으로 찾아 정보만 보탭니다. 다시 가져와도 인원이 늘지 않고, 빈칸이 기존 값을 지우지 않으며, 기존 기록은 그대로 남습니다. 학생 정보는 이 PC에만
        저장되고 AI 분류에는 이름·학급·번호만 쓰입니다.
      </p>
      {result && (
        <>
          <table className="import-table">
            <thead>
              <tr>
                <th>파일</th>
                <th>학급</th>
                <th>인원</th>
                <th>새로 등록</th>
                <th>정보 보탬</th>
                <th>합침</th>
                <th>그대로</th>
              </tr>
            </thead>
            <tbody>
              {result.files.map((f, i) =>
                f.error ? (
                  <tr key={i} className="err-row">
                    <td>{f.file}</td>
                    <td colSpan={6}>{f.error}</td>
                  </tr>
                ) : (
                  <tr key={i}>
                    <td title={f.optional_columns.length ? `읽은 항목: ${f.optional_columns.join(", ")}` : undefined}>{f.file}</td>
                    <td>{f.classes.join(", ")}</td>
                    <td>{f.total}</td>
                    <td>{f.added}</td>
                    <td>{f.updated}</td>
                    <td>{f.merged}</td>
                    <td>{f.unchanged}</td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
          {result.files.some((f) => f.merged > 0) && <p className="small">‘합침’은 기록에서 자동으로 등록된 ‘확인 필요’ 학생을 명렬표의 학생과 합친 것입니다. 그 학생의 기록은 그대로 옮겨졌습니다.</p>}
          {rrnSkipped && <p className="small muted">주민등록번호 열은 저장하지 않았습니다. 필요하면 설정 → 개인정보 보호에서 켤 수 있습니다.</p>}
          {result.problems.length > 0 && <pre className="result">{result.problems.join("\n")}</pre>}
        </>
      )}
    </Modal>
  );
}
