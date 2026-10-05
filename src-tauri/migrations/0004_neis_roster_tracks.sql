-- 0.7: 나이스 명렬표 가져오기, 기록 두 갈래(일반 기록 / 정리된 업무기록)

-- 나이스 명렬표 항목 (값이 없으면 빈 문자열)
--   student_no : 학생개인번호 10자리. 학년이 올라가도 바뀌지 않아 같은 학생을 찾는 기준이 된다.
--   rrn        : 주민등록번호(외국인등록번호). 나이스가 뒷자리를 가려 내보낸 값 그대로.
--                설정에서 '주민등록번호 저장'을 켠 경우에만 채운다.
ALTER TABLE students ADD COLUMN student_no  TEXT NOT NULL DEFAULT '';
ALTER TABLE students ADD COLUMN postal_code TEXT NOT NULL DEFAULT '';
ALTER TABLE students ADD COLUMN prev_record TEXT NOT NULL DEFAULT '';   -- 이전학적정보 (예: 2학년1반1번)
ALTER TABLE students ADD COLUMN remark      TEXT NOT NULL DEFAULT '';   -- 나이스 비고
ALTER TABLE students ADD COLUMN rrn         TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_students_no ON students(student_no) WHERE student_no <> '';

-- 기록의 갈래: NULL = AI 분류를 따른다 (업무·할 일·일정·학생 관찰이 나오면 업무기록)
--             'general' / 'work' = 교사가 직접 옮김 (AI가 다시 분류해도 바꾸지 않는다)
ALTER TABLE notes ADD COLUMN track TEXT CHECK (track IN ('general', 'work'));
