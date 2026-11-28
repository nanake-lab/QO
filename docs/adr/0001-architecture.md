# ADR 0001 · 本机服务、审批归属与事件回放

状态：提请评审。适用于 QO 第一版及其假服务端。

QO 的 Rust 服务在本机后台运行，独占它启动的编程助手。Tauri v2 桌面壳和浏览器使用同一份 TypeScript/React 网页。关闭桌面窗口以后，可以从托盘再打开，任务仍由服务接管。桌面窗口和浏览器都从服务加载网页，凭一次性链接登录；Nemu 用本机服务令牌连接。服务按任务归属转交审批，并按游标向客户端补发进度。QO 的 JSON Schema 同时生成 Rust 和 TypeScript 类型。

## 1. 进程归属

**候选**　每个客户端各管一套 harness，或由一个本机服务统一托管。

**决定 / 理由**　采用独立的 Rust 服务进程。桌面壳先与已有实例握手，必要时以独立的 `--serve` 进程启动同一个服务入口。无界面时可直接运行 `qo-server`。关闭窗口仅隐藏 WebView，托盘可恢复窗口。退出桌面壳也不结束服务。托盘采用 Tauri v2 核心的 `tray-icon` 功能，系统通知与用户可选的登录自启采用官方 notification、autostart 插件。官方 single-instance 插件先于其他插件注册。再次启动时，已有实例显示并聚焦主窗口；登录自启以隐藏窗口模式连接服务。服务独占每个会话的子进程、stdio 和在途请求，可以把一份审批交给唯一的处理方。一个用户的数据目录同时只允许一个服务持有操作系统独占锁。客户端退出只释放连接。显式取消才打断执行。服务正常退出时回收进程树。服务崩溃后，使用随父进程关闭的作业或监督机制回收子进程。重启先对账，将失去 stdio 的非终态会话记为 `failed(harness_lost)`，撤销其未决审批并写入事件。用户可另开会话，旧命令不会被重发。

## 2. 连接、发现与握手

**候选**　Windows 命名管道可以使用系统 ACL，原生客户端支持好。浏览器需要桥接，其他平台还要维护另一套传输。回环 WebSocket 支持浏览器与原生客户端共用双向 RPC。

**决定 / 理由**　QO 对客户端使用回环 WebSocket 和 JSON-RPC 2.0，业务入口是 `/rpc`。QO 与 Codex 之间使用独占的 stdio。两段传输分别协商协议。

1. 服务只绑定 `127.0.0.1`，默认端口 **47261**，固定地址可供书签和桌面复用。可在第 8 节配置目录的 `settings.json` 设置 `port`；占用时报告错误并退出。启动先持有运行锁，用系统随机源生成独立的 32 字节原生凭据和桌面凭据，以临时文件原子替换 `endpoint.json` 和 `desktop.json`。两份记录只有端口和本次启动的令牌。Unix 目录 0700、记录 0600；Windows 在发布任何凭据前，为私有运行目录设置受保护、可继承的所有者 DACL。
2. 外部原生客户端（含 Nemu）读取 `endpoint.json`。桌面壳和 `qo open` 使用服务为内置界面签发的本机令牌（`desktop.json`），在 WebSocket 升级时发送 Bearer 和精确的 Origin，再以 `ui.loginLink` 请求新链接。只有这个原生角色可以签发链接，服务令牌不进入网页。壳把链接交给主 WebView；网页、脚本及 CSP 都来自服务，并通过同源 `/rpc` 与服务通信。壳不打包第二份页面，也不转发业务 RPC。
3. 登录链接形如 `http://127.0.0.1:<端口>/#login=<一次性码>`。服务启动时可在交互终端展示首个链接。关掉浏览器页面后，用户可以从托盘选「在浏览器中打开」或运行 `qo open`。两者都先申请新链接，再交给系统默认浏览器打开。新码取代旧码。默认有效 120 秒，供本机切换浏览器，可配置 `loginLinkSeconds`。一次性码是凭据进入 URL 的唯一例外，放在片段中以避开 HTTP 请求行。登录脚本先清除片段，再向同源 `POST /auth/exchange` 发送码；服务原子消费后设置 HttpOnly、SameSite=Strict、Path=/rpc 的 host-only 会话 cookie。升级时浏览器自动发送 cookie。服务重启使所有旧 cookie、链接与原生令牌失效。
4. 静态入口核对实际 Host。换码和 WebSocket 升级同时核对唯一的 Host 和 Origin，精确匹配 `127.0.0.1:<端口>` 与 `http://127.0.0.1:<端口>`。原生客户端也带该 Origin。无效、缺失或多值均拒绝，不采信转发头。网页跨端口访问由 Origin 校验阻断，Host 校验阻断 DNS 重绑定。Cookie 和 SameSite 均不区分端口，同一主机其他端口在路径匹配时也可能收到 cookie。此设计针对外部网页，同一操作系统用户下已运行的恶意程序在威胁范围之外。Linux 和 macOS 上，`qo open` 和托盘「在浏览器中打开」交给浏览器的一次性链接会短暂出现在进程参数里，其他本机用户可能看到并抢先使用。第一版接受此限制；后续可采用 Jupyter 的方式，打开权限为 0600 的跳转页文件。Windows 默认权限下，普通用户无法读取其他用户进程的参数。
5. 在返回 101 前完成认证，把客户端角色绑定到连接。RPC 帧不重复带凭据。首条请求为 `initialize({versions:["qo.v1"]})`，成功返回选中的 `version`。无共同版本返回 `unsupported_version` 并断开。重连时重新认证和协商。本阶段已实现握手、换码及签发；任务和事件方法列在下节，供后续契约与服务使用。

