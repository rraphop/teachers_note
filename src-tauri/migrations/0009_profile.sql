-- 기록 속 학생 개인정보를 학생 명렬표로 (2차 수정목록 8번)

-- 보호자 연락처: 학생마다 최대 4개 (관계 + 번호). students.guardian_phone 은 첫 번호로 계속 맞춰 둔다(내보내기 호환)
CREATE TABLE student_contacts (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  student_id INTEGER NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  relation   TEXT NOT NULL DEFAULT '',
  phone      TEXT NOT NULL,
  pos        INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_contacts_student ON student_contacts(student_id, pos);
-- 예전 보호자 연락처 한 칸을 첫 연락처로 옮긴다
INSERT INTO student_contacts(student_id, relation, phone, pos, created_at, updated_at)
  SELECT id, '', TRIM(guardian_phone), 0, updated_at, updated_at FROM students WHERE TRIM(guardian_phone) <> '';

-- 기록에서 찾은 개인정보: 넣음(applied) · 확인 기다림(pending) · 그대로 둠(rejected). 바뀐 이력도 이 표에 남는다.
--   field: phone | guardian | address | gender | birthday | student_no | prev_record
--   target_id: 보호자 연락처를 바꿀 때 바꿀 연락처 (4개가 찼을 때는 교사가 고른다)
CREATE TABLE profile_changes (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  note_id     INTEGER REFERENCES notes(id) ON DELETE SET NULL,
  student_id  INTEGER NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  field       TEXT NOT NULL,
  relation    TEXT NOT NULL DEFAULT '',
  old_value   TEXT NOT NULL DEFAULT '',
  new_value   TEXT NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('applied', 'pending', 'rejected')),
  target_id   INTEGER,
  source      TEXT NOT NULL DEFAULT 'rule',
  created_at  TEXT NOT NULL,
  decided_at  TEXT
);
CREATE INDEX idx_profile_changes_note ON profile_changes(note_id);
CREATE INDEX idx_profile_changes_student ON profile_changes(student_id, status);

-- 특이사항으로 보이는 내용: 어디에 둘지 교사에게 묻는다 (record = 학생부 자료, roster = 학생 명렬표 특이사항)
CREATE TABLE note_questions (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  note_id     INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  student_id  INTEGER REFERENCES students(id) ON DELETE CASCADE,
  text        TEXT NOT NULL,
  area        TEXT NOT NULL DEFAULT 'none',
  status      TEXT NOT NULL CHECK (status IN ('pending', 'record', 'roster')),
  created_at  TEXT NOT NULL,
  decided_at  TEXT
);
CREATE INDEX idx_note_questions_note ON note_questions(note_id, status);
