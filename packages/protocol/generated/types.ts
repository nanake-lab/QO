// Generated from JSON Schema. Run pnpm generate to update.

export interface InitializeParams {
    versions: [string, ...string[]];
}

export interface InitializeResult {
    version: ProtocolVersion;
}

export type ProtocolVersion = "qo.v1";

export interface LoginLinkResult {
    url: string;
}