## 3. 对象、状态与外部操作

**候选**　直接暴露 Codex 的线程和条目，或用 QO 的 ID 关联各家回报。前者会把协议升级、重试以及跨客户端显示都绑在一家 harness 上。

**决定 / 理由**　使用下表的 QO 对象。除特别说明外，ID 为小写 UUIDv4，创建任务、开会话、开始轮次和追加指令的客户端分别生成 taskId、sessionId、turnId 和 appendId。重试、重启和回放沿用原值。父子关系由字段引用，不靠 ID 前缀解析。时间为服务器 UTC RFC 3339。`?` 表示可缺省。harness 自己的 ID 只进适配器映射。终态不可改写，补录观察值可追加新事件。

| 对象 | 字段 | 产生与状态变化 |
| --- | --- | --- |
| Task 任务 | taskId、name、group?、cwd、forbiddenRoots、createdBy、state、createdAt、closedAt? | 创建方生成 taskId。服务登记内置界面归属，外部创建方另生成任务密钥。open 可开多个会话。createdBy 为 ui 或 external。open → closed 仅由创建者显式关闭；还有非终态会话时返回 conflict。全部会话结束后任务仍为 open。关闭后保留历史与事件，session.open 返回 conflict，第一版不支持重开。 |
| Session 会话 | sessionId、taskId、harness、harnessVersion、model?、reasoningEffort?、serviceTier?、state、error?、createdAt、cancelledBy?、endedAt? | 客户端生成 sessionId。harnessVersion 是启动前核对的 CLI 版本（mock 为 crate 版本）。model、reasoningEffort、serviceTier 取上游在 thread/start 返回的生效值，未报告的字段省略。上游报告 model/rerouted 时更新 Session 并发 session.changed。starting → idle。开始轮次进入 running。需要输入或审批时进入 waiting_input 或 waiting_approval。处理后回到相应等待态或 running。轮次结束回 idle，显式结束空闲会话成为 completed。启动或执行故障可成为 failed。显式取消可从任何非终态进入 cancelled。此时必须记录 cancelledBy（外部创建者为 owner，内置界面为 ui）。session.changed 向所有观察者公布该值。 |
| Turn 轮次 | turnId、sessionId、input、state、startedAt、endedAt? | 客户端生成 turnId，每次追加另生成 appendId。同一会话同时只有一个 active 轮次。running → completed / interrupted / failed。等待期间轮次仍为 running。追加指令属于同一轮。 |
| Item 条目 | itemId、turnId、kind、state、content、startedAt、endedAt? | assistant_message 累积文本。command_execution 记录命令、输出和可用的退出码。file_change 记录路径与变更。running → completed / failed / interrupted。一次性条目可直接以终态产生。 |
| ApprovalRequest 审批请求 | approvalId、taskId、sessionId、turnId?、itemId?、kind、raw、summary、digest、expiresAt、state | 新请求为 pending，只接受一份决定。可进入 decided、expired 或 withdrawn。续期发新 ID。私有载荷的可见性见第 5 节。 |
| ApprovalDecision 审批决定 | decisionId、approvalId、digest、choice、answer?、decidedAt、delivery | 创建方为每次决定生成 decisionId，choice 为 allow_once 或 deny。征询类请求可带所有者明确填写且符合请求表单的回答。服务持久化后 delivery 为 recorded，交付后为 sent，无法确认则为 unknown，不重放授权。 |
| ApprovalStatus 审批状态 | approvalId、taskId、sessionId、kind、state、expiresAt、digest、decision? | 所有者可查询任务整个存续期内的记录，包括关闭后。已有决定时提供 decisionId、choice、decidedAt、delivery。 |
| Usage 用量 | sessionId、turnId?、model?、reasoningEffort?、serviceTier?、source、counts、observedAt | 只读快照，以来源、会话及其统计维度为键，同一键由新快照替换。缺失的计数为 null，已知的零写为十进制字符串 `"0"`。缺失的统计维度可省略。观察样本的唯一 ID 为对应事件游标。 |
| Event 事件 | cursor、at、kind、taskId、sessionId?、turnId?、payload | cursor 是事件 ID，由数据库事务分配。公开日志只追加。事件写入后不修改。 |

