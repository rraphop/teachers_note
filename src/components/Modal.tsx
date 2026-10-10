import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { IconClose } from "./Icons";

// 열린 창 수: 하나라도 열려 있으면 뒤 화면이 휠로 움직이지 않게 막는다
let openCount = 0;
function lockScroll(): () => void {
  const root = document.documentElement;
  if (openCount === 0) {
    // 스크롤 막대가 있던 화면이면 그 자리를 남겨 두어 본문이 옆으로 밀리지 않게
    root.classList.toggle("modal-gutter", root.scrollHeight > root.clientHeight);
    root.classList.add("modal-open");
  }
  openCount += 1;
  return () => {
    openCount = Math.max(0, openCount - 1);
    if (openCount === 0) root.classList.remove("modal-open", "modal-gutter");
  };
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    const first = ref.current?.querySelector<HTMLElement>("input, textarea, select, button.primary");
    first?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  useEffect(() => lockScroll(), []);
  // 창은 body 바로 아래에 그린다: 왼쪽 메뉴(업데이트 기록 등) 안에서 열어도
  // 본문의 급식 칸·아래 입력창보다 늘 위에 오고, 휠도 창이 받는다
  return createPortal(
    <div className="overlay" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog ${wide ? "wide" : ""}`} role="dialog" aria-modal="true" aria-label={title} ref={ref}>
        <div className="dialog-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="닫기" title="닫기 (Esc)">
            <IconClose />
          </button>
        </div>
        <div className="dialog-body">{children}</div>
        {footer && <div className="dialog-foot">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

// ───────────── 확인 창 ─────────────

interface ConfirmReq {
  message: string;
  okText: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
}

let push: ((r: ConfirmReq) => void) | null = null;

export function confirmDialog(message: string, opts: { okText?: string; danger?: boolean } = {}): Promise<boolean> {
  return new Promise((resolve) => {
    if (!push) {
      resolve(window.confirm(message));
      return;
    }
    push({ message, okText: opts.okText ?? "확인", danger: opts.danger ?? false, resolve });
  });
}

export function ConfirmHost() {
  const [req, setReq] = useState<ConfirmReq | null>(null);
  useEffect(() => {
    push = setReq;
    return () => {
      push = null;
    };
  }, []);
  if (!req) return null;
  const done = (ok: boolean) => {
    req.resolve(ok);
    setReq(null);
  };
  return (
    <Modal
      title="확인"
      onClose={() => done(false)}
      footer={
        <>
          <button onClick={() => done(false)}>취소</button>
          <button className={req.danger ? "danger" : "primary"} onClick={() => done(true)} autoFocus>
            {req.okText}
          </button>
        </>
      }
    >
      <p className="confirm-text">{req.message}</p>
    </Modal>
  );
}
