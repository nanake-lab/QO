# ADR 0001 · 本机服务、审批归属与事件回放

状态：提请评审。适用于 QO 第一版及其假服务端。

QO 在本机运行一个服务，网页和 Nemu 都向它派活。一个任务可以开多个会话，每个会话由服务启动一份 Codex。客户端能看到所有会话的进度，只有创建任务的客户端能处理它的审批。断线时，Codex 遇到审批会继续等，客户端回来后可以接着处理。QO 把状态变化写进本机数据库，用游标补发进度。用量按 Codex 实际给出的字段展示。

## 1. 进程归属

**候选**　每个客户端各管一套 harness，或由一个本机服务统一托管。

**决定 / 理由**　采用后一种。服务独占每个会话的子进程、stdio 和在途请求，可以把一份审批交给唯一的处理方。一个用户的数据目录同时只允许一个服务持有操作系统独占锁。客户端退出只释放连接。显式取消才打断执行。服务正常退出时回收进程树。服务崩溃后，使用随父进程关闭的作业或监督机制回收子进程。重启先对账，将失去 stdio 的非终态会话记为 `failed(harness_lost)`，撤销其未决审批并写入事件。用户可另开会话，旧命令不会被重发。

## 2. 连接、发现与握手

**候选**　Windows 命名管道可以使用系统 ACL，原生客户端支持好。浏览器需要桥接，其他平台还要维护另一套传输。回环 WebSocket 支持浏览器与原生客户端共用双向 RPC。

**决定 / 理由**　服务绑定 IPv4 字面地址 `127.0.0.1:0`，由系统分配端口。网页从同一端口的静态入口打开，业务接口统一为 `/rpc` 上的 WebSocket 和 JSON-RPC 2.0。启动顺序如下。

1. 持有实例锁以后，用系统 CSPRNG 生成 32 字节服务令牌，以无填充 base64url 编码（256 位随机性）。成功监听后原子替换运行目录的 `endpoint.json`，内容为 `{port,token}`。每次启动换令牌。Unix 私有目录权限为 0700，临时文件和最终文件为 0600。Windows 停止继承，在创建目录和临时文件时将访问权限制到当前用户 SID。无法建立私有权限就中止启动。目录位置见第 8 节。
2. 原生客户端读该文件，从端口构造地址。浏览器用户在本机打开该地址，将文件中的令牌粘贴到页面，页面只在内存保管它。静态入口只提供程序资源。文件、任务和令牌均不能从静态路径读取。刷新页面需要重新输入服务令牌。
3. 升级请求必须有且只有一个 `Host: 127.0.0.1:<实际端口>` 和一个精确匹配的 `Origin: http://127.0.0.1:<实际端口>`。原生客户端也发送该 Origin。缺失、多值、`null`、域名、其他端口及代理转发头都不能替代这两项校验。固定 Host 阻断 DNS 重绑定，同源检查阻断其他网页，令牌验证覆盖能自填请求头的客户端。
4. 浏览器 WebSocket 构造器通过子协议列表发送 `qo.rpc` 和 `qo.auth.<token>`。原生客户端用相同机制。服务在返回 101 之前校验令牌，只回显选中的 `qo.rpc`。每条客户端 RPC 请求（包括握手）还须携带 `params.token`。令牌用定长、常数时间比较，凭据头和消息正文不记日志。令牌不得放在 URL。错误的升级返回 403，未认证的消息不分派。
5. 首条请求为 `initialize({versions:["qo.v1"],token})`。服务从交集中选支持的最高版本，返回 `{version:"qo.v1"}`。没有交集返回 `unsupported_version` 并关闭。协商前不接受业务调用，每次重连重新协商。服务启动新的实例后重新读取发现文件。

## 3. 对象、状态与外部操作

**候选**　直接暴露 Codex 的线程和条目，或用 QO 的 ID 关联各家回报。前者会把协议升级、重试以及跨客户端显示都绑在一家 harness 上。