当同一轮有多个未决回调，展示优先级为等审批、等输入、在跑。逐一解决后重算状态。`turn.interrupt` 结束本轮，仍可开下一轮。`session.cancel` 终止整场会话及其在途请求。取消请求与完成回报并发时，以服务事务先提交的有效终态为准。

**方法表**　服务令牌和会话 cookie 只在升级时出现。修改已存在任务的操作都校验该连接的任务归属，唯一例外是内置界面可以对任何未结束的会话调用 `session.cancel`。

| 方法 | 主要入参 | 结果或作用 |
| --- | --- | --- |
| initialize | versions | 选中的 version |
| ui.loginLink | 无业务参数，仅私有桌面凭据可调用 | 签发主窗口或默认浏览器可消费的一次性 url |
| task.create / task.attach | taskId，外部客户端另带 ownerKey。创建时另有 name、group?、forbiddenRoots?、cwd? | 创建或恢复该连接对任务的控制。创建重试须校验同一归属和相同参数 |
| task.close | taskId | 关闭已无活动会话的任务 |
| state.get | 无 | 公共任务、会话、轮次、条目、用量的同一事务快照及其 cursor |
| session.open / session.finish / session.cancel | taskId、sessionId。打开时客户端指定新 sessionId，可选 model、reasoningEffort、serviceTier | 返回会话。finish 只接受 idle |
| turn.start / turn.append / turn.interrupt | sessionId、turnId。开始时客户端指定新 turnId。开始或追加带 input，每次追加必带新的 appendId | 返回对应轮次。对已结束轮次的新追加或打断返回 conflict |
| input.answer | sessionId、turnId、requestId、answers | 按问题 ID 回答对应的待输入请求。每个值的形状为 `{"answers":["…"]}` |
| approval.list / approval.renew / approval.decide | taskId。续期带 approvalId，决定带 approvalId、decisionId、digest、choice | 所有者取回未决及可续期请求、重取当前载荷或提交一次决定（征询的允许决定可另带 answer） |
| approval.get | taskId、approvalId | 返回 ApprovalStatus。通过 list 或 renew 读取仍可处理的审批材料。 |
| usage.get | sessionId | 返回会话快照和有来源的档位分组 |
| events.subscribe | afterCursor | 确认订阅，随后按序送出 event 通知 |

`task.create`、`session.open`、`turn.start`、`turn.append` 先核对目标任务归属，再查客户端 ID，最后检查对象状态。非所有者返回 `forbidden`。在同一方法下，同一 ID 和按 RFC 8785 规范化后相同的完整 params 原样返回首次成功结果，包括当时的时间和状态。重试不追加事件或重复驱动 harness。复用 ID 改参数、跨对象或跨任务复用均返回 `conflict`。防重记录和第一次写入在同一事务提交，保存至对应任务、会话或轮次清理。外部客户端换连接后用 `task.attach` 恢复归属，再重试。

