# QO

QO is a local service for running coding agents. A browser or a desktop client such as Nemu can follow sessions, see usage, and answer approval requests for the tasks it creates. The first integration targets Codex app-server.

This repository currently contains the architecture and workspace skeleton. The public protocol belongs in `packages/protocol`, the service in `packages/server`, the Codex integration in `packages/adapter-codex`, and the browser client in `apps/web`.

## Development

Use Node.js 24 and pnpm 12.11.2. From the repository root, run:

```sh
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm test
```

## 中文

QO 在本机统一展示编程助手的会话、审批和用量。网页和 Nemu 都可以创建任务，各自负责自己任务的审批。仓库目前提供工程骨架，第一版将接入 Codex。详见[产品定义](docs/PRODUCT.md)和[架构决定](docs/adr/0001-architecture.md)。
