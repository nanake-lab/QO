# QO

QO is a local Rust service for coding-agent clients. The Tauri v2 desktop app and the browser share a React/TypeScript frontend. This first slice starts the service, authenticates each client, and negotiates `qo.v1`. Task execution and approval delivery are planned for the Codex adapter.

## Run locally

Install Rust through rustup (this workspace pins 1.99.0), Node.js 24, pnpm 12.11.2 and the native dependencies for your platform. [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```sh
pnpm install --frozen-lockfile
pnpm build:web
cargo run --locked -p qo-desktop
```

The desktop app starts or attaches to the service. Closing the window hides it. Use the tray to restore the window, open QO in your default browser, or enable launch at login. Starting the desktop app again focuses its existing window. Quitting the shell leaves the independent service running.

For a browser-only session, start the service with `cargo run --locked -p qo-server`. While the service is running, choose **在浏览器中打开** in the tray or run **`qo open`**. Use `qo open --print` to print a fresh link for a headless session. The tray action and `qo open` both request a fresh one-use link and open the system default browser. From a source checkout the equivalent command is `cargo run --locked -p qo-server --bin qo -- open`. To put the command on your Cargo bin path, run `cargo install --locked --path packages/server --bin qo` after building the web assets.

The default listener is `127.0.0.1:47261`. The configuration location and the port and login-link lifetime settings are in the [architecture decision](docs/adr/0001-architecture.md).

## Development

The public schemas live in `packages/protocol/schema`. Use `pnpm generate` after a schema or harness-lock change. The generator also runs the pinned Codex CLI to export its server callback schema. Run the following checks after building the web assets:

```sh
pnpm generate:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo install --locked cargo-deny --version 0.22.1
cargo deny --locked check licenses
```

## 中文

QO 的 Rust 服务可以单独运行。桌面窗口和浏览器都加载服务提供的 React 网页，用一次性链接换取会话 cookie。关掉页面后，可以从托盘选择「在浏览器中打开」，也可以运行 `qo open`。当前可验证登录、握手和关窗后的后台运行；任务执行与审批交付按后续卡实施。[产品定义](docs/PRODUCT.md) · [架构决定](docs/adr/0001-architecture.md)
