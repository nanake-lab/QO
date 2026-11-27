//! Public QO wire types and handshake validation.
include!("../generated/types.rs");

use std::sync::LazyLock;

static INITIALIZE: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/InitializeParams.json"))
            .expect("embedded initialize schema is JSON");
    jsonschema::validator_for(&schema).expect("valid initialize schema")
});

pub fn initialize_params(value: serde_json::Value) -> Option<InitializeParams> {
    if INITIALIZE.is_valid(&value) {
        serde_json::from_value(value).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn initialize_matches_the_schema() {
        assert!(initialize_params(json!({"versions":["qo.v1"]})).is_some());
        for invalid in [
            json!({}),
            json!({"versions":[]}),
            json!({"versions":[1]}),
            json!({"versions":["qo.v1"],"token":"unused"}),
        ] {
            assert!(initialize_params(invalid).is_none());
        }
    }
}
