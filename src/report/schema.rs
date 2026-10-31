//! JSON Schemas for the report, scenario files and targets files. They're generated
//! from the Rust types into `schemas/v1/` and published with the site, so editors can
//! validate the files. These tests keep the committed schemas in sync with the types
//! and check them against real files.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{REPORT_SCHEMA, Report, SCHEMAS_URL};

/// (file name, schema) for each published schema, with its `$id` and title.
fn schemas() -> Vec<(&'static str, Value)> {
    [
        (
            "report.schema.json",
            "poolproof report (run --format json)",
            schemars::schema_for!(Report),
        ),
        (
            "scenario.schema.json",
            "poolproof scenario file",
            schemars::schema_for!(crate::scenario::Scenario),
        ),
        (
            "targets.schema.json",
            "poolproof targets file",
            crate::targets::file_schema(),
        ),
    ]
    .into_iter()
    .map(|(file, title, schema)| {
        let mut value = schema.to_value();
        let object = value.as_object_mut().unwrap();
        object.insert("$id".into(), format!("{SCHEMAS_URL}/{file}").into());
        object.insert("title".into(), title.into());
        (file, value)
    })
    .collect()
}

fn schema(file: &str) -> Value {
    schemas().into_iter().find(|(f, _)| *f == file).unwrap().1
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/v1")
}

#[test]
fn committed_schemas_are_up_to_date() {
    let update = std::env::var_os("UPDATE_SCHEMAS").is_some();
    let mut stale = Vec::new();
    for (file, schema) in schemas() {
        let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
        let path = dir().join(file);
        if update {
            std::fs::create_dir_all(dir()).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(file);
        }
    }
    assert!(
        stale.is_empty(),
        "schemas/v1 is out of date ({stale:?}); run `UPDATE_SCHEMAS=1 cargo test schema` and commit it"
    );
    assert_eq!(REPORT_SCHEMA, format!("{SCHEMAS_URL}/report.schema.json"));
}

/// Checks `value` against `schema` for the keywords schemars writes, and panics on
/// any other keyword so nothing is skipped without notice. The first problem comes
/// back with its path.
fn validate(root: &Value, schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    const IGNORED: &[&str] = &["$schema", "$id", "$defs", "title", "description", "default", "format"];
    let schema = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(format!("{path}: not allowed")),
        Value::Object(s) => s,
        _ => panic!("schema at {path} is not an object"),
    };
    for key in schema.keys() {
        let known = [
            "$ref",
            "type",
            "enum",
            "const",
            "minimum",
            "allOf",
            "anyOf",
            "oneOf",
            "properties",
            "required",
            "additionalProperties",
            "items",
            "prefixItems",
            "minItems",
            "maxItems",
        ];
        assert!(
            known.contains(&key.as_str()) || IGNORED.contains(&key.as_str()),
            "validate() doesn't handle the `{key}` keyword (at {path})"
        );
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let name = r.strip_prefix("#/$defs/").expect("only local $refs");
        validate(root, &root["$defs"][name], value, path)?;
    }
    if let Some(types) = schema.get("type") {
        let is = |t: &Value| match t.as_str().unwrap() {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            other => panic!("unknown type {other}"),
        };
        let ok = match types {
            Value::Array(types) => types.iter().any(is),
            t => is(t),
        };
        if !ok {
            return Err(format!("{path}: expected {types}, got {value}"));
        }
    }
    if let Some(Value::Array(options)) = schema.get("enum")
        && !options.contains(value)
    {
        return Err(format!(
            "{path}: {value} is not one of {}",
            Value::from(options.clone())
        ));
    }
    if let Some(c) = schema.get("const")
        && c != value
    {
        return Err(format!("{path}: expected {c}, got {value}"));
    }
    if let (Some(min), Some(v)) = (schema.get("minimum").and_then(Value::as_f64), value.as_f64())
        && v < min
    {
        return Err(format!("{path}: {v} is below {min}"));
    }
    for s in schema.get("allOf").and_then(Value::as_array).into_iter().flatten() {
        validate(root, s, value, path)?;
    }
    if let Some(any) = schema.get("anyOf").and_then(Value::as_array)
        && !any.iter().any(|s| validate(root, s, value, path).is_ok())
    {
        return Err(format!("{path}: {value} matches nothing in anyOf"));
    }
    if let Some(one) = schema.get("oneOf").and_then(Value::as_array) {
        let results: Vec<_> = one.iter().map(|s| validate(root, s, value, path)).collect();
        match results.iter().filter(|r| r.is_ok()).count() {
            1 => {}
            0 => {
                let why: Vec<String> = results.into_iter().filter_map(Result::err).collect();
                return Err(format!("{path}: matches nothing in oneOf: {}", why.join("; ")));
            }
            n => return Err(format!("{path}: matches {n} options in oneOf")),
        }
    }
    if let Value::Object(object) = value {
        for r in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
            let r = r.as_str().unwrap();
            if !object.contains_key(r) {
                return Err(format!("{path}: missing `{r}`"));
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, v) in object {
            let at = format!("{path}.{key}");
            match properties.and_then(|p| p.get(key)) {
                Some(s) => validate(root, s, v, &at)?,
                None => {
                    if let Some(extra) = schema.get("additionalProperties") {
                        validate(root, extra, v, &at).map_err(|e| format!("{e} (unknown field `{key}`)"))?;
                    }
                }
            }
        }
    }
    if let Value::Array(items) = value {
        let count = |k: &str| schema.get(k).and_then(Value::as_u64);
        if count("minItems").is_some_and(|n| (items.len() as u64) < n)
            || count("maxItems").is_some_and(|n| (items.len() as u64) > n)
        {
            return Err(format!("{path}: wrong number of items ({})", items.len()));
        }
        let prefix = schema
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for (i, v) in items.iter().enumerate() {
            let at = format!("{path}[{i}]");
            if let Some(s) = prefix.get(i).or(schema.get("items")) {
                validate(root, s, v, &at)?;
            }
        }
    }
    Ok(())
}

