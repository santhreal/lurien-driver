//! One spec, every surface: JSON Schema for MCP, a clap command for the CLI,
//! and a markdown row for the docs. Generated, never hand-maintained, so a
//! thousand verbs stay coherent across faces.

use super::{ArgSpec, ArgType, Args, Domain, OutputKind, VerbSpec};
use crate::error::Error;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Map, Value};

/// MCP `inputSchema` for one verb.
#[must_use]
pub fn json_schema(spec: &VerbSpec) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for arg in spec.args {
        let mut entry = json!({ "type": arg.ty.json_type(), "description": arg.help });
        if arg.ty == ArgType::StrList {
            entry["items"] = json!({ "type": "string" });
        }
        if let Some(default) = arg.default {
            entry["default"] = default_value(arg.ty, default);
        }
        props.insert(arg.name.to_string(), entry);
        if arg.required {
            required.push(Value::String(arg.name.to_string()));
        }
    }
    let mut schema = json!({
        "type": "object",
        "properties": Value::Object(props),
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

/// Description a face shows. Preview verbs say so.
#[must_use]
pub fn description(spec: &VerbSpec) -> String {
    match spec.stability {
        super::Stability::Stable => spec.summary.to_string(),
        super::Stability::Preview => format!("{} (preview: shape may change)", spec.summary),
    }
}

/// CLI subcommand for one verb. Required arguments are positionals in
/// declaration order; optional ones are long flags.
#[must_use]
pub fn clap_command(spec: &'static VerbSpec) -> Command {
    let mut cmd = Command::new(spec.name).about(description(spec));
    for alias in spec.aliases {
        cmd = cmd.alias(*alias);
    }
    for arg in spec.args {
        cmd = cmd.arg(clap_arg(arg));
    }
    cmd
}

fn clap_arg(arg: &'static ArgSpec) -> Arg {
    let mut a = Arg::new(arg.name).help(arg.help);
    if arg.required {
        // No `long`/`short` makes this a positional; clap indexes them in
        // declaration order.
        a = a.required(true);
    } else {
        a = a.long(arg.name);
    }
    a = match arg.ty {
        ArgType::Bool => a.action(ArgAction::SetTrue),
        ArgType::Int => a.value_parser(clap::value_parser!(i64)),
        ArgType::Float => a.value_parser(clap::value_parser!(f64)),
        ArgType::StrList => a.num_args(1..).action(ArgAction::Append),
        ArgType::Str | ArgType::Path => a,
    };
    if let Some(default) = arg.default {
        a = a.default_value(default);
    }
    a
}

/// Decode clap matches into [`Args`] using the spec, not hand-written parsing.
pub fn args_from_matches(spec: &VerbSpec, matches: &ArgMatches) -> Result<Args, Error> {
    let mut args = Args::new();
    for arg in spec.args {
        match arg.ty {
            ArgType::Bool => {
                if matches.get_flag(arg.name) {
                    args.set(arg.name, true);
                }
            }
            ArgType::Int => {
                if let Some(v) = matches.get_one::<i64>(arg.name) {
                    args.set(arg.name, *v);
                }
            }
            ArgType::Float => {
                if let Some(v) = matches.get_one::<f64>(arg.name) {
                    args.set(arg.name, *v);
                }
            }
            ArgType::StrList => {
                let items: Vec<Value> = matches
                    .get_many::<String>(arg.name)
                    .map(|vals| vals.map(|s| Value::String(s.clone())).collect())
                    .unwrap_or_default();
                if !items.is_empty() {
                    args.set(arg.name, Value::Array(items));
                }
            }
            ArgType::Str | ArgType::Path => {
                if let Some(v) = matches.get_one::<String>(arg.name) {
                    args.set(arg.name, v.clone());
                }
            }
        }
    }
    Ok(args)
}

/// Decode an HTTP query string or form body into [`Args`] using the spec's
/// declared types, so `?ms=500` becomes an integer and not a string.
pub fn args_from_pairs<'a, I>(spec: &VerbSpec, pairs: I) -> Result<Args, Error>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut args = Args::new();
    for (key, raw) in pairs {
        let Some(arg) = spec.arg(key) else {
            return Err(Error::BadArgs {
                verb: spec.name.to_string(),
                detail: format!("unknown argument {key:?}"),
            });
        };
        let value = parse_scalar(spec.name, arg, raw)?;
        args.set(key, value);
    }
    Ok(args)
}

fn parse_scalar(verb: &str, arg: &ArgSpec, raw: &str) -> Result<Value, Error> {
    let bad = |detail: String| Error::BadArgs {
        verb: verb.to_string(),
        detail,
    };
    match arg.ty {
        ArgType::Str | ArgType::Path => Ok(Value::String(raw.to_string())),
        ArgType::Int => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|e| bad(format!("{}: {e}", arg.name))),
        ArgType::Float => raw
            .parse::<f64>()
            .map(Value::from)
            .map_err(|e| bad(format!("{}: {e}", arg.name))),
        ArgType::Bool => match raw {
            "1" | "true" | "yes" | "on" | "" => Ok(Value::Bool(true)),
            "0" | "false" | "no" | "off" => Ok(Value::Bool(false)),
            other => Err(bad(format!("{}: {other:?} is not a boolean", arg.name))),
        },
        ArgType::StrList => Ok(Value::Array(
            raw.split(',')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect(),
        )),
    }
}

