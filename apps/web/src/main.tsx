import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { InitializeParams, InitializeResult } from "@qo/protocol";

async function browserHandshake(): Promise<InitializeResult> {
  const code = new URLSearchParams(location.hash.slice(1)).get("login");
  if (code) {
    history.replaceState(null, "", location.pathname);
    const response = await fetch("/auth/exchange", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code }),
      credentials: "same-origin",
    });
    if (!response.ok)
      throw new Error(
        "登录链接已失效。请从托盘选择「在浏览器中打开」，或运行 qo open。",
      );
  }
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://${location.host}/rpc`);
    const failed = () =>
      reject(
        new Error("请从托盘选择「在浏览器中打开」，或运行 qo open 登录。"),
      );
    socket.onopen = () => {
      const params: InitializeParams = { versions: ["qo.v1"] };
      socket.send(
        JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params }),
      );
    };
    socket.onmessage = (event) => {
      try {
        const reply = JSON.parse(event.data);
        if (reply.id !== 1 || reply.result?.version !== "qo.v1")
          throw new Error();
        resolve(reply.result);
      } catch {
        reject(new Error("服务协议不匹配。"));
      } finally {
        socket.close();
      }
    };
    socket.onerror = socket.onclose = failed;
  });
}

function App() {
  const [status, setStatus] = useState("正在连接本机服务…");
  useEffect(() => {
    browserHandshake()
      .then(({ version }) => setStatus(`本机服务已就绪 · ${version}`))
      .catch((error: unknown) =>
        setStatus(
          error instanceof Error ? error.message : "无法连接本机服务。",
        ),
      );
  }, []);
  return (
    <>
      <h1>QO</h1>
      <p role="status">{status}</p>
    </>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<App />);
