# QO

QO is a local Rust service for coding-agent clients. The Tauri v2 desktop app and the browser share a React/TypeScript frontend. This first slice starts the service, authenticates each client, and negotiates `qo.v1`. Task execution and approval delivery are planned for the Codex adapter.

## Run locally

Install Rust through rustup (this workspace pins 1.99.0), Node.js 24, pnpm 12.11.2 and the native dependencies for your platform. [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```sh
pnpm install --frozen-lockfile
pnpm build:web
cargo run --locked -p qo-desktop
```

The desktop app starts or attaches to the service. Closing the window hides it. Use the tray to reopen it or enable launch at login. Quitting the shell leaves the independent service running.

For a browser-only session, run `cargo run --locked -p qo-server`. Open the one-use link printed in the terminal. To sign in again while the service is running, use `cargo run --locked -p qo-server -- login-link` in another terminal. The default listener is `127.0.0.1:47261`. The configuration location and the port and login-link lifetime settings are in the [architecture decision](docs/adr/0001-architecture.md).

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

QO 的 Rust 服务可以单独运行。桌面壳直接认证，浏览器用一次性链接登录，两者加载同一份 React 网页。当前可验证登录、握手和关窗后的后台运行；任务执行与审批交付按后续卡实施。[产品定义](docs/PRODUCT.md) · [架构决定](docs/adr/0001-architecture.md)