**方法权限**　外部连接的「自己的任务」指创建成功或凭任务密钥重新 attach 后由该连接控制的任务。内置界面重新连接后自动拥有界面建过的任务。桌面原生凭据仅用于发放登录链接，WebView 换码后按内置界面角色连接。

| 方法 | 内置界面 | 外部原生客户端 | 桌面原生凭据 |
| --- | --- | --- | --- |
| initialize | 可以 | 可以 | 可以 |
| ui.loginLink | 不可以 | 不可以 | 可以 |
| state.get、usage.get、events.subscribe | 可以 | 可以 | 不可以 |
| task.create | 可以，省略 ownerKey | 可以，必须带 ownerKey | 不可以 |
| task.attach | 不可以 | 只限自己的任务，校验 ownerKey | 不可以 |
| session.cancel | 可以，任何任务 | 只限自己的任务 | 不可以 |
| task.close、session.open、session.finish | 只限自己的任务 | 只限自己的任务 | 不可以 |
| turn.start、turn.append、turn.interrupt、input.answer | 只限自己的任务 | 只限自己的任务 | 不可以 |
| approval.list、approval.renew、approval.decide | 只限自己的任务 | 只限自己的任务 | 不可以 |
| approval.get | 只限自己的任务 | 只限自己的任务 | 不可以 |

「全部取消并关闭」先逐个取消，再由创建者关闭。每一步独立产生事件。「隐藏已关闭」和「只看我建的」只筛选客户端已经收到的任务，后者依据 createdBy。

公开通知 `event` 的 kind 为 `task.changed`、`session.changed`、`turn.changed`、`item.changed`、`usage.updated`。流式文本及命令输出用 `item.delta` 并携带 itemId。每个增量都写进公开事件日志，占一个游标；重连时按顺序补发，才能拼回完整输出。只向所有者发送的即时通知为 `approval.requested` 与 `input.requested`，后者带 requestId、任务、会话和轮次引用，以及按原顺序排列的非空 questions。成功 attach 后重送仍有效的私有回调。

每道输入题保留上游的字符串 id、header、question 和可选的 options（每项含 label、description）。Codex 的 isOther、isSecret 分别映射为必填的 allowOther、secret，省略时取上游默认值 false。上游 options 为 null 或缺省时，QO 省略该字段。secret 为 true 的题目，其回答只交给发起回调的 harness，不写入公开事件、日志或诊断。

`input.answer` 用题目原有的 id 作键，去掉 QO 路由字段后的 `{"answers":{"mode":{"answers":["离线"]}}}` 即固定版 Codex 的 `ToolRequestUserInputResponse`。服务在核对所有者和未决请求后，按该请求的题目 ID 检查整个回答。任何一个键不存在即返回 `invalid_params`，整份回答不转交。例如，本次只问了 mode 和 note，回答里出现 missing 就属于此错误。

