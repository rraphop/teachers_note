-- 할 일을 끝내면 남기는 완료 기록 (3차 6번)
--   업무와 연결된 할 일 → 그 업무의 업무 기록, 아니면 → 일반 기록 하나. 체크를 풀면 지운다.
--   from_task_id: 이 완료 기록을 만든 할 일 / ref_note_id: 그 할 일이 나온 원래 기록 (원래 기록이 지워져도 완료 기록은 남는다)
ALTER TABLE duty_logs ADD COLUMN from_task_id INTEGER;
ALTER TABLE duty_logs ADD COLUMN ref_note_id INTEGER;
ALTER TABLE notes ADD COLUMN from_task_id INTEGER;
ALTER TABLE notes ADD COLUMN ref_note_id INTEGER;
CREATE INDEX idx_duty_logs_task ON duty_logs(from_task_id);
CREATE INDEX idx_notes_task ON notes(from_task_id);