**决定 / 理由**　使用下表的 QO 对象。除特别说明外，ID 为小写 UUIDv4，每次创建生成一次，重启和回放沿用原值。父子关系由字段引用，不靠 ID 前缀解析。时间为服务器 UTC RFC 3339。`?` 表示可缺省。harness 自己的 ID 只进适配器映射。终态不可改写，补录观察值可追加新事件。

| 对象 | 字段 | 产生与状态变化 |
| --- | --- | --- |
| Task 任务 | taskId、name、group?、cwd、forbiddenRoots、state、createdAt | 创建方生成 taskId 和独立的任务密钥。open 可开多个会话。显式关闭且所有会话已终止后成为 closed。 |
| Session 会话 | sessionId、taskId、harness、model?、reasoningEffort?、serviceTier?、state、error?、createdAt | starting → idle。开始轮次进入 running。需要输入或审批时进入 waiting_input 或 waiting_approval。处理后回到相应等待态或 running。轮次结束回 idle，显式结束空闲会话成为 completed。启动或执行故障可成为 failed。显式取消可从任何非终态进入 cancelled。 |
| Turn 轮次 | turnId、sessionId、input、state、startedAt、endedAt? | 同一会话同时只有一个 active 轮次。running → completed / interrupted / failed。等待期间轮次仍为 running。追加指令属于同一轮。 |
| Item 条目 | itemId、turnId、kind、state、content、startedAt、endedAt? | assistant_message 累积文本。command_execution 记录命令、输出和可用的退出码。file_change 记录路径与变更。running → completed / failed / interrupted。一次性条目可直接以终态产生。 |
| ApprovalRequest 审批请求 | approvalId、taskId、sessionId、turnId、itemId、kind、raw、summary、digest、expiresAt、state | 新请求为 pending，只接受一份决定。可进入 decided、expired 或 withdrawn。续期发新 ID。私有载荷的可见性见第 5 节。 |
| ApprovalDecision 审批决定 | decisionId、approvalId、digest、choice、decidedAt、delivery | 创建方为每次决定生成 decisionId，choice 为 allow_once 或 deny。服务持久化后 delivery 为 recorded，交付后为 sent，无法确认则为 unknown，不重放授权。 |
| Usage 用量 | sessionId、turnId?、model?、reasoningEffort?、serviceTier?、source、counts、observedAt | 只读快照，以来源、会话及其统计维度为键，同一键由新快照替换。缺失值为 null。观察样本的唯一 ID 为对应事件游标。 |
| Event 事件 | cursor、at、kind、taskId、sessionId?、turnId?、payload | cursor 是事件 ID，由数据库事务分配。公开日志只追加。事件写入后不修改。 |

当同一轮有多个未决回调，展示优先级为等审批、等输入、在跑。逐一解决后重算状态。`turn.interrupt` 结束本轮，仍可开下一轮。`session.cancel` 终止整场会话及其在途请求。取消请求与完成回报并发时，以服务事务先提交的有效终态为准。

**供 2/2 实现的方法表**　每个方法的 `params` 均含服务令牌。除握手外，读操作对完成握手的连接开放。修改已存在任务的操作都校验该连接的任务归属。

| 方法 | 主要入参 | 结果或作用 |
| --- | --- | --- |
| initialize | versions | 选中的 version |
| task.create / task.attach | taskId、ownerKey。创建时另有 name、group?、forbiddenRoots?、cwd? | 创建或恢复该连接对任务的控制。创建重试须校验同一密钥和相同参数 |
| task.close | taskId | 关闭已无活动会话的任务 |
| state.get | 无 | 公共任务、会话、轮次、条目、用量的同一事务快照及其 cursor |
| session.open / session.finish / session.cancel | taskId。操作已有会话时带 sessionId，打开时可选 model、reasoningEffort、serviceTier | 返回会话。finish 只接受 idle |
| turn.start / turn.append / turn.interrupt | sessionId。开始或追加带 input，追加或打断还须带 turnId | 返回对应轮次。旧 turnId 返回 conflict |
| input.answer | sessionId、turnId、requestId、answer | 归还特定的待输入回调 |
| approval.list / approval.renew / approval.decide | taskId。续期带 approvalId，决定带 approvalId、decisionId、digest、choice | 所有者取回未决及可续期请求、重取当前载荷或提交一次决定 |
| usage.get | sessionId | 返回会话快照和有来源的档位分组 |
| events.subscribe | afterCursor | 确认订阅，随后按序送出 event 通知 |

