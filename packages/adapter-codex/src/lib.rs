//! Codex JSONL callback routing. Replying is the responsibility of the task owner.
mod adapter;
mod tree;
mod wire;
pub use adapter::CodexFactory;
pub mod generated;
mod method;
pub use method::Method;

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Approval,
    Input,
    Unsupported,
}

impl Method {
    pub fn route(self) -> Route {
        match self {
            Self::CommandExecutionRequestApproval
            | Self::FileChangeRequestApproval
            | Self::PermissionsRequestApproval
            | Self::McpServerElicitationRequest
            | Self::ApplyPatchApproval
            | Self::ExecCommandApproval => Route::Approval,
            Self::ToolRequestUserInput => Route::Input,
            Self::DynamicToolCall | Self::ChatgptAuthTokensRefresh | Self::AttestationGenerate => {
                Route::Unsupported
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_exported_approval_routes_to_its_owner() {
        for name in [
            "item/commandExecution/requestApproval",
            "item/fileChange/requestApproval",
            "item/permissions/requestApproval",
            "mcpServer/elicitation/request",
            "applyPatchApproval",
            "execCommandApproval",
        ] {
            let method: Method = serde_json::from_value(name.into()).unwrap();
            assert_eq!(method.route(), Route::Approval, "{name}");
        }
        assert!(serde_json::from_value::<Method>("future/approval".into()).is_err());
    }
}