错误码、reason、附带字段与触发条件见 [协议 schema 的 RpcError](../../packages/protocol/schema/protocol.json#/definitions/RpcError)。`unauthorized` 不进入 RPC，认证失败在 HTTP 升级前返回 403。

## 4. 持久事件与断线补发

**候选**　短时内存队列适合实时推送，重启就失去历史。数据库顺序日志同时支持恢复与现在的订阅者。

**决定 / 理由**　公开事件采用连续递增的全局 64 位正整数游标，线上以十进制字符串表示，空日志的高水位为 `"0"`，第一条事件游标为 `"1"`。唯一写者在同一事务里更新投影、递增持久高水位并追加事件。回滚连同游标一起撤销。所有已认证客户端看到同一条公共序列，私有回调不占用公共游标。审批状态只公开“等审批”等会话状态，原始请求与决定保留在所有者通道。

`afterCursor=C` 表示客户端已应用 C，只接收大于 C 的事件。订阅者先登记实时队列并取已提交高水位 H，补发 (C,H]，再按序发送队列中大于 H 的事件。回放期间以一致的读取视图隔离裁剪。客户端在应用事件后才保存游标，收到已处理游标时去重。慢连接超出服务发送缓冲可断开，再用最后已应用的游标接续。同一连接只接受一次成功订阅。第二次调用 `events.subscribe` 返回 `conflict`，原订阅继续按原游标顺序发送。

默认保留 7 天或 256 MiB 的事件有效载荷（含事件元数据，按编码后的 UTF-8 字节计），任一先达到即从最老记录连续裁剪。前者覆盖周末离线，后者限制高频输出的磁盘占用。两项均可配置。上限只约束保留的事件，SQLite 索引、事务日志与当前态另计。裁剪不重置高水位。若 C 早于可续接下界，返回 `cursor_expired` 及保留区间，客户端用 `state.get` 的快照和配套游标重订阅。C 超过高水位返回无效参数。新客户端也可走快照路径，实时流不会静默跳过缺口。

## 5. 审批归属、内容绑定与超时

**候选**　由界面代批、以可自报的 clientId 辨认所有者，或由任务创建方持有独立随机密钥。前两种无法隔开网页和 Nemu 的决定权。

**决定 / 理由**　服务只为内置界面登记固定所有者。浏览器或桌面 WebView 完成换码后即可重新取得该界面的任务；两种入口对同一请求竞争时仍只提交一份决定。浏览器不保存任务密钥，也无需导出恢复文件。外部客户端每任务生成独立 32 字节 ownerKey，线上编码为 64 位小写十六进制，服务只保存校验值；原生服务令牌本身没有审批权。外部客户端只能对自己创建或凭正确密钥 attach 的任务作决定，重新 attach 时撤下旧连接的处理权。QO 不保存外部客户端的身份或记忆。内置界面取消外部会话时，依第 5.4 条撤销该会话全部未决审批和输入。之后提交审批决定或回答输入均返回 `request_withdrawn`，与摘要不符和审批超时分别处理。

1. 适配器保存上游 RPC 请求 ID 与进程实例的对应关系，连同原样的 method、params 和执行所需的命令或变更上下文，冻结为只读审批材料。请求缺少必要的命令、目标路径或待修改内容时，先关联同一 item 的数据。仍无法展示实际操作就中止该回调。人读的 summary 与核对用的 digest 都取自同一份材料。从收到的 ApprovalRequest 去掉 digest 和 state 即得到 ApprovalDigestInput。缺省的 turnId、itemId 整个省略，操作上下文位于 raw.context。对该对象做 RFC 8785 规范化，取 UTF-8 字节的 SHA-256，输出小写十六进制 digest。服务和客户端共用 `qo_protocol::approval_digest`。[ApprovalDigestInput](../../packages/protocol/schema/protocol.json#/definitions/ApprovalDigestInput) · [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
2. 通过所有者的私有通知或私有读取交付材料。MCP 征询和旧版请求可能缺少 turnId、itemId；按已知进程、线程和原 RPC id 关联任务，不能唯一定位的请求直接拒绝。公开状态、重放日志、诊断和测试样本不携带凭据、ownerKey 或审批原文。处理输入回调时按原请求中各题的 secret 标记识别敏感回答。敏感回答只在私有回调中交付，不进入公开事件、日志或诊断。原始审批材料仅在处理中的服务内存保存。持久索引保留 ID、状态、到期时间和摘要哈希。命令、用户输入、差异和输出在进入投影或事件之前过滤凭据。无法安全公开的内容仅给状态。认证回调由 CLI 自行处理，不作为命令或审批展示。
3. 服务在同一事务校验归属、pending、到期时间与 digest，原子记录决定。摘要不符只拒绝本次提交。征询的表单回答须通过该请求的模式校验，权限批准仅能返回此次列明的权限。对于仅能授予更广权限或需要额外信任流程的请求，本版返回拒绝或取消。如果上游回调或关联条目在等待时变更了实际操作，立即撤销旧请求，按新材料重新索取决定。相同 decisionId 和相同内容的重试返回现有结果，复用 ID 改内容或竞争提交第二份决定返回 conflict。只向原来的上游回调交付一次，接受仅限这次操作，拒绝权利扩展、永久规则和会话级授权。`approval.decide` 尝试交付后才返回，delivery 限 sent 或 unknown。若服务在记录决定后、交付前崩溃，`approval.get` 可能短暂读到 recorded，重启对账后改为 unknown 并结束受影响会话。客户端通过查询取得最终状态，无需另订交付通知。服务不会重新发送授权以探测执行结果。
4. 审批有效期默认 10 分钟，可配置，给用户留出阅读时间并限制陈旧授权。客户端断线时解绑，上游继续等待。到期使旧 ID 失效，上游仍等待。所有者回来后用 list 查看，renew 仅对仍活着的原回调重新核对材料，签发新 ID、digest 和期限。取消、进程退出或上游撤回时撤销所有相关请求。与决定竞争时按事务顺序处理。没有定时批准、缺省批准或其他客户端接管的路径。

## 6. 适配器与版本边界

**候选**　让前端直接处理各家消息，或让服务内的适配器把消息翻译成上面的模型。

**决定 / 理由**　选择适配器。服务持有归属、状态和审批决定。适配器持有进程、线程、轮次和在途 RPC 的映射。所需接口如下。

| 适配器操作 | Codex 映射 |
| --- | --- |
| 版本与能力 | 启动前核对 CLI 版本。stdio 上 initialize / initialized 协商，记录版本和可用能力 |
| 开线程、发轮次 | thread/start、turn/start。保留上游线程与轮次 ID |
| 追加、打断 | turn/steer 带 expectedTurnId。turn/interrupt 指定线程和轮次 |
| 回调与回复 | 按下表分类，使用上游原 RPC id 响应。每条 JSON 消息单独占一行，stdout 只作协议通道，stderr 和凭据载荷不进入公开事件 |
| 条目、终态和用量 | 消费 item 变化、turn/completed、thread/tokenUsage/updated。上游撤回或解析失败时通知服务 |

固定版 [harness 锁](../../config/harness.lock.json) 指向 Codex 0.162.1。启动命令为 `codex app-server`，使用默认 stdio JSONL。升级时重新核验版本与线协议，未经校验的版本不能启动会话。生成器直接调用工作区固定版本的 `codex app-server generate-json-schema --out <临时目录>`，从稳定的 ServerRequest 导出 Rust 载荷类型和方法枚举。已提交的 schema、绑定、枚举都由 CI 重新生成比对。Rust 的穷尽匹配让新增的回调必须得到明确分类。协议材料由适配器保存原始 JSON，生成类型用于解释字段，保留计算摘要所需的原文。[固定版发布记录](https://github.com/openai/codex/releases/tag/rust-v0.162.1) · [回调原始 schema](../../packages/adapter-codex/schema/ServerRequest.json)

| 固定版服务端请求 | 处理 |
| --- | --- |
| `item/commandExecution/requestApproval`、`item/fileChange/requestApproval` | 向所有者展示命令或变更，只允许针对本次操作的决定。 |
| `item/permissions/requestApproval` | 展示原权限集，只能授予请求内能够限于本次的权限，或拒绝。 |
| `mcpServer/elicitation/request` | 表单或 URL 征询交给任务所有者，按上游类型回复或取消。不能把打开 URL 或返回表单内容当作普通命令同意。 |
| `execCommandApproval`、`applyPatchApproval` | 旧版命令和补丁回调，同样经过所有者审批。 |
| `item/tool/requestUserInput` | 私有输入请求，按原始关联 ID 回答。 |
| `item/tool/call`、`account/chatgptAuthTokens/refresh`、`attestation/generate` | 第一版未启用这些能力，显式拒绝。认证材料不进入审批流或日志。 |

QO 自身的契约来自 [packages/protocol/schema](../../packages/protocol/schema)。同一组 JSON Schema 经 `pnpm generate` 生成 Rust 与 TS，生成物进库。2/4 定义全部对象、事件、校验器和样例，3/4 实现假服务端，4/5 补全契约，5/5 运行一致性测试。两平台用 `pnpm generate:check` 检查生成物，差异即失败。

每家适配器用脱敏的离线录制覆盖正常轮次、交错的审批回调、拒绝、打断、用量、撤销和异常退出。保留双向消息顺序、关联 ID 关系、预期 QO 输出。测试无需账户或模型。假服务端和外部一致性测试验证 QO 的线协议，真实厂商的转换由各自适配器卡验证。TypeScript 只生成静态类型。网页只连接本机服务，5/5 的一致性测试用 Rust 出参校验器逐条检查回复和通知，网页无需额外的运行时校验库。

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
| 运行锁、endpoint.json 与 desktop.json | `%LOCALAPPDATA%/QO/run` | `$XDG_RUNTIME_DIR/qo`，缺失时使用上述私有状态目录内的 `run` |
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

## 10. 为什么选 Rust 和 Tauri，不选 Node 和 Electron

**候选**　Node 服务加 Electron 可复用 TypeScript，并用同一版本 Chromium 渲染各平台。Rust 加 Tauri v2 需要同时维护 Cargo 与 pnpm，还要分别验证系统 WebView、Linux 图形库和平台托盘实现。

**决定 / 理由**　服务独立管理子进程、凭据和长连接，和桌面原生代码共享 Rust 库及类型；Nemu 单独使用服务时无需前端运行时。Tauri 使用系统 WebView，桌面壳与浏览器复用 React 网页，安装包无需再携带 Electron 的 Chromium 和 Node。这里没有实测包体或内存，选择依据是分发与进程边界。托盘使用官方核心功能，通知和开机自启使用官方插件；图形依赖与两端生成器均计入构建成本。[Tauri 架构](https://v2.tauri.app/concept/architecture/) · [官方托盘](https://v2.tauri.app/learn/system-tray/) · [官方通知](https://v2.tauri.app/plugin/notification/) · [官方自启](https://v2.tauri.app/plugin/autostart/)

## 11. 附录 · 第二家 harness 的调研

**候选**　复用用户安装的 Gemini CLI 或 Antigravity CLI 的 ACP，或维护私有桥接。调研日期为 2026-11-27，仅核对官方文档、协议和许可说明。

**决定 / 理由**　第二家优先评估官方 ACP v1 的 stdio 接口，第一版只接 Codex。Google 已提供原生 ACP，技术上无需另造桥接。能否使用具体账号、能否把每个需审批操作交回创建者以及发行条款，须在第二家适配器卡选定版本后验证。

| 问题 | 核实结果 |
| --- | --- |
| Gemini 怎么连 | `gemini --acp` 在 stdio 上传输 JSON-RPC 2.0。Google 的方法列表写作 initialize、authenticate、newSession、loadSession、prompt、cancel。ACP v1 线协议中对应的会话方法名为 session/new、session/load、session/prompt、session/cancel。加载历史要看协商能力。[Gemini CLI ACP](https://geminicli.com/docs/cli/acp-mode/) · [ACP v1 会话协议](https://agentclientprotocol.com/protocol/v1/session-setup) |
| Antigravity 有没有 | 有。程序名是 `agy`，启动命令是 `agy acp`。官方说明它通过 stdio 运行 ACP v1，支持会话、流式工具动作和权限请求。具体版本可用 `agy acp --help` 检查。[CLI ACP](https://antigravity.google/docs/cli/acp) · [编辑器集成](https://antigravity.google/docs/ide/extensions/acp/) |
| 谁决定 | ACP agent 通过 session/request_permission 给客户端一组 optionId，暂停并等选择或取消。将一次性允许或拒绝映射为上游实际给的选项。不接受永久允许。Antigravity 的既有规则会影响是否产生回调。接入时须隔离配置、核对 strict 模式和实际录制，不满足时不能声称所有请求均经所有者审批。[ACP 权限往返](https://agentclientprotocol.com/protocol/v1/tool-calls) · [Antigravity 权限设置](https://antigravity.google/docs/cli/reference) |
| 哪些账号迁移 | Google 公告将 2026-06-18 的变化限定于个人免费渠道和 Google AI Pro/Ultra。Gemini Code Assist Standard/Enterprise 以及公告列出的付费 API key 渠道仍可使用 Gemini CLI。[Google 迁移公告](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/) |
| 许可证和分发 | Gemini CLI 源码为 Apache-2.0。Antigravity 个人版受 Google 通用条款及 Antigravity 附加条款约束，企业渠道依其适用协议。官方提供 ACP 集成文档，但未查到授予 QO 重新分发 Antigravity CLI 二进制的开源许可证。个人条款也对第三方软件使用服务有限制。后续应核对 QO 集成适用的条款和账号渠道，本仓库不捆绑该 CLI。[Gemini CLI 许可证](https://github.com/google-gemini/gemini-cli/blob/main/LICENSE) · [Antigravity 附加条款](https://antigravity.google/terms) |
