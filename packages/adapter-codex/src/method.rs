// Generated from the pinned Codex ServerRequest schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum Method {
    #[serde(rename = "item/commandExecution/requestApproval")]
    CommandExecutionRequestApproval,
    #[serde(rename = "item/fileChange/requestApproval")]
    FileChangeRequestApproval,
    #[serde(rename = "item/tool/requestUserInput")]
    ToolRequestUserInput,
    #[serde(rename = "mcpServer/elicitation/request")]
    McpServerElicitationRequest,
    #[serde(rename = "item/permissions/requestApproval")]
    PermissionsRequestApproval,
    #[serde(rename = "item/tool/call")]
    DynamicToolCall,
    #[serde(rename = "account/chatgptAuthTokens/refresh")]
    ChatgptAuthTokensRefresh,
    #[serde(rename = "attestation/generate")]
    AttestationGenerate,
    #[serde(rename = "applyPatchApproval")]
    ApplyPatchApproval,
    #[serde(rename = "execCommandApproval")]
    ExecCommandApproval,
}
