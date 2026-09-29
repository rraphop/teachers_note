// WebView2 원격 디버깅(CDP)으로 실제 앱 화면을 조작하는 작은 도우미.
// 사용: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 으로 앱을 띄운 뒤 connect(9333)
import fs from "node:fs/promises";
import { spawn } from "node:child_process";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function waitFor(fn, { timeout = 30000, interval = 250, label = "조건" } = {}) {
  const start = Date.now();
  let last;
  while (Date.now() - start < timeout) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await sleep(interval);
  }
  throw new Error(`${label} 기다리다 시간 초과 (${timeout}ms): ${last instanceof Error ? last.message : JSON.stringify(last)}`);
}

export function launch(exe, { port = 9333, dataRoot, extraEnv = {} } = {}) {
  const env = {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    ...(dataRoot ? { TN_DATA_ROOT: dataRoot } : {}),
    ...extraEnv,
  };
  const child = spawn(exe, [], { env, detached: false, stdio: "ignore" });
  return child;
}

export async function connect(port = 9333, timeout = 40000) {
  const page = await waitFor(
    async () => {
      const r = await fetch(`http://127.0.0.1:${port}/json/list`);
      const list = await r.json();
      return list.find((t) => t.type === "page" && /tauri\.localhost|index\.html/.test(t.url));
    },
    { timeout, label: "WebView2 디버그 포트" },
  );
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = rej;
  });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const m = JSON.parse(typeof ev.data === "string" ? ev.data : ev.data.toString());
    if (m.id && pending.has(m.id)) {
      const { res, rej } = pending.get(m.id);
      pending.delete(m.id);
      if (m.error) rej(new Error(m.error.message));
      else res(m.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const i = ++id;
      pending.set(i, { res, rej });
      ws.send(JSON.stringify({ id: i, method, params }));
    });
  const evaluate = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  await send("Page.enable");
  await send("Runtime.enable");
  // 화면 조작 도우미를 페이지에 심는다
  await evaluate(`(() => {
    window.__e2e = {
      invoke: (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args || {}),
      byText: (text, sel = 'button') => [...document.querySelectorAll(sel)].find(e => (e.innerText || e.textContent || '').replace(/\\s+/g,' ').includes(text)),
      click: (text, sel = 'button') => { const e = window.__e2e.byText(text, sel); if (!e) throw new Error('못 찾음: ' + text); e.scrollIntoView({block:'center'}); e.click(); return true; },
      setValue: (el, value) => {
        const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
        el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
        return true;
      },
      type: (selector, value) => { const el = document.querySelector(selector); if (!el) throw new Error('입력칸 없음: ' + selector); return window.__e2e.setValue(el, value); },
      text: (sel = 'body') => (document.querySelector(sel)?.innerText || ''),
      nav: (label) => window.__e2e.click(label, '.sidebar button'),
    };
    return true;
  })()`);
  return {
    send,
    evaluate,
    invoke: (cmd, args) => evaluate(`window.__e2e.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args ?? {})})`),
    click: (text, sel) => evaluate(`window.__e2e.click(${JSON.stringify(text)}${sel ? `, ${JSON.stringify(sel)}` : ""})`),
    type: (sel, value) => evaluate(`window.__e2e.type(${JSON.stringify(sel)}, ${JSON.stringify(value)})`),
    text: (sel) => evaluate(`window.__e2e.text(${JSON.stringify(sel ?? "body")})`),
    nav: (label) => evaluate(`window.__e2e.nav(${JSON.stringify(label)})`),
    screenshot: async (path) => {
      const r = await send("Page.captureScreenshot", { format: "png" });
      await fs.writeFile(path, Buffer.from(r.data, "base64"));
      return path;
    },
    close: () => ws.close(),
  };
}
