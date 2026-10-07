import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "../api";
import { useLoad, useStore } from "../lib/store";
import { setMask as saveMask, useMaskContacts } from "../lib/privacy";
import { nameMarks, studentMatcher } from "../lib/studentSearch";
import { ContactsCell } from "./ProfileParts";
import type { AppSettings, RosterColumns, RosterField, Student } from "../types";
import { todayText, useExporter } from "./ExportTools";

// 선택 칸: 칸마다 최소·최대 폭과 남는 폭을 나눌 비율 (성별·생일 좁게, 전화번호·학생개인번호 보통, 비고·특이사항·주소 넓게)
const OPTIONAL: Array<{ key: keyof RosterColumns; label: string; field: RosterField; placeholder: string; sensitive?: boolean; min: number; max: number; weight: number }> = [
  { key: "phone", label: "전화번호", field: "phone", placeholder: "010-0000-0000", sensitive: true, min: 120, max: 160, weight: 1 },
  { key: "guardian_phone", label: "보호자 연락처", field: "guardian_phone", placeholder: "010-0000-0000", sensitive: true, min: 130, max: 200, weight: 1.2 },
  { key: "address", label: "주소", field: "address", placeholder: "주소", sensitive: true, min: 180, max: 420, weight: 3 },
  { key: "gender", label: "성별", field: "gender", placeholder: "남/여", min: 52, max: 64, weight: 0.3 },
  { key: "birthday", label: "생일", field: "birthday", placeholder: "2012-03-05 또는 3월 5일", min: 96, max: 140, weight: 0.6 },
  { key: "memo", label: "특이사항", field: "memo", placeholder: "학생 개별 특이사항", min: 180, max: 420, weight: 3 },
  { key: "student_no", label: "학생개인번호", field: "student_no", placeholder: "나이스 10자리", min: 110, max: 140, weight: 0.8 },
  { key: "prev_record", label: "이전학적", field: "prev_record", placeholder: "예: 2학년1반1번", min: 120, max: 180, weight: 1 },
  { key: "remark", label: "비고", field: "remark", placeholder: "나이스 비고", min: 140, max: 360, weight: 2 },
];
/** 고정 칸: 학급·번호·이름(왼쪽 고정)과 기록 */
const FIXED = { class: 64, number: 52, name: 96, record: 84 };

/** 남는 폭을 비율대로 나누되 칸마다 최소·최대를 지킨다. 모자라면 모두 최소(가로 스크롤), 남으면 표 오른쪽 여백 */
export function columnWidths(specs: Array<{ min: number; max: number; weight: number }>, available: number): number[] {
  const w = specs.map((s) => s.min);
  let rest = available - w.reduce((a, b) => a + b, 0);
  let open = specs.map((_, i) => i);
  while (rest > 0.5 && open.length > 0) {
    const total = open.reduce((a, i) => a + specs[i]!.weight, 0);
    let used = 0;
    const next: number[] = [];
    for (const i of open) {
      const add = (rest * specs[i]!.weight) / total;
      const room = specs[i]!.max - w[i]!;
      const take = Math.min(add, room);
      w[i] = w[i]! + take;
      used += take;
      if (take < add - 0.01) continue; // 최대에 닿음
      next.push(i);
    }
    rest -= used;
    if (next.length === open.length && used < 0.5) break;
    open = next.filter((i) => w[i]! < specs[i]!.max - 0.01);
  }
  return w.map((x) => Math.floor(x));
}

function Marked({ text, query }: { text: string; query: string }) {
  const marks = nameMarks(text, query);
  if (marks.length === 0) return <>{text}</>;
  const out: ReactNode[] = [];
  let at = 0;
  marks.forEach(([s, e], i) => {
    if (s < at) return;
    if (s > at) out.push(text.slice(at, s));
    out.push(<mark key={i}>{text.slice(s, e)}</mark>);
    at = e;
  });
  out.push(text.slice(at));
  return <>{out}</>;
}