fn default_value(ty: ArgType, raw: &str) -> Value {
    match ty {
        ArgType::Int => raw.parse::<i64>().map(Value::from).unwrap_or(Value::Null),
        ArgType::Float => raw.parse::<f64>().map(Value::from).unwrap_or(Value::Null),
        ArgType::Bool => Value::Bool(matches!(raw, "1" | "true" | "yes" | "on")),
        ArgType::Str | ArgType::Path | ArgType::StrList => Value::String(raw.to_string()),
    }
}

/// Generated reference for `docs/VERBS.md`. A test regenerates and diffs it, so
/// the document cannot drift from the registry.
#[must_use]
pub fn markdown(registry: &[&VerbSpec]) -> String {
    let mut out = String::from(
        "# Verbs\n\nGenerated from the registry by `cargo test -p lurien-driver verbs_doc`. \
         Do not edit by hand.\n\nEvery verb is reachable identically from the `lurien` CLI, \
         `lurien-mcp`, and `lurien serve`: one spec, three transports.\n\nA `selector` \
         argument accepts a CSS selector or one of the semantic forms in \
         [`SELECTORS.md`](SELECTORS.md). Verbs that act wait for the element; \
         `timeout_ms` bounds that wait.\n\n`batch` runs several of these verbs in one \
         call; its step syntax is in [`BATCH.md`](BATCH.md).\n",
    );
    for domain in Domain::all() {
        let verbs: Vec<&&VerbSpec> = registry.iter().filter(|s| s.domain == *domain).collect();
        if verbs.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n\n", domain.as_str()));
        out.push_str("| Verb | Arguments | Output | Stability | Summary |\n|---|---|---|---|---|\n");
        for spec in verbs {
            let args = if spec.args.is_empty() {
                "-".to_string()
            } else {
                spec.args
                    .iter()
                    .map(|a| {
                        if a.required {
                            format!("`{}`", a.name)
                        } else {
                            format!("`{}?`", a.name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} |\n",
                spec.name,
                args,
                output_word(spec.output),
                spec.stability.as_str(),
                spec.summary
            ));
        }
    }
    out
}

const fn output_word(kind: OutputKind) -> &'static str {
    match kind {
        OutputKind::Empty => "none",
        OutputKind::Text => "text",
        OutputKind::Json => "json",
        OutputKind::Png => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verb::lookup;

    #[test]
    fn schema_marks_required_and_refuses_extras() {
        let spec = lookup("fill").expect("fill");
        let schema = json_schema(spec);
        assert_eq!(schema["additionalProperties"], json!(false));
        let required = schema["required"].as_array().expect("required");
        assert!(required.contains(&json!("selector")));
        assert!(required.contains(&json!("text")));
    }

    #[test]
    fn http_pairs_use_the_declared_type() {
        let spec = lookup("wait").expect("wait");
        let args = args_from_pairs(spec, [("ms", "250")]).expect("decode");
        assert_eq!(args.u64("ms", 0), 250);
    }

    #[test]
    fn http_pairs_reject_an_unknown_argument() {
        let spec = lookup("wait").expect("wait");
        assert!(args_from_pairs(spec, [("seconds", "1")]).is_err());
    }
}
