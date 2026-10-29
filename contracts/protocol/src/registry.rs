//! Role: Declare the current protocol types, methods and message directions.
use crate::command_values::*;
use crate::commands::*;
use crate::lsp_values::*;
use crate::rendering::*;
use crate::wire_values::*;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MethodDirection {
    ClientToServer,
    ServerToClient,
    Bidirectional,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Request,
    Notification,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    Deserialize,
    Serialize,
}
#[derive(Clone, Copy, Debug)]
pub struct WireDescriptor {
    pub wire_name: &'static str,
    pub schema_name: &'static str,
    pub typescript_name: &'static str,
    pub contract: Contract,
    pub semantic: Option<&'static str>,
    pub accept: Option<fn(serde_json::Value) -> Result<(), serde_json::Error>>,
    #[cfg(feature = "contract-schema")]
    pub schema: fn(Contract) -> Result<serde_json::Value, String>,
}
#[cfg(feature = "contract-schema")]
trait SchemaType: schemars::JsonSchema {}
#[cfg(feature = "contract-schema")]
impl<T: schemars::JsonSchema> SchemaType for T {}
#[cfg(not(feature = "contract-schema"))]
trait SchemaType {}
#[cfg(not(feature = "contract-schema"))]
impl<T> SchemaType for T {}
fn accept<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<(), serde_json::Error> {
    serde_json::from_value::<T>(value).map(drop)
}
#[cfg(feature = "contract-schema")]
fn schema_for<T: SchemaType>(contract: Contract) -> Result<serde_json::Value, String> {
    let settings = schemars::generate::SchemaSettings::draft2020_12();
    let settings = match contract {
        Contract::Deserialize => settings.for_deserialize(),
        Contract::Serialize => settings.for_serialize(),
    };
    serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .map_err(|error| error.to_string())
}
const fn receive<T: serde::de::DeserializeOwned + SchemaType>(
    wire_name: &'static str,
    schema_name: &'static str,
    typescript_name: &'static str,
) -> WireDescriptor {
    WireDescriptor {
        wire_name,
        schema_name,
        typescript_name,
        contract: Contract::Deserialize,
        semantic: None,
        accept: Some(accept::<T>),
        #[cfg(feature = "contract-schema")]
        schema: schema_for::<T>,
    }
}
const fn send<T: SchemaType>(
    wire_name: &'static str,
    schema_name: &'static str,
    typescript_name: &'static str,
) -> WireDescriptor {
    WireDescriptor {
        wire_name,
        schema_name,
        typescript_name,
        contract: Contract::Serialize,
        semantic: None,
        accept: None,
        #[cfg(feature = "contract-schema")]
        schema: schema_for::<T>,
    }
}
impl WireDescriptor {
    pub const fn with_semantic(mut self, semantic: &'static str) -> Self {
        self.semantic = Some(semantic);
        self
    }
}
pub const WIRE_TYPES: &[WireDescriptor] = &[
    receive::<LspTextDocument>("LspTextDocument", "lspTextDocument", "LspTextDocument"),
    receive::<LspDocumentParams>(
        "LspDocumentParams",
        "lspDocumentParams",
        "LspDocumentParams",
    ),
    receive::<LspCodeActionParams>(
        "LspCodeActionParams",
        "lspCodeActionParams",
        "LspCodeActionParams",
    ),
    receive::<LspExecuteCommandParams>(
        "LspExecuteCommandParams",
        "lspExecuteCommandParams",
        "LspExecuteCommandParams",
    ),
    receive::<LspEditorCommand>("LspEditorCommand", "lspEditorCommand", "LspEditorCommand"),
    receive::<LspCodeAction>("LspCodeAction", "lspCodeAction", "LspCodeAction"),
    send::<LspSemanticTokens>(
        "LspSemanticTokens",
        "lspSemanticTokens",
        "LspSemanticTokens",
    ),
    send::<LspShowDocumentParams>(
        "LspShowDocumentParams",
        "lspShowDocumentParams",
        "LspShowDocumentParams",
    ),
    send::<LspLogMessageParams>(
        "LspLogMessageParams",
        "lspLogMessageParams",
        "LspLogMessageParams",
    ),
    receive::<()>("Unit", "unit", "null"),
    receive::<RpcId>("RpcId", "id", "RpcId"),
    receive::<TextPosition>("TextPosition", "position", "TextPosition"),
    receive::<TextRange>("TextRange", "range", "TextRange"),
    receive::<TextSelection>("TextSelection", "selection", "TextSelection"),
    send::<NodeId>("NodeId", "nodeId", "NodeId"),
    send::<PreviewSessionId>("PreviewSessionId", "previewSessionId", "PreviewSessionId"),
    send::<SourcePosition>("SourcePosition", "sourcePosition", "SourcePosition"),
    send::<SourceRange>("SourceRange", "sourceRange", "SourceRange"),
    send::<NavigationEntry>("NavigationEntry", "navigationEntry", "NavigationEntry"),
    send::<RenderStyle>("RenderStyle", "renderStyle", "RenderStyle"),
    send::<RenderAsset>("RenderAsset", "renderAsset", "RenderAsset"),
    send::<RenderBlock>("RenderBlock", "renderBlock", "RenderBlock"),
    send::<RenderFrame>("RenderFrame", "renderFrame", "RenderFrame").with_semantic("frame"),
    receive::<PreviewNavigationEvent>(
        "PreviewNavigationEvent",
        "previewNavigationEvent",
        "PreviewNavigationEvent",
    ),
    send::<SourceNavigationEvent>(
        "SourceNavigationEvent",
        "sourceNavigationEvent",
        "SourceNavigationEvent",
    ),
    send::<RenderNavigationEvent>(
        "RenderNavigationEvent",
        "renderNavigationEvent",
        "RenderNavigationEvent",
    ),
    receive::<WorkspaceGrant>("WorkspaceGrant", "workspaceGrant", "WorkspaceGrant"),
    receive::<ClientInfo>("ClientInfo", "clientInfo", "ClientInfo"),
    receive::<ClientCapabilities>(
        "ClientCapabilities",
        "clientCapabilities",
        "ClientCapabilities",
    ),
    send::<WorkspaceStatus>("WorkspaceStatus", "workspaceStatus", "WorkspaceStatus"),
    send::<ServerCapabilities>(
        "ServerCapabilities",
        "serverCapabilities",
        "ServerCapabilities",
    ),
    receive::<InitializeParams>("InitializeParams", "initializeParams", "InitializeParams"),
    send::<InitializeResult>("InitializeResult", "initializeResult", "InitializeResult"),
    receive::<AttachDocumentParams>(
        "AttachDocumentParams",
        "attachDocumentParams",
        "AttachDocumentParams",
    ),
    send::<AttachDocumentResult>(
        "AttachDocumentResult",
        "attachDocumentResult",
        "AttachDocumentResult",
    ),
    receive::<CreatePreviewParams>(
        "CreatePreviewParams",
        "createPreviewParams",
        "CreatePreviewParams",
    ),
    receive::<PreviewTarget>("PreviewTarget", "previewTarget", "PreviewTarget"),
    send::<CreatePreviewResult>(
        "CreatePreviewResult",
        "createPreviewResult",
        "CreatePreviewResult",
    )
    .with_semantic("frameEnvelope"),
    send::<PreviewChangedParams>(
        "PreviewChangedParams",
        "previewChangedParams",
        "PreviewChangedParams",
    )
    .with_semantic("frameEnvelope"),
    receive::<RerenderPreviewParams>(
        "RerenderPreviewParams",
        "rerenderPreviewParams",
        "RerenderPreviewParams",
    ),
    receive::<DisposePreviewParams>(
        "DisposePreviewParams",
        "disposePreviewParams",
        "DisposePreviewParams",
    ),
    receive::<SetSelectionParams>(
        "SetSelectionParams",
        "setSelectionParams",
        "SetSelectionParams",
    ),
    receive::<SetViewportParams>(
        "SetViewportParams",
        "setViewportParams",
        "SetViewportParams",
    ),
    receive::<PreviewEventParams>(
        "PreviewEventParams",
        "previewEventParams",
        "PreviewEventParams",
    )
    .with_semantic("previewEvent"),
    send::<ServerPreviewEventParams>(
        "ServerPreviewEventParams",
        "serverPreviewEventParams",
        "ServerPreviewEventParams",
    )
    .with_semantic("serverPreviewEvent"),
    receive::<WorkspaceCommandRequest>(
        "WorkspaceCommandRequest",
        "workspaceCommandRequest",
        "WorkspaceCommandRequest",
    ),
    receive::<CommandContext>("CommandContext", "commandContext", "CommandContext"),
    receive::<DocumentCommandContext>(
        "DocumentCommandContext",
        "documentCommandContext",
        "DocumentCommandContext",
    ),
    receive::<WorkspaceCommand>("WorkspaceCommand", "workspaceCommand", "WorkspaceCommand"),
    receive::<MigrationPlanResult>(
        "MigrationPlanResult",
        "migrationPlanResult",
        "MigrationPlanResult",
    ),
    receive::<MigrationStatus>("MigrationStatus", "migrationStatus", "MigrationStatus"),
    receive::<WorkspaceStateRequest>(
        "WorkspaceStateRequest",
        "workspaceStateRequest",
        "WorkspaceStateRequest",
    ),
    receive::<WorkspaceStateResult>(
        "WorkspaceStateResult",
        "workspaceStateResult",
        "WorkspaceStateResult",
    ),
    receive::<WorkspaceUiContext>(
        "WorkspaceUiContext",
        "workspaceUiContext",
        "WorkspaceUiContext",
    ),
    receive::<WorkspaceUiWorkspace>(
        "WorkspaceUiWorkspace",
        "workspaceUiWorkspace",
        "WorkspaceUiWorkspace",
    ),
    receive::<WorkspaceUiDocument>(
        "WorkspaceUiDocument",
        "workspaceUiDocument",
        "WorkspaceUiDocument",
    ),
    receive::<WorkspaceUiCommandRequest>(
        "WorkspaceUiCommandRequest",
        "workspaceUiCommandRequest",
        "WorkspaceUiCommandRequest",
    ),
    send::<WorkspaceUiError>("WorkspaceUiError", "workspaceUiError", "WorkspaceUiError"),
    send::<CommandMessage>("CommandMessage", "commandMessage", "CommandMessage"),
    send::<CommandResult>("CommandResult", "commandResult", "CommandResult"),
    send::<NoteCategoryOption>(
        "NoteCategoryOption",
        "noteCategoryOption",
        "NoteCategoryOption",
    ),
    send::<GetNoteOptionsResult>(
        "GetNoteOptionsResult",
        "getNoteOptionsResult",
        "GetNoteOptionsResult",
    ),
    receive::<ReconfigureWorkspaceParams>(
        "ReconfigureWorkspaceParams",
        "reconfigureWorkspaceParams",
        "ReconfigureWorkspaceParams",
    ),
];
pub fn wire_type(name: &str) -> Result<&'static WireDescriptor, String> {
    WIRE_TYPES
        .iter()
        .find(|descriptor| descriptor.wire_name == name)
        .ok_or_else(|| format!("unknown wire type: {name}"))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: &'static str,
    pub direction: MethodDirection,
    pub kind: MethodKind,
    pub client_to_server_params_type: Option<&'static str>,
    pub server_to_client_params_type: Option<&'static str>,
    pub result_type: Option<&'static str>,
}
pub mod method {
    //! Role: Name the current extension methods carried alongside standard LSP.
    pub const ATTACH_DOCUMENT: &str = "fleximark/attachDocument";
    pub const CREATE_PREVIEW: &str = "fleximark/createPreview";
    pub const PREVIEW_CHANGED: &str = "fleximark/previewChanged";
    pub const RERENDER_PREVIEW: &str = "fleximark/rerenderPreview";
    pub const DISPOSE_PREVIEW: &str = "fleximark/disposePreview";
    pub const SET_SELECTION: &str = "fleximark/setSelection";
    pub const SET_VIEWPORT: &str = "fleximark/setViewport";
    pub const PREVIEW_EVENT: &str = "fleximark/previewEvent";
    pub const EXECUTE_COMMAND: &str = "fleximark/executeCommand";
    pub const RECONFIGURE_WORKSPACE: &str = "fleximark/reconfigureWorkspace";
}
pub const METHOD_SPECS: &[MethodSpec] = &[
    MethodSpec {
        name: method::ATTACH_DOCUMENT,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("AttachDocumentParams"),
        server_to_client_params_type: None,
        result_type: Some("AttachDocumentResult"),
    },
    MethodSpec {
        name: method::CREATE_PREVIEW,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("CreatePreviewParams"),
        server_to_client_params_type: None,
        result_type: Some("CreatePreviewResult"),
    },
    MethodSpec {
        name: method::PREVIEW_CHANGED,
        direction: MethodDirection::ServerToClient,
        kind: MethodKind::Notification,
        client_to_server_params_type: None,
        server_to_client_params_type: Some("PreviewChangedParams"),
        result_type: None,
    },
    MethodSpec {
        name: method::RERENDER_PREVIEW,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("RerenderPreviewParams"),
        server_to_client_params_type: None,
        result_type: Some("Unit"),
    },
    MethodSpec {
        name: method::DISPOSE_PREVIEW,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("DisposePreviewParams"),
        server_to_client_params_type: None,
        result_type: Some("Unit"),
    },
    MethodSpec {
        name: method::SET_SELECTION,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Notification,
        client_to_server_params_type: Some("SetSelectionParams"),
        server_to_client_params_type: None,
        result_type: None,
    },
    MethodSpec {
        name: method::SET_VIEWPORT,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Notification,
        client_to_server_params_type: Some("SetViewportParams"),
        server_to_client_params_type: None,
        result_type: None,
    },
    MethodSpec {
        name: method::PREVIEW_EVENT,
        direction: MethodDirection::Bidirectional,
        kind: MethodKind::Notification,
        client_to_server_params_type: Some("PreviewEventParams"),
        server_to_client_params_type: Some("ServerPreviewEventParams"),
        result_type: None,
    },
    MethodSpec {
        name: method::EXECUTE_COMMAND,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("WorkspaceCommandRequest"),
        server_to_client_params_type: None,
        result_type: Some("CommandResult"),
    },
    MethodSpec {
        name: method::RECONFIGURE_WORKSPACE,
        direction: MethodDirection::ClientToServer,
        kind: MethodKind::Request,
        client_to_server_params_type: Some("ReconfigureWorkspaceParams"),
        server_to_client_params_type: None,
        result_type: Some("Unit"),
    },
];
pub fn method_spec(name: &str) -> Option<&'static MethodSpec> {
    METHOD_SPECS.iter().find(|spec| spec.name == name)
}

