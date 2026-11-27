// Generated from the QO protocol schema. Run pnpm generate to update.
rpc_methods! {
  Initialize => ("initialize", InitializeParams, InitializeResult),
  UiLoginLink => ("ui.loginLink", LoginLinkParams, LoginLinkResult),
  TaskCreate => ("task.create", TaskCreateParams, TaskCreateResult),
  TaskAttach => ("task.attach", TaskAttachParams, TaskAttachResult),
  TaskClose => ("task.close", TaskCloseParams, TaskCloseResult),
  StateGet => ("state.get", StateGetParams, StateGetResult),
  SessionOpen => ("session.open", SessionOpenParams, SessionOpenResult),
  SessionFinish => ("session.finish", SessionFinishParams, SessionFinishResult),
  SessionCancel => ("session.cancel", SessionCancelParams, SessionCancelResult),
  TurnStart => ("turn.start", TurnStartParams, TurnStartResult),
  TurnAppend => ("turn.append", TurnAppendParams, TurnAppendResult),
  TurnInterrupt => ("turn.interrupt", TurnInterruptParams, TurnInterruptResult),
  InputAnswer => ("input.answer", InputAnswerParams, InputAnswerResult),
  ApprovalList => ("approval.list", ApprovalListParams, ApprovalListResult),
  ApprovalRenew => ("approval.renew", ApprovalRenewParams, ApprovalRenewResult),
  ApprovalDecide => ("approval.decide", ApprovalDecideParams, ApprovalDecideResult),
  UsageGet => ("usage.get", UsageGetParams, UsageGetResult),
  EventsSubscribe => ("events.subscribe", EventsSubscribeParams, EventsSubscribeResult),
}

pub fn notification_schema(name: &str) -> Option<&'static str> {
    match name {
        "event" => Some(stringify!(Event)),
        "approval.requested" => Some(stringify!(ApprovalRequest)),
        "input.requested" => Some(stringify!(InputRequested)),
        _ => None,
    }
}