公开通知 `event` 的 kind 为 `task.changed`、`session.changed`、`turn.changed`、`item.changed`、`usage.updated`。流式文本及命令输出用 `item.delta` 并携带 itemId。只向所有者发送的即时通知为 `approval.requested` 与 `input.requested`，后者带 requestId、问题与轮次引用。成功 attach 后重送仍有效的私有回调。错误用 JSON-RPC 标准错误码或服务错误，服务错误在 `error.data.reason` 中区分 `unauthorized`、`forbidden`、`unsupported_version`、`not_found`、`conflict`、`cursor_expired`、`digest_mismatch` 和 `approval_expired`。

## 4. 持久事件与断线补发

**候选**　短时内存队列适合实时推送，重启就失去历史。数据库顺序日志同时支持恢复与现在的订阅者。

**决定 / 理由**　公开事件采用连续递增的全局 64 位正整数游标，线上以十进制字符串表示，起点为 `"0"`。唯一写者在同一事务里更新投影、递增持久高水位并追加事件。回滚连同游标一起撤销。所有已认证客户端看到同一条公共序列，私有回调不占用公共游标。审批状态只公开“等审批”等会话状态，原始请求与决定保留在所有者通道。

`afterCursor=C` 表示客户端已应用 C，只接收大于 C 的事件。订阅者先登记实时队列并取已提交高水位 H，补发 (C,H]，再按序发送队列中大于 H 的事件。回放期间以一致的读取视图隔离裁剪。客户端在应用事件后才保存游标，收到已处理游标时去重。慢连接超出服务发送缓冲可断开，再用最后已应用的游标接续。

默认保留 7 天或 256 MiB 的事件有效载荷（含事件元数据，按编码后的 UTF-8 字节计），任一先达到即从最老记录连续裁剪。前者覆盖周末离线，后者限制高频输出的磁盘占用。两项均可配置。上限只约束保留的事件，SQLite 索引、事务日志与当前态另计。裁剪不重置高水位。若 C 早于可续接下界，返回 `cursor_expired` 及保留区间，客户端用 `state.get` 的快照和配套游标重订阅。C 超过高水位返回无效参数。新客户端也可走快照路径，实时流不会静默跳过缺口。

## 5. 审批归属、内容绑定与超时

**候选**　由界面代批、以可自报的 clientId 辨认所有者，或由任务创建方持有独立随机密钥。前两种无法隔开网页和 Nemu 的决定权。

**决定 / 理由**　创建方为每任务生成 32 字节随机 ownerKey，服务只持久化它的 SHA-256 校验值，按任务校验。服务令牌只证明能使用本机服务。成功创建或 attach 才将任务绑定到连接。同一所有者重新 attach 会撤下旧连接的处理权。Nemu 将密钥放在自己的用户私有存储。网页仅在内存持有任务密钥，创建者可导出到仅自己可读的本机文件。服务换端口后网页来源也变了，需要用恢复副本重新 attach。丢失密钥的任务可查看公开进度，不能从公共接口重新领取控制权。QO 没有客户端账户表。