fn check(file: &str, value: &Value) -> Result<(), String> {
    let schema = schema(file);
    validate(&schema, &schema, value, "$")
}

fn toml_value(text: &str) -> Value {
    serde_json::to_value(toml::from_str::<toml::Value>(text).unwrap()).unwrap()
}

fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    files.sort();
    files
}

#[test]
fn committed_results_match_the_report_schema() {
    let results = files("results", "json");
    assert!(!results.is_empty());
    for path in results {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        check("report.schema.json", &value).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

#[test]
fn a_fresh_report_with_a_comparison_matches_the_report_schema() {
    let outcome = |o: &str| format!(r#"{{"postgres":{o},"pooler":{o}}}"#);
    let steps = [
        r#"{"status":"ok","rows":[["1",null]],"tags":[1]}"#,
        r#"{"status":"error","code":"57P01","message":"terminated","connect":true}"#,
        r#"{"status":"abandoned"}"#,
        r#"{"status":"messages","messages":["ReadyForQuery I"]}"#,
    ]
    .map(outcome)
    .join(",");
    let rows = |verdict: &str| {
        format!(
            r#"{{"poolproof":"0","targets":[{{"name":"p","product":null,"version":null,"config":null,
            "mode":"transaction","postgres":"17.11","reset":"auto"}}],"scenarios":[{{"name":"s","description":"d",
            "refs":["https://example.com"],"steps":["x"],"repeat":5}},{{"name":"t","description":"d","refs":[],"steps":[]}}],
            "results":[{{"target":"p","scenario":"s",{verdict},"steps":[{steps}],"runs":5,"failures":2}},
            {{"target":"p","scenario":"t","result":"skip","reason":"needs superuser","steps":[]}}]}}"#
        )
    };
    let baseline = Report::parse(&rows(r#""result":"error","reason":"target unreachable""#)).unwrap();
    let mut report = Report::parse(&rows(
        r#""result":"fail","mismatch":{"step":1,"action":"x","kind":"silent",
        "expected":{"status":"ok","rows":[],"tags":[]},"actual":{"status":"abandoned"}}"#,
    ))
    .unwrap();
    report.schema = Some(REPORT_SCHEMA.into());
    report.generated_at = Some(super::now_rfc3339());
    report.comparison = Some(super::compare::compare(&baseline, &report, crate::runner::suite::FailOn::Any).unwrap());
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(
        value["comparison"]["regressed"][0]["after"]["failed_runs"],
        serde_json::json!([2, 5])
    );
    check("report.schema.json", &value).unwrap();
}

#[test]
fn bundled_scenarios_and_targets_files_match() {
    let scenarios = files("scenarios", "toml");
    assert!(scenarios.len() > 30);
    for path in scenarios {
        let value = toml_value(&std::fs::read_to_string(&path).unwrap());
        check("scenario.schema.json", &value).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    for path in ["targets.toml", "selftest/targets.toml"] {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
        check("targets.schema.json", &toml_value(&text)).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}

#[test]
fn files_poolproof_rejects_fail_the_schema_too() {
    let scenario = |extra: &str, step_extra: &str| {
        format!(
            "name = \"s\"\ndescription = \"d\"\n{extra}\n[[step]]\nclient = \"a\"\nsimple = \"SELECT 1\"\n{step_extra}\n"
        )
    };
    assert!(check("scenario.schema.json", &toml_value(&scenario("", ""))).is_ok());
    for (extra, step_extra, why) in [
        ("requires2 = []", "", "unknown field `requires2`"),
        ("", "simpel = \"SELECT 1\"", "unknown field `simpel`"),
        ("modes = [\"statement-ish\"]", "", "is not one of"),
        ("requires = [\"root\"]", "", "is not one of"),
        ("", "compare = \"some\"", "matches nothing in oneOf"),
        ("repeat = -1", "", "below"),
    ] {
        let text = scenario(extra, step_extra);
        let err = check("scenario.schema.json", &toml_value(&text)).unwrap_err();
        assert!(err.contains(why), "{extra}{step_extra}: {err}");
        assert!(
            toml::from_str::<crate::scenario::Scenario>(&text).is_err(),
            "serde accepts {text}"
        );
    }
    let target = |extra: &str| {
        format!(
            "[[target]]\nname = \"p\"\npooler = \"postgres://p/db\"\ndirect = \"postgres://d/db\"\nmode = \"transaction\"\n{extra}\n"
        )
    };
    for ok in [
        "",
        "reset = \"auto\"",
        "reset = [\"SELECT 1\"]",
        "product = \"pgbouncer\"",
    ] {
        check("targets.schema.json", &toml_value(&target(ok))).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
    for (extra, why) in [
        ("reset = \"sometimes\"", "anyOf"),
        ("mode = \"statement-ish\"", ""),
        ("pooler_url = \"x\"", "unknown field `pooler_url`"),
    ] {
        let text = target(extra).replace("mode = \"transaction\"\nmode", "mode");
        let err = check("targets.schema.json", &toml_value(&text)).unwrap_err();
        assert!(err.contains(why), "{extra}: {err}");
    }
    let mut no_mode = toml_value(&target(""));
    no_mode["target"][0].as_object_mut().unwrap().remove("mode");
    assert!(
        check("targets.schema.json", &no_mode)
            .unwrap_err()
            .contains("missing `mode`")
    );
}
