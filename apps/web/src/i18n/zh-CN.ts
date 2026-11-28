import type {
  CancelledBy,
  Creator,
  SessionState,
  TaskState,
} from "@qo/protocol";

export const zhCN = {
  app: {
    name: "QO",
    documentTitle: (pendingCount: number) =>
      pendingCount > 0 ? `(${pendingCount}) QO` : "QO",
  },
  connection: {
    connecting: "正在连接本机服务…",
    connected: (version: string) => `已连接 · ${version}`,
    reconnecting: (attempt: number, delaySeconds: number) =>
      `连接已断开，${delaySeconds} 秒后进行第 ${attempt} 次重连…`,
    reconnectingShort: "重连中",
    authExpiredBanner:
      "登录状态已失效。请从托盘选择「在浏览器中打开」，或运行 qo open 重新打开。",
    loginLinkInvalidBanner:
      "登录链接已失效。请从托盘选择「在浏览器中打开」，或运行 qo open 重新打开。",
    authExpiredShort: "需重新登录",
    unsupportedVersionBanner: "本机服务协议版本不匹配，无法继续连接。",
    errorShort: "连接异常",
    cursorLabel: (cursor: string) => `游标 ${cursor}`,
  },
  sidebar: {
    navLabel: "任务导航",
    attentionHeading: "需要你处理",
    attentionEmpty: "当前没有待处理项",
    filtersLabel: "任务筛选",
    hideClosed: "隐藏已关闭",
    onlyMine: "只看我建的",
    tasksHeading: "任务列表",
    ungrouped: "未分组",
    emptyTasks: "当前没有任务。",
    emptyFilteredTasks: "没有符合当前筛选条件的任务。",
    noSessions: "暂无会话",
    externalWaiting: "等外部客户端处理",
  },
  creator: {
    ui: "我",
    external: "外部",
  } satisfies Record<Creator, string>,
  taskState: {
    open: "打开",
    closed: "已关闭",
  } satisfies Record<TaskState, string>,
  sessionState: {
    starting: "启动中",
    idle: "空闲",
    running: "在跑",
    waiting_input: "等输入",
    waiting_approval: "等批准",
    completed: "已完成",
    failed: "失败",
    cancelled: "已取消",
  } satisfies Record<SessionState, string>,
  cancelledBy: {
    ui: "由界面取消",
    owner: "由创建者取消",
  } satisfies Record<CancelledBy, string>,
  overview: {
    noSelectionTitle: "未选择任务",
    noSelectionBody: "从左侧选择一个任务查看它的会话状态和当前进展。",
    connectingTitle: "正在连接本机服务",
    connectingBody: "正在与本机服务完成握手并同步任务列表…",
    authRequiredTitle: "需要重新登录",
    authRequiredBody:
      "当前页面没有有效登录会话。请从托盘选择「在浏览器中打开」，或运行 qo open 获取新的登录链接。",
    errorTitle: "无法连接本机服务",
    errorBody: "本机服务协议版本不匹配，无法同步状态。",
    noTasksTitle: "还没有任务",
    noTasksBody:
      "本机服务已就绪。当前列表为空，由界面或外部客户端创建的任务会实时出现在左侧。",
    cwdLabel: "工作目录",
    forbiddenRootsLabel: "禁用目录",
    sessionsHeading: "会话",
    sessionLabel: (index: number) => `会话 ${index}`,
    noSessionsBody: "这个任务还没有开启会话。",
    harnessLabel: "驱动",
    modelLabel: "生效模型",
    reasoningEffortLabel: "推理力度",
    serviceTierLabel: "服务档位",
    unsetValue: "默认",
    externalNotice:
      "这是外部客户端创建的任务。界面只展示公开状态和条目，不能替创建者批准或回答。",
    waitingExternalApproval: "等外部客户端处理",
    waitingYourApproval: "正在等待你处理批准请求",
    waitingYourInput: "正在等待你回答提问",
    turnsHeading: "当前轮次",
    noTurnsBody: "这个会话还没有开始对话轮次。",
    userInputLabel: "输入",
    assistantLabel: "助手",
    commandLabel: "命令",
    fileChangeLabel: "文件修改",
    exitCodeLabel: (code: number) => `退出码 ${code}`,
    sessionErrorLabel: "错误",
  },
} as const;
