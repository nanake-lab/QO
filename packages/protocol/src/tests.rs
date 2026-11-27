use super::*;
use std::{fs, path::Path};

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap())
        .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

#[test]
fn every_wire_schema_has_passing_and_failing_samples() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    let catalog = read(&root.join("schema/methods.json"));
    for path in fs::read_dir(root.join("samples")).unwrap() {
        let name = path.unwrap().file_name();
        assert!(
            contract["definitions"]
                .get(name.to_str().unwrap())
                .is_some(),
            "orphan samples {name:?}"
        );
    }
    for name in contract["definitions"].as_object().unwrap().keys() {
        for (dir, expected) in [("valid", true), ("invalid", false)] {
            let folder = root.join("samples").join(name).join(dir);
            let samples: Vec<_> = fs::read_dir(&folder)
                .unwrap_or_else(|err| panic!("{}: {err}", folder.display()))
                .map(|p| p.unwrap().path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
                .collect();
            assert!(!samples.is_empty(), "no samples at {}", folder.display());
            for path in samples {
                let value = read(&path);
                assert_eq!(named_valid(name, &value), expected, "{}", path.display());
                if name == "RpcError" {
                    assert_eq!(validate_error(&value), expected);
                }
                for (wire, pair) in catalog["methods"].as_object().unwrap() {
                    if pair["params"].as_str() == Some(name) {
                        assert_eq!(
                            validate_params(wire, &value),
                            expected,
                            "{wire}: {}",
                            path.display()
                        );
                    }
                    if pair["result"].as_str() == Some(name) {
                        assert_eq!(
                            validate_output(wire, &value),
                            expected,
                            "{wire}: {}",
                            path.display()
                        );
                    }
                }
                for (wire, ty) in catalog["notifications"].as_object().unwrap() {
                    if ty.as_str() == Some(name) {
                        assert_eq!(
                            validate_output(wire, &value),
                            expected,
                            "{wire}: {}",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unknown_names_fail_closed_and_types_match_the_catalog() {
    assert_eq!(InitializeParams::METHOD.name(), "initialize");
    let reply: <InitializeParams as RpcParams>::Result = InitializeResult {
        version: ProtocolVersion::QoV1,
    };
    assert!(validate_output(
        "initialize",
        &serde_json::to_value(reply).unwrap()
    ));
    assert!(!validate_params("unknown", &serde_json::json!({})));
    assert!(!validate_output("unknown", &serde_json::json!({})));
    assert!(
        initialize_params(serde_json::json!({"versions": ["qo.v1"], "token": "extra"})).is_none()
    );
}
