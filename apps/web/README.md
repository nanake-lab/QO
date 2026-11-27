# QO web client

在仓库根目录安装前端依赖（`pnpm install --frozen-lockfile`）后，使用两个终端。

1. 运行 `cargo run -p qo-mock -- --seed external`。mock 打印监听地址、隔离目录和一次性登录链接。示例任务由一个外部客户端创建，会话正在等批准。
2. 在另一个终端运行 `pnpm --filter @qo/web dev`。Vite 会重新构建 `apps/web/dist`。
3. 用浏览器打开第一个终端打印的登录链接。页面显示 `qo.v1` 后即可开发。改完代码，等构建完成后刷新。链接过期时重启 mock 取得新链接。

页面、换码和 WebSocket 都来自 mock 的同一个回环地址，沿用正式服务的 Host、Origin 和 cookie 校验。关闭 mock 后内存场景会清空。[场景脚本](../../packages/mock-server/scenarios/README.md) · [协议](../../packages/protocol/schema/methods.json)
