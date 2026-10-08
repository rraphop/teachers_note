import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "../api";
import { useLoad, useStore } from "../lib/store";
import { setMask as saveMask, useMaskContacts } from "../lib/privacy";
import { nameMarks, studentMatcher } from "../lib/studentSearch";
import { confirmDialog } from "./Modal";
import { ContactsCell } from "./ProfileParts";
import type { AppSettings, RosterColumns, RosterField, Student } from "../types";
import { todayText, useExporter } from "./ExportTools";

// 열 폭 (3차 1번): 표는 늘 본문 폭을 채우고, 켜진 열이 칸마다 정한 비율대로 폭을 나눠 가진다.
// 칸마다 최소 폭이 있어, 최소 폭의 합이 화면보다 넓을 때만 가로 스크롤 (학급·번호·이름은 왼쪽 고정, 기록은 맨 오른쪽)
type ColSpec = { min: number; weight: number };
const OPTIONAL: Array<{ key: keyof RosterColumns; label: string; field: RosterField; placeholder: string; sensitive?: boolean } & ColSpec> = [
  { key: "phone", label: "전화번호", field: "phone", placeholder: "010-0000-0000", sensitive: true, min: 120, weight: 1.5 },
  { key: "guardian_phone", label: "보호자 연락처", field: "guardian_phone", placeholder: "010-0000-0000", sensitive: true, min: 140, weight: 2 },
  { key: "address", label: "주소", field: "address", placeholder: "주소", sensitive: true, min: 180, weight: 3 },
  { key: "gender", label: "성별", field: "gender", placeholder: "남/여", min: 52, weight: 0.8 },
  { key: "birthday", label: "생일", field: "birthday", placeholder: "2012-03-05 또는 3월 5일", min: 96, weight: 1.2 },
  { key: "memo", label: "특이사항", field: "memo", placeholder: "학생 개별 특이사항", min: 160, weight: 3 },
  { key: "student_no", label: "학생개인번호", field: "student_no", placeholder: "나이스 10자리", min: 110, weight: 1.5 },
  { key: "prev_record", label: "이전학적", field: "prev_record", placeholder: "예: 2학년1반1번", min: 110, weight: 1.5 },
  { key: "remark", label: "비고", field: "remark", placeholder: "나이스 비고", min: 130, weight: 2.5 },
];
/** 늘 있는 칸: 학급·번호·이름(왼쪽 고정)과 기록(맨 오른쪽) */
const FIXED: Record<"class" | "number" | "name" | "record", ColSpec> = {
  class: { min: 56, weight: 1 },
  number: { min: 48, weight: 0.7 },
  name: { min: 80, weight: 1.5 },
  record: { min: 76, weight: 1 },
};

/** 전체 폭을 비율대로 나눈다. 비율대로 하면 최소보다 좁아지는 칸은 최소로 두고 나머지를 다시 나눈다.
 *  최소 폭의 합이 전체보다 넓으면 모두 최소(가로 스크롤). 결과의 합은 전체 폭과 같다(정수 픽셀). */