function mask(v: string, kind: RosterField): string {
  if (!v) return v;
  if (kind === "address") return v.split(" ").slice(0, 2).join(" ") + " ●●●";
  const digits = v.replace(/\D/g, "");
  return digits.length >= 4 ? `●●●-●●●●-${digits.slice(-4)}` : "●●●●";
}

function birthdayText(b: string | null): string {
  if (!b) return "";
  const md = b.length === 10 ? b.slice(5) : b;
  const [m, d] = md.split("-").map(Number);
  return `${m}월 ${d}일${b.length === 10 ? ` (${b.slice(0, 4)})` : ""}`;
}

/** 눌러서 바로 고치는 칸 */
function Cell({
  student,
  field,
  value,
  display,
  placeholder,
  wide,
  highlight,
  fresh,
}: {
  student: Student;
  field: RosterField;
  value: string;
  display?: string;
  placeholder: string;
  wide?: boolean;
  highlight?: string;
  /** 지난번에 본 뒤 기록에서 개인정보가 새로 들어간 학생 (이름 옆 점) */
  fresh?: boolean;
}) {
  const { toast, bump } = useStore();
  const [editing, setEditing] = useState(false);
  const [v, setV] = useState(value);
  const ref = useRef<HTMLInputElement | HTMLSelectElement>(null);
  useEffect(() => setV(value), [value]);
  useEffect(() => {
    if (editing) ref.current?.focus();
  }, [editing]);
  const save = async () => {
    setEditing(false);
    if (v.trim() === value.trim()) return;
    try {
      await api.setStudentField(student.id, field, v);
      bump();
    } catch (e) {
      toast(errorText(e), "error");
      setV(value);
    }
  };
  if (!editing) {
    return (
      <td className={`roster-cell ${wide ? "wide" : ""}`} onClick={() => setEditing(true)} title="눌러서 고치기">
        {display ?? value ? <span>{highlight ? <Marked text={display ?? value} query={highlight} /> : display ?? value}</span> : <span className="ph">{placeholder}</span>}
        {fresh && <span className="new-dot" title="기록에서 개인정보가 새로 들어감" aria-label="새로 들어간 정보" />}
      </td>
    );
  }
  return (
    <td className={`roster-cell editing ${wide ? "wide" : ""}`}>
      {field === "gender" ? (
        <select
          ref={ref as React.RefObject<HTMLSelectElement>}
          value={v}
          onChange={(e) => setV(e.target.value)}
          onBlur={() => void save()}
          onKeyDown={(e) => e.key === "Enter" && void save()}
        >
          <option value="">-</option>
          <option value="남">남</option>
          <option value="여">여</option>
        </select>
      ) : (
        <input
          ref={ref as React.RefObject<HTMLInputElement>}
          value={v}
          placeholder={placeholder}
          onChange={(e) => setV(e.target.value)}
          onBlur={() => void save()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") {
              setV(value);
              setEditing(false);
            }
          }}
        />
      )}
    </td>
  );
}

