import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { useLoad, useStore } from "../lib/store";
import type { AppSettings, RosterColumns, RosterField, Student } from "../types";
import { todayText, useExporter } from "./ExportTools";

const OPTIONAL: Array<{ key: keyof RosterColumns; label: string; field: RosterField; placeholder: string; sensitive?: boolean }> = [
  { key: "phone", label: "전화번호", field: "phone", placeholder: "010-0000-0000", sensitive: true },
  { key: "guardian_phone", label: "보호자 연락처", field: "guardian_phone", placeholder: "010-0000-0000", sensitive: true },
  { key: "address", label: "주소", field: "address", placeholder: "주소", sensitive: true },
  { key: "gender", label: "성별", field: "gender", placeholder: "남/여" },
  { key: "birthday", label: "생일", field: "birthday", placeholder: "2012-03-05 또는 3월 5일" },
  { key: "memo", label: "특이사항", field: "memo", placeholder: "학생 개별 특이사항" },
  { key: "student_no", label: "학생개인번호", field: "student_no", placeholder: "나이스 10자리" },
  { key: "prev_record", label: "이전학적", field: "prev_record", placeholder: "예: 2학년1반1번" },
  { key: "remark", label: "비고", field: "remark", placeholder: "나이스 비고" },
];

const MASK_KEY = "tn.roster.mask";

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
}: {
  student: Student;
  field: RosterField;
  value: string;
  display?: string;
  placeholder: string;
  wide?: boolean;
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
        {display ?? value ? <span>{display ?? value}</span> : <span className="ph">{placeholder}</span>}
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

export function RosterTable({ onOpen }: { onOpen: (id: number) => void }) {
  const { toast, bump } = useStore();
  const [classId, setClassId] = useState<number | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [masked, setMasked] = useState<boolean>(() => {
    try {
      return window.localStorage.getItem(MASK_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [adding, setAdding] = useState({ class_label: "", number: "", name: "" });
  const exporter = useExporter();
  const classes = useLoad(() => api.listClasses(), []);
  const list = useLoad(() => api.listStudents(null, classId), [classId]);
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
  const setMask = (m: boolean) => {
    setMasked(m);
    try {
      window.localStorage.setItem(MASK_KEY, m ? "1" : "0");
    } catch {
      /* 저장 못 해도 동작 */
    }
  };
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
            <label className="check">
              <input type="checkbox" checked={masked} onChange={(e) => setMask(e.target.checked)} /> 연락처·주소 가리기 (수업 화면에 띄울 때)
            </label>
          )}
          <span className="grow" />
          <button
            disabled={exporter.busy || !settings}
            title="지금 보이는 학급·항목 그대로 엑셀 파일로 저장합니다"
            onClick={() =>
              void exporter.run(
                {
                  format: "xlsx",
                  sheets: ["roster"],
                  class_id: classId,
                  roster_columns: settings?.roster_columns ?? null,
                  mask_contacts: masked && enabled.some((o) => o.sensitive),
                },
                `학급명렬표 ${classes.data?.find((c) => c.id === classId)?.label ?? "전체"} ${todayText()}`,
              )
            }
          >
            엑셀로 저장
          </button>
          <button onClick={() => window.print()}>인쇄</button>
        </div>
      </div>
      <p className="hint no-print">칸을 누르면 바로 고칠 수 있습니다. 이 정보는 이 PC 안에만 저장됩니다.</p>
      <div className="roster-scroll">
        <table className="roster-table">
          <thead>
            <tr>
              <th>학급</th>
              <th>번호</th>
              <th>이름</th>
              {enabled.map((o) => (
                <th key={o.key}>{o.label}</th>
              ))}
              <th className="no-print">기록</th>
            </tr>
          </thead>
          <tbody>
            {list.data?.map((s) => (
              <tr key={s.id} className={s.needs_review ? "review" : ""}>
                <Cell student={s} field="class_label" value={s.class_label ?? ""} placeholder="2-1" />
                <Cell student={s} field="number" value={s.number !== null ? String(s.number) : ""} placeholder="번호" />
                <Cell student={s} field="name" value={s.name} placeholder="이름" />
                {enabled.map((o) => {
                  const raw = o.field === "birthday" ? s.birthday ?? "" : String(s[o.field as keyof Student] ?? "");
                  const display = o.field === "birthday" ? birthdayText(s.birthday) : masked && o.sensitive ? mask(raw, o.field) : undefined;
                  return <Cell key={o.key} student={s} field={o.field} value={raw} display={display} placeholder={o.placeholder} wide={o.field === "memo" || o.field === "address"} />;
                })}
                <td className="no-print">
                  <button className="link" onClick={() => onOpen(s.id)}>
                    관찰 {s.observation_count}
                  </button>
                </td>
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
      {exporter.modal}
    </div>
  );
}