export function fillWidths(specs: ColSpec[], available: number): number[] {
  const n = specs.length;
  const minSum = specs.reduce((a, s) => a + s.min, 0);
  if (n === 0) return [];
  if (available <= minSum) return specs.map((s) => s.min);
  const w = new Array<number>(n).fill(0);
  let free = specs.map((_, i) => i);
  let rest = available;
  for (;;) {
    const total = free.reduce((a, i) => a + specs[i]!.weight, 0);
    const low = free.filter((i) => (rest * specs[i]!.weight) / total < specs[i]!.min);
    if (low.length === 0) {
      for (const i of free) w[i] = (rest * specs[i]!.weight) / total;
      break;
    }
    for (const i of low) {
      w[i] = specs[i]!.min;
      rest -= specs[i]!.min;
    }
    free = free.filter((i) => !low.includes(i));
  }
  const out = w.map((x) => Math.floor(x));
  let left = Math.round(available) - out.reduce((a, b) => a + b, 0);
  for (let i = n - 1; left > 0; i = (i - 1 + n) % n, left--) out[i] = out[i]! + 1;
  return out;
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
  const [newClass, setNewClass] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const exporter = useExporter();
  const classes = useLoad(() => api.listClasses(), []);
  const list = useLoad(() => api.listStudents(null, classId), [classId]);
  // 학생 추가 줄의 번호(그 학급 마지막 번호 + 1)는 학급 버튼과 상관없이 전체 명단으로 센다
  const everyone = useLoad(() => api.listStudents(null, null), []);
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
  // 학생 추가 줄 (3차 3번): 학급은 지금 보는 학급(전체 학급이면 담임반), 번호는 그 학급 마지막 번호 + 1 을 실제 값으로 채운다
  const viewLabel = classId === null ? settings?.homeroom || classes.data?.[0]?.label || "" : classes.data?.find((c) => c.id === classId)?.label ?? "";
  const nextNumber = (label: string): string => {
    if (!label) return "";
    const nums = (everyone.data ?? []).filter((s) => s.class_label === label && s.number !== null).map((s) => s.number!);
    return String(nums.length ? Math.max(...nums) + 1 : 1);
  };
  useEffect(() => {
    setNewClass(false);
    setAdding((a) => ({ ...a, class_label: viewLabel, number: nextNumber(viewLabel) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewLabel]);
  useEffect(() => {
    // 명단이 바뀌면(추가·가져오기) 번호를 다시 센다
    setAdding((a) => (a.class_label ? { ...a, number: nextNumber(a.class_label) } : a));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [everyone.data]);
  const pickClass = (label: string) => setAdding((a) => ({ ...a, class_label: label, number: nextNumber(label) }));
  const addStudent = async () => {
    const name = adding.name.trim();
    if (!name) return;
    const num = adding.number ? Number(adding.number) : null;
    const label = adding.class_label.trim();
    const same = num !== null && (everyone.data ?? []).find((s) => s.class_label === label && s.number === num);
    if (same && !(await confirmDialog(`${label} ${num}번에 이미 ${same.name} 학생이 있습니다. 그래도 추가할까요?`, { okText: "추가" }))) {
      nameRef.current?.focus();
      return;
    }
    try {
      await api.createStudent({ name, class_label: label || null, number: num });
      setAdding({ ...adding, number: num !== null ? String(num + 1) : "", name: "" });
      bump();
      everyone.reload();
    } catch (e) {
      toast(errorText(e), "error");
    }
    nameRef.current?.focus();
  };
  const enabled = OPTIONAL.filter((o) => cols?.[o.key]);
  // 학급·번호·이름 + 켠 칸 + 기록 — 표 폭 전체를 비율대로
  const all = fillWidths([FIXED.class, FIXED.number, FIXED.name, ...enabled, FIXED.record], boxWidth - 2);
  const [wClass, wNumber, wName] = [all[0]!, all[1]!, all[2]!];
  const widths = all.slice(3, 3 + enabled.length);
  const wRecord = all[all.length - 1]!;
  const tableWidth = all.reduce((a, b) => a + b, 0);
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
        <table className="roster-table" style={{ width: tableWidth, ["--pin2" as string]: `${wClass}px`, ["--pin3" as string]: `${wClass + wNumber}px` }}>
          <colgroup>
            <col style={{ width: wClass }} />
            <col style={{ width: wNumber }} />
            <col style={{ width: wName }} />
            {enabled.map((o, i) => (
              <col key={o.key} style={{ width: widths[i] }} />
            ))}
            <col className="no-print" style={{ width: wRecord }} />
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
                {/* 학급이 아직 없거나 목록을 불러오는 중이면 입력칸 (드롭다운에 없는 값을 고르면 빈 값이 되므로) */}
                {newClass || !classes.data?.length ? (
                  <input
                    value={adding.class_label}
                    onChange={(e) => pickClass(e.target.value)}
                    placeholder="예: 2-3"
                    aria-label="새 학급"
                    autoFocus={newClass}
                  />
                ) : (
                  <select
                    value={adding.class_label}
                    aria-label="추가할 학생의 학급"
                    onChange={(e) => {
                      if (e.target.value === "__new__") {
                        setNewClass(true);
                        pickClass("");
                      } else pickClass(e.target.value);
                    }}
                  >
                    {!classes.data?.some((c) => c.label === adding.class_label) && <option value={adding.class_label}>{adding.class_label || "학급 없음"}</option>}
                    {classes.data?.map((c) => (
                      <option key={c.id} value={c.label}>
                        {c.label}
                      </option>
                    ))}
                    <option value="__new__">+ 새 학급</option>
                  </select>
                )}
              </td>
              <td>
                <input value={adding.number} onChange={(e) => setAdding({ ...adding, number: e.target.value.replace(/\D/g, "") })} placeholder="번호" inputMode="numeric" aria-label="추가할 학생의 번호" />
              </td>
              <td colSpan={enabled.length + 2}>
                <div className="row gap">
                  <input
                    ref={nameRef}
                    aria-label="추가할 학생의 이름"
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
