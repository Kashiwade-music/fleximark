//! Role: Define JSON-RPC and application failure codes at the wire boundary.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const SERVER_NOT_INITIALIZED: i64 = -32002;
pub const UNSUPPORTED_PROTOCOL: i64 = -32001;
pub const COMMAND_FAILED: i64 = -32020;
pub const WRITE_DENIED: i64 = -32021;
pub const RECONFIGURATION_FAILED: i64 = -32022;
pub const REQUEST_CANCELLED: i64 = -32800;
pub const CONTENT_MODIFIED: i64 = -32801;
