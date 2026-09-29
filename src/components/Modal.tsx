import { useEffect, useRef, useState, type ReactNode } from "react";
import { IconClose } from "./Icons";

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
  return (
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
    </div>
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