#[derive(Clone, Copy, Debug)]
pub struct LspMethodSpec {
    pub name: &'static str,
    pub params: Option<&'static str>,
}
pub const LSP_METHODS: &[LspMethodSpec] = &[
    LspMethodSpec {
        name: "initialize",
        params: None,
    },
    LspMethodSpec {
        name: "initialized",
        params: None,
    },
    LspMethodSpec {
        name: "shutdown",
        params: None,
    },
    LspMethodSpec {
        name: "exit",
        params: None,
    },
    LspMethodSpec {
        name: "$/cancelRequest",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/didOpen",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/didChange",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/didClose",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/completion",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/semanticTokens/full",
        params: Some("LspDocumentParams"),
    },
    LspMethodSpec {
        name: "textDocument/hover",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/documentSymbol",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/diagnostic",
        params: None,
    },
    LspMethodSpec {
        name: "textDocument/codeAction",
        params: Some("LspCodeActionParams"),
    },
    LspMethodSpec {
        name: "workspace/didChangeWorkspaceFolders",
        params: None,
    },
    LspMethodSpec {
        name: "workspace/executeCommand",
        params: Some("LspExecuteCommandParams"),
    },
    LspMethodSpec {
        name: "textDocument/publishDiagnostics",
        params: None,
    },
    LspMethodSpec {
        name: "window/showDocument",
        params: Some("LspShowDocumentParams"),
    },
    LspMethodSpec {
        name: "window/logMessage",
        params: Some("LspLogMessageParams"),
    },
];
