# QO web client

This React/TypeScript workspace renders in both Tauri v2 and a local browser. Build the shared assets with `pnpm build:web` from the repository root. The local service serves the frontend to both clients. The page exchanges its one-use link for a session cookie and connects to the same-origin WebSocket at `/rpc`. The task and approval views belong to Antigravity.

网页与桌面使用同一份构建产物。启动方式见[项目首页](../../README.md)，任务界面的设计依据见[产品定义](../../docs/PRODUCT.md)。