1. 适配器保存上游 RPC 请求 ID 与进程实例的对应关系，连同原样的 method、params 和执行所需的命令或变更上下文，冻结为只读审批材料。请求缺少必要的命令、目标路径或待修改内容时，先关联同一 item 的数据。仍无法展示实际操作就中止该回调。人读的 summary 与核对用的 digest 都取自同一份材料。digest 使用 SHA-256，计算对象含 approvalId、任务/轮次/条目引用、原始请求、操作上下文、summary 和 expiresAt。用 RFC 8785 规范 JSON 的 UTF-8 字节，输出小写 hex。[RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
2. 通过所有者的私有通知或私有读取交付材料。公开状态、重放日志、诊断和测试样本不携带凭据、ownerKey 或审批原文。原始审批材料仅在处理中的服务内存保存。持久索引保留 ID、状态、到期时间和摘要哈希。命令、用户输入、差异和输出在进入投影或事件之前过滤凭据。无法安全公开的内容仅给状态。认证回调由 CLI 自行处理，不作为命令或审批展示。
3. 服务在同一事务校验归属、pending、到期时间与 digest，原子记录决定。摘要不符只拒绝本次提交。如果上游回调或关联条目在等待时变更了实际操作，立即撤销旧请求，按新材料重新索取决定。相同 decisionId 和相同内容的重试返回现有结果，复用 ID 改内容或竞争提交第二份决定返回 conflict。只向原来的上游回调交付一次，接受仅限这次操作，拒绝权利扩展、永久规则和会话级授权。交付和崩溃可能相邻。状态不明即标 unknown 并结束受影响会话，不通过重发来探测执行结果。
4. 审批有效期默认 10 分钟，可配置，给用户留出阅读时间并限制陈旧授权。客户端断线时解绑，上游继续等待。到期使旧 ID 失效，上游仍等待。所有者回来后用 list 查看，renew 仅对仍活着的原回调重新核对材料，签发新 ID、digest 和期限。取消、进程退出或上游撤回时撤销所有相关请求。与决定竞争时按事务顺序处理。没有定时批准、缺省批准或其他客户端接管的路径。

## 6. 适配器与版本边界

**候选**　让前端直接处理各家消息，或让服务内的适配器把消息翻译成上面的模型。

**决定 / 理由**　选择适配器。服务持有归属、状态和审批决定。适配器持有进程、线程、轮次和在途 RPC 的映射。所需接口如下。

| 适配器操作 | Codex 映射 |
| --- | --- |
| 版本与能力 | 启动前核对 CLI 版本。stdio 上 initialize / initialized 协商，记录版本和可用能力 |
| 开线程、发轮次 | thread/start、turn/start。保留上游线程与轮次 ID |
| 追加、打断 | turn/steer 带 expectedTurnId。turn/interrupt 指定线程和轮次 |
| 回调与回复 | item/commandExecution/requestApproval、item/fileChange/requestApproval、item/permissions/requestApproval 映射到审批。item/tool/requestUserInput 映射到输入。以原 RPC id 响应 |
| 条目、终态和用量 | 消费 item 变化、turn/completed、thread/tokenUsage/updated。上游撤回或解析失败时通知服务 |

每次启动核对 [harness 锁](../../config/harness.lock.json)。未知或不匹配的版本在新建会话时失败。新适配器必须说明每种回调怎样决定、取消、失效。无法可靠表达一次性同意的回调只能拒绝或中止。本卡固定 Codex CLI 0.162.1。QO-02 在 `packages/adapter-codex` 用该版本的 `codex app-server generate-ts --out <目录>` 生成稳定协议并随适配器提交。升级须同时更新锁、生成物与样本。对照依据来自固定版本：[Codex 发布记录](https://github.com/openai/codex/releases/tag/rust-v0.162.1)、[协议类型](https://github.com/openai/codex/tree/rust-v0.162.1/codex-rs/app-server-protocol/schema/typescript)。

每家适配器用脱敏的离线录制覆盖正常轮次、交错的审批回调、拒绝、打断、用量、撤销和异常退出。保留双向消息顺序、关联 ID 关系、预期 QO 输出。测试无需账户或模型。2/2 的假服务端和外部一致性测试只验证 QO 的线协议，真实厂商的转换由各自适配器卡验证。

## 7. Codex 隔离与工作目录

**候选**　复用用户现有 CLI 环境可减少配置。专用环境可以独立核验审批、工作目录和登录状态。

**决定 / 理由**　仅给 QO 启动的子进程设置专用 `CODEX_HOME=<状态目录>/codex`，启动 app-server 的私有 stdio。父进程和日常开发的 Codex 配置独立。凭据只来自明确传入子进程的环境变量或专用 CLI 的既有登录状态。服务不复制日常登录文件，不向 harness 传递 QO 令牌或任务密钥。

固定版本采用交互审批策略 `untrusted`、审批接收方 `user`、初始沙箱 `read-only`。核对 thread/start 返回的生效设置，禁用自动评审、会话放行和扩权规则。调用方不得覆盖。可继续执行哪些只读命令仍由该版本的内置可信规则决定，QO 只能转交 harness 实际发出的请求。写操作须有可核对的一次性审批与隔离边界，沙箱无法实现时终止该操作。[Codex 固定版配置](https://github.com/openai/codex/tree/rust-v0.162.1/codex-rs/app-server-protocol/schema/typescript/v2)

默认工作目录为受管根下按 taskId 新建的目录。同一任务内的会话共用它。客户端也可声明任务独占的现有目录及 forbiddenRoots。创建时和启动每个进程前，解析实际绝对路径，跟随符号链接及 Windows junction，按平台大小写和路径分量比较。目录位于禁用根内或覆盖禁用根、与其他活动任务目录重叠、无法解析，或进入 QO 私有数据区时，拒绝启动。目录选择限制的是 cwd。进程可访问的文件范围由沙箱另行执行。

## 8. 存储与恢复

**候选**　JSONL 容易逐行查看，但跨文件更新索引、审批状态和游标需要额外恢复协议。SQLite 可在一个事务完成这些操作。

**决定 / 理由**　服务独占 SQLite 写入，使用事务和 WAL。数据库保存任务与会话索引、当前展示状态、所有权校验值、审批决定元数据和公共事件。Codex 的原始线程档案留在专用 CODEX_HOME。QO 只保存向客户端提供状态和回放所需的投影。环境变量、登录文件、令牌和完整 harness 收发包不入库。

| 用途 | Windows | 其他系统 |
| --- | --- | --- |
| 状态、数据库、Codex 专用目录 | `%LOCALAPPDATA%/QO/state` | `$XDG_STATE_HOME/qo`，默认 `~/.local/state/qo` |
| 运行锁与 endpoint.json | `%LOCALAPPDATA%/QO/run` | `$XDG_RUNTIME_DIR/qo`，缺失时使用上述私有状态目录内的 `run` |
| 用户配置 | `%LOCALAPPDATA%/QO/config` | `$XDG_CONFIG_HOME/qo`，默认 `~/.config/qo` |

运行时文件的权限见第 2 节。状态目录也限当前用户。重启恢复事务、高水位与任务归属，处理失联会话后再开放订阅。默认工作根为 Windows 的 `%LOCALAPPDATA%/QO/workspaces` 或其他系统的 `$XDG_DATA_HOME/qo/workspaces`（默认 `~/.local/share/qo/workspaces`）。此路径可配置。启动时校验它与私有状态、配置、运行目录互不覆盖。[XDG 目录规范](https://specifications.freedesktop.org/basedir-spec/latest/)

## 9. 用量与档位

**候选**　按文本长度或价目表推算，或保存 harness 实际回报并标明来源和缺项。

**决定 / 理由**　采用后者。以下是从固定版生成类型核对的协议能力，本卡未执行付费轮次。

| 来源 | 可展示 | 统计口径 |
| --- | --- | --- |
| thread/tokenUsage/updated | threadId、turnId，total 与 last 两个计数组，及可空的 modelContextWindow。每组有 totalTokens、inputTokens、cachedInputTokens、cacheWriteInputTokens、outputTokens、reasoningOutputTokens | total 为来源给出的线程累计快照，新值覆盖旧值。last 是最近一次模型调用快照，不按通知次数累加。缓存、推理等子项单列，不再加回 total。 |
| account/usage/read 的可选 threadUsage.groups | 按实际返回的 model、reasoningEffort、speed 和计数字段展示。缺项为 null。上游还可返回标为 estimated 的积分或美元值 | 分组有来源才展示。与流式累计分别标明，不相加。大整数经无损解析后以十进制字符串传输。 |
| thread/start 与 model/rerouted | 会话选择的 model、serviceTier、reasoningEffort，及上游报告的模型改派 | 这些是上下文。流式计数本身没有模型档位。不能将整场用量归给最后一个模型，也不把 speed 当作已证实的 serviceTier。 |

拿不到实际账单金额、逐条命令的钱数或未回报的逐模型档位拆分。显示“拿不到”，和明确上报的 0 分开。账户级余额和限速窗口不能换算成会话消费。[计数字段](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/app-server-protocol/schema/typescript/v2/TokenUsageBreakdown.ts) · [线程分组](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/app-server-protocol/schema/typescript/v2/ThreadUsageBreakdownGroup.ts)

## 10. 附录 · 第二家 harness 的调研

**候选**　复用用户安装的 Gemini CLI 或 Antigravity CLI 的 ACP，或维护私有桥接。调研日期为 2026-11-27，仅核对官方文档、协议和许可说明。

**决定 / 理由**　第二家优先评估官方 ACP v1 的 stdio 接口，第一版只接 Codex。Google 已提供原生 ACP，技术上无需另造桥接。能否使用具体账号、能否把每个需审批操作交回创建者以及发行条款，须在第二家适配器卡选定版本后验证。

| 问题 | 核实结果 |
| --- | --- |
| Gemini 怎么连 | `gemini --acp` 在 stdio 上传输 JSON-RPC 2.0。Google 的方法列表写作 initialize、authenticate、newSession、loadSession、prompt、cancel。ACP v1 线协议中对应的会话方法名为 session/new、session/load、session/prompt、session/cancel。加载历史要看协商能力。[Gemini CLI ACP](https://geminicli.com/docs/cli/acp-mode/) · [ACP v1 会话协议](https://agentclientprotocol.com/protocol/v1/session-setup) |
| Antigravity 有没有 | 有。程序名是 `agy`，启动命令是 `agy acp`。官方说明它通过 stdio 运行 ACP v1，支持会话、流式工具动作和权限请求。具体版本可用 `agy acp --help` 检查。[CLI ACP](https://antigravity.google/docs/cli/acp) · [编辑器集成](https://antigravity.google/docs/ide/extensions/acp/) |
| 谁决定 | ACP agent 通过 session/request_permission 给客户端一组 optionId，暂停并等选择或取消。将一次性允许或拒绝映射为上游实际给的选项。不接受永久允许。Antigravity 的既有规则会影响是否产生回调。接入时须隔离配置、核对 strict 模式和实际录制，不满足时不能声称所有请求均经所有者审批。[ACP 权限往返](https://agentclientprotocol.com/protocol/v1/tool-calls) · [Antigravity 权限设置](https://antigravity.google/docs/cli/reference) |
| 哪些账号迁移 | Google 公告将 2026-06-18 的变化限定于个人免费渠道和 Google AI Pro/Ultra。Gemini Code Assist Standard/Enterprise 以及公告列出的付费 API key 渠道仍可使用 Gemini CLI。[Google 迁移公告](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/) |
| 许可证和分发 | Gemini CLI 源码为 Apache-2.0。Antigravity 个人版受 Google 通用条款及 Antigravity 附加条款约束，企业渠道依其适用协议。官方提供 ACP 集成文档，但未查到授予 QO 重新分发 Antigravity CLI 二进制的开源许可证。个人条款也对第三方软件使用服务有限制。后续应核对 QO 集成适用的条款和账号渠道，本仓库不捆绑该 CLI。[Gemini CLI 许可证](https://github.com/google-gemini/gemini-cli/blob/main/LICENSE) · [Antigravity 附加条款](https://antigravity.google/terms) |
