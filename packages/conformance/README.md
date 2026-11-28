# QO protocol conformance

Build the web assets with `pnpm build:web`. Run all four groups against the mock with `cargo test --locked -p qo-mock --test conformance`. Run all four groups and crash recovery against the server with `cargo test --locked -p qo-server --features test-harness --test conformance`. To run one group, append its module filter, for example `events::`.

| Group        | Contract                                                                                |
| ------------ | --------------------------------------------------------------------------------------- |
| `connection` | HTTP guards, version negotiation, login, desktop role and credential rotation           |
| `tasks`      | Creation, retries, takeover, cancellation, revocation, closure and model rerouting      |
| `events`     | Continuous cursors, replay into live delivery, text reconstruction and retention        |
| `approvals`  | Private requests, digest verification, one-time decisions, delivery, expiry and renewal |

The package integration tests select groups explicitly. Each group launches an isolated child and exercises the public wire protocol. The client validates every JSON-RPC result, error and notification and checks incoming frames for credentials used in that test.

The server tests launch the real service entry point with the test-only `--test-harness` flag. Both targets use the scripts in `packages/mock-server/scenarios/`. The recovery test kills its child after a decision is recorded and before delivery, then checks the recovered snapshot against its event stream. Production server and desktop builds exclude the feature.

## 中文

真服务和 mock 都运行连接、任务、事件和审批四组测试。真服务另测进程强杀后的对账和事件补发。脚本 harness 仅在测试构建和 mock 中启用。