export function RosterTable({
  onOpen,
  classId,
  setClassId,
  query,
  fresh,
}: {
  onOpen: (id: number) => void;
  classId: number | null;
  setClassId: (id: number | null) => void;
  query: string;
  fresh?: Set<number>;
}) {
  const { toast, bump } = useStore();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  // 가리기는 기록 화면과 함께 쓴다
  const masked = useMaskContacts();
  const [adding, setAdding] = useState({ class_label: "", number: "", name: "" });
  const exporter = useExporter();
  const classes = useLoad(() => api.listClasses(), []);
  const list = useLoad(() => api.listStudents(null, classId), [classId]);
  const matcher = useMemo(() => studentMatcher(query), [query]);
  const rows = useMemo(() => (matcher ? (list.data ?? []).filter(matcher) : list.data ?? []), [list.data, matcher]);
  // 표 폭: 왼쪽 고정 세 칸 + 선택 칸(남는 폭을 비율대로) + 기록 칸
  const scrollRef = useRef<HTMLDivElement>(null);
  const [boxWidth, setBoxWidth] = useState(1000);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBoxWidth(el.clientWidth));
    ro.observe(el);
    setBoxWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);
  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => toast(errorText(e), "error"));
  }, [toast]);

  const cols = settings?.roster_columns;
  const saveSettings = async (s: AppSettings) => {
    setSettings(s);
    try {
      setSettings(await api.updateSettings(s));
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const toggleCol = (k: keyof RosterColumns) => {
    if (!settings) return;
    void saveSettings({ ...settings, roster_columns: { ...settings.roster_columns, [k]: !settings.roster_columns[k] } });
  };
  const setMask = (m: boolean) => saveMask(m);
  const addStudent = async () => {
    if (!adding.name.trim()) return;
    try {
      await api.createStudent({ name: adding.name, class_label: adding.class_label || null, number: adding.number ? Number(adding.number) : null });
      setAdding({ ...adding, number: adding.number ? String(Number(adding.number) + 1) : "", name: "" });
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const enabled = OPTIONAL.filter((o) => cols?.[o.key]);
  const fixedSum = FIXED.class + FIXED.number + FIXED.name + FIXED.record;
  const widths = columnWidths(enabled, boxWidth - fixedSum - 2);
  const tableWidth = fixedSum + widths.reduce((a, b) => a + b, 0);
  const searching = !!matcher;
  const guardianOn = enabled.some((o) => o.field === "guardian_phone");

  return (
    <div className="roster">
      <div className="roster-bar no-print">
        <div className="class-filter">
          <button className={classId === null ? "on" : ""} onClick={() => setClassId(null)}>
            전체 학급
          </button>
          {classes.data?.map((c) => (
            <button key={c.id} className={classId === c.id ? "on" : ""} onClick={() => setClassId(c.id)}>
              {c.label} <small>{c.student_count}</small>
            </button>
          ))}
        </div>
        <fieldset className="roster-cols">
          <legend>보일 항목</legend>
          {OPTIONAL.map((o) => (
            <label key={o.key} className="check">
              <input type="checkbox" checked={!!cols?.[o.key]} onChange={() => toggleCol(o.key)} disabled={!settings} /> {o.label}
            </label>
          ))}
        </fieldset>
        <div className="row gap wrap">
          {cols?.birthday && settings && (
            <label className="check">
              <input type="checkbox" checked={settings.birthdays_on_calendar} onChange={(e) => void saveSettings({ ...settings, birthdays_on_calendar: e.target.checked })} />
              학생 생일을 일정표(학사력)와 오늘 화면에 표시
            </label>
          )}
          {enabled.some((o) => o.sensitive) && (
            <label className="check" title="기록 화면과 오늘의 기록에서도 전화번호·주소를 가립니다">
              <input type="checkbox" checked={masked} onChange={(e) => setMask(e.target.checked)} /> 연락처·주소 가리기 (수업 화면에 띄울 때, 기록 화면도 함께)
            </label>
          )}
          <span className="grow" />
          <button
            disabled={exporter.busy || !settings || (searching && rows.length === 0)}
            title={searching ? `찾은 학생 ${rows.length}명만 엑셀 파일로 저장합니다` : "지금 보이는 학급·항목 그대로 엑셀 파일로 저장합니다"}
            onClick={() =>
              void exporter.run(
                {
                  format: "xlsx",
                  sheets: ["roster"],
                  class_id: classId,
                  roster_columns: settings?.roster_columns ?? null,
                  mask_contacts: masked && enabled.some((o) => o.sensitive),
                  student_ids: searching ? rows.map((s) => s.id) : null,
                },
                `학급명렬표 ${classes.data?.find((c) => c.id === classId)?.label ?? "전체"}${searching ? " 검색" : ""} ${todayText()}`,
              )
            }
          >
            엑셀로 저장{searching && <small className="search-scope"> (검색 결과 {rows.length}명)</small>}
          </button>
          <button onClick={() => window.print()} title={searching ? `찾은 학생 ${rows.length}명만 인쇄합니다` : "지금 보이는 표를 인쇄합니다"}>
            인쇄{searching && <small className="search-scope"> (검색 결과 {rows.length}명)</small>}
          </button>
        </div>
      </div>
      <p className="hint no-print">칸을 누르면 바로 고칠 수 있습니다. 이 정보는 이 PC 안에만 저장됩니다.</p>
      <div className="roster-scroll" ref={scrollRef}>
        <table className="roster-table" style={{ width: tableWidth }}>
          <colgroup>
            <col style={{ width: FIXED.class }} />
            <col style={{ width: FIXED.number }} />
            <col style={{ width: FIXED.name }} />
            {enabled.map((o, i) => (
              <col key={o.key} style={{ width: widths[i] }} />
            ))}
            <col className="no-print" style={{ width: FIXED.record }} />
          </colgroup>
          <thead>
            <tr>
              <th className="pin pin-1">학급</th>
              <th className="pin pin-2">번호</th>
              <th className="pin pin-3">이름</th>
              {enabled.map((o) => (
                <th key={o.key} className={o.field === "guardian_phone" ? "contacts-head" : undefined}>
                  {o.label}
                </th>
              ))}
              <th className="no-print">기록</th>
              {guardianOn && [1, 2, 3, 4].map((i) => <th key={`g${i}`} className="print-only">보호자 연락처{i}</th>)}
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <tr key={s.id} className={s.needs_review ? "review" : ""}>
                <Cell student={s} field="class_label" value={s.class_label ?? ""} placeholder="2-1" />
                <Cell student={s} field="number" value={s.number !== null ? String(s.number) : ""} placeholder="번호" />
                <Cell student={s} field="name" value={s.name} placeholder="이름" highlight={query} fresh={fresh?.has(s.id)} />
                {enabled.map((o) => {
                  if (o.field === "guardian_phone") return <ContactsCell key={o.key} student={s} masked={masked} />;
                  const raw = o.field === "birthday" ? s.birthday ?? "" : String(s[o.field as keyof Student] ?? "");
                  const display = o.field === "birthday" ? birthdayText(s.birthday) : masked && o.sensitive ? mask(raw, o.field) : undefined;
                  return <Cell key={o.key} student={s} field={o.field} value={raw} display={display} placeholder={o.placeholder} wide={o.field === "memo" || o.field === "address" || o.field === "remark"} />;
                })}
                <td className="no-print">
                  <button className="link" onClick={() => onOpen(s.id)}>
                    관찰 {s.observation_count}
                  </button>
                </td>
                {guardianOn &&
                  [0, 1, 2, 3].map((i) => {
                    const c = s.contacts[i];
                    return (
                      <td key={`g${i}`} className="print-only">
                        {c ? `${c.relation ? `${c.relation} ` : ""}${masked ? mask(c.phone, "phone") : c.phone}` : ""}
                      </td>
                    );
                  })}
              </tr>
            ))}
            <tr className="add-row no-print">
              <td>
                <input value={adding.class_label} onChange={(e) => setAdding({ ...adding, class_label: e.target.value })} placeholder="2-1" />
              </td>
              <td>
                <input value={adding.number} onChange={(e) => setAdding({ ...adding, number: e.target.value.replace(/\D/g, "") })} placeholder="번호" inputMode="numeric" />
              </td>
              <td colSpan={enabled.length + 2}>
                <div className="row gap">
                  <input
                    value={adding.name}
                    onChange={(e) => setAdding({ ...adding, name: e.target.value })}
                    onKeyDown={(e) => e.key === "Enter" && void addStudent()}
                    placeholder="이름을 적고 Enter — 학생 추가"
                  />
                  <button onClick={() => void addStudent()} disabled={!adding.name.trim()}>
                    추가
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      {list.data && list.data.length === 0 && <p className="muted">학생이 없습니다. 맨 아래 줄에서 추가하거나 ‘명단 붙여넣기’를 쓰세요.</p>}
      {searching && list.data && list.data.length > 0 && rows.length === 0 && <p className="muted">‘{query}’에 맞는 학생이 없습니다.</p>}
      {exporter.modal}
    </div>
  );
}
