# QO protocol conformance

Build the web assets with `pnpm build:web`. Run all four groups against the mock with `cargo test --locked -p qo-mock --test conformance`. Run the connection group against the server with `cargo test --locked -p qo-server --test conformance`. To run one group, append its module filter, for example `events::`.

| Group        | Contract                                                                                |
| ------------ | --------------------------------------------------------------------------------------- |
| `connection` | HTTP guards, version negotiation, login, desktop role and credential rotation           |
| `tasks`      | Creation, retries, takeover, cancellation, revocation, closure and model rerouting      |
| `events`     | Continuous cursors, replay into live delivery, text reconstruction and retention        |
| `approvals`  | Private requests, digest verification, one-time decisions, delivery, expiry and renewal |

The package integration tests select groups explicitly. Each group launches an isolated child and exercises the public wire protocol. The client validates every JSON-RPC result, error and notification and checks incoming frames for credentials used in that test.

For QO-02, register the other three groups in the real server's conformance test entry point. Supply a **test-build-only** harness adapter that reads `packages/mock-server/scenarios/`, and map the group's expiry and retention settings to the test launch configuration. Production builds must exclude that adapter and its scenario trigger.

## 中文

mock 运行四组，真服务当前运行连接组。QO-02 接入任务执行时，在真服务测试入口启用归属、事件和审批三组，用仅在测试构建中启用的适配器读取同一批场景。正式构建排除这条入口。
