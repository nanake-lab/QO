//! QO wire types and draft-07 validation shared by services and test clients.
use serde_json::Value;
use std::sync::LazyLock;

include!("../generated/types.rs");

/// Associates a request's Rust parameter type with its method and result type.
pub trait RpcParams {
    type Result;
    const METHOD: Method;
}

macro_rules! rpc_methods {
    ($($variant:ident => ($name:literal, $params:ident, $result:ident)),+ $(,)?) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub enum Method { $($variant),+ }

        impl Method {
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)+ _ => None }
            }
            pub fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
            pub fn schemas(self) -> (&'static str, &'static str) {
                match self { $(Self::$variant => (stringify!($params), stringify!($result))),+ }
            }
        }
        $(impl RpcParams for $params {
            type Result = $result;
            const METHOD: Method = Method::$variant;
        })+
    };
}
include!("../generated/methods.rs");

const CONTRACT: &str = include_str!("../schema/protocol.json");
static VALIDATORS: LazyLock<jsonschema::ValidatorMap> = LazyLock::new(|| {
    let document: Value = serde_json::from_str(CONTRACT).expect("embedded protocol JSON");
    jsonschema::draft7::options()
        .should_validate_formats(true)
        .build_map(&document)
        .expect("valid embedded protocol schemas")
});

fn named_valid(name: &str, value: &Value) -> bool {
    VALIDATORS
        .get(&format!("#/definitions/{name}"))
        .is_some_and(|validator| validator.is_valid(value))
}

pub fn validate_params(method: &str, value: &Value) -> bool {
    Method::from_name(method).is_some_and(|m| named_valid(m.schemas().0, value))
}

/// Validate a successful RPC result or a notification's params.
pub fn validate_output(name: &str, value: &Value) -> bool {
    Method::from_name(name)
        .map(|m| m.schemas().1)
        .or_else(|| notification_schema(name))
        .is_some_and(|schema| named_valid(schema, value))
}

pub fn validate_error(value: &Value) -> bool {
    named_valid("RpcError", value)
}

pub fn initialize_params(value: Value) -> Option<InitializeParams> {
    validate_params("initialize", &value)
        .then(|| serde_json::from_value(value).ok())
        .flatten()
}

#[cfg(test)]
mod tests;
