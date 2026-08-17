//! Several verbs in one call.
//!
//! An agent pays a model round trip for every tool call, and a login is four
//! calls that never needed a decision between them. A batch runs a sequence
//! against one page, stops at the first failure, and reports what each step did,
//! so the caller learns how far the page got rather than only that something
//! broke.
//!
//! Steps are validated before any of them runs. A typo in the fifth step should
//! not leave the page half filled in.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{
    self, ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec,
};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "batch",
    aliases: &["session.batch"],
    domain: Domain::Session,
    summary: "Run several verbs in one call, stopping at the first failure.",
    args: &[ArgSpec {
        name: "steps",
        ty: ArgType::StrList,
        required: true,
        default: None,
        help: "Steps like 'click selector=role:button=Log in'. Quote a value with spaces.",
    }],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let steps = args.str_list("steps")?;
    if steps.is_empty() {
        return Err(Error::BadArgs {
            verb: "batch".to_string(),
            detail: "a batch with no steps has nothing to run".to_string(),
        });
    }
    // Parse and validate everything first: a batch that fails on step five after
    // mutating the page through step four is worse than one that never started.
    let mut plan = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        plan.push(parse(step, index + 1)?);
    }

    let mut done = Vec::with_capacity(plan.len());
    for (index, (name, step_args)) in plan.iter().enumerate() {
        let number = index + 1;
        match session.call(name, step_args).await {
            Ok(output) => done.push(serde_json::json!({
                "step": number,
                "verb": name,
                "output": output.to_json(),
            })),
            Err(error) => {
                return Err(Error::BatchFailed {
                    step: number,
                    verb: name.clone(),
                    detail: error.to_string(),
                    ran: report(&done),
                    skipped: plan.len() - number,
                })
            }
        }
    }
    Ok(Output::Json(serde_json::json!({
        "ran": done.len(),
        "steps": done,
    })))
}

/// What already happened to the page, for the failure message.
fn report(done: &[serde_json::Value]) -> String {
    if done.is_empty() {
        return "nothing ran".to_string();
    }
    done.iter()
        .map(|row| {
            format!(
                "{} {}",
                row["step"],
                row["verb"].as_str().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One step: a verb name and its arguments, typed by the verb's own spec.
fn parse(step: &str, number: usize) -> Result<(String, Args), Error> {
    let step = step.trim();
    let (name, rest) = step.split_once(char::is_whitespace).unwrap_or((step, ""));
    if name.is_empty() {
        return Err(bad(number, "a step needs a verb"));
    }
    if name == "batch" || name == "session.batch" {
        return Err(bad(
            number,
            "a batch cannot run a batch: flatten the steps into one list",
        ));
    }
    let spec = verb::lookup(name).ok_or_else(|| Error::UnknownVerb {
        name: format!("{name} (step {number})"),
    })?;
    let mut args = Args::new();
    for (key, value) in pairs(rest, number)? {
        let arg = spec.arg(&key).ok_or_else(|| {
            let known: Vec<&str> = spec.args.iter().map(|a| a.name).collect();
            bad(
                number,
                &format!("{name} has no argument {key:?}; accepts {known:?}"),
            )
        })?;
        args.set(key, typed(arg.ty, &value, number, arg.name)?);
    }
    args.validate(spec)?;
    Ok((name.to_string(), args))
}

/// `key=value` pairs, where a value may be double-quoted to hold spaces.
fn pairs(rest: &str, number: usize) -> Result<Vec<(String, String)>, Error> {
    let mut pairs = Vec::new();
    let mut chars = rest.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut key = String::new();
        for c in chars.by_ref() {
            if c == '=' {
                break;
            }
            key.push(c);
        }
        let key = key.trim().to_string();
        if key.is_empty() {
            break;
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '"' {
                    closed = true;
                    break;
                }
                value.push(c);
            }
            if !closed {
                return Err(bad(number, &format!("{key} has an unclosed quote")));
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        pairs.push((key, value));
    }
    Ok(pairs)
}

/// A step's text value in the type the verb declared, so a batch cannot smuggle
/// a string into an integer argument.
fn typed(
    ty: ArgType,
    raw: &str,
    number: usize,
    name: &str,
) -> Result<serde_json::Value, Error> {
    match ty {
        ArgType::Str | ArgType::Path => Ok(serde_json::Value::String(raw.to_string())),
        ArgType::Int => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .map_err(|_| bad(number, &format!("{name} takes an integer, got {raw:?}"))),
        ArgType::Float => raw
            .parse::<f64>()
            .map(serde_json::Value::from)
            .map_err(|_| bad(number, &format!("{name} takes a number, got {raw:?}"))),
        ArgType::Bool => match raw {
            "1" | "true" | "yes" | "on" | "" => Ok(serde_json::Value::Bool(true)),
            "0" | "false" | "no" | "off" => Ok(serde_json::Value::Bool(false)),
            other => Err(bad(
                number,
                &format!("{name} takes true or false, got {other:?}"),
            )),
        },
        // A list in one token is comma separated: a step is a line, and a caller
        // with a comma in a filename can send the batch as JSON instead.
        ArgType::StrList => Ok(serde_json::Value::Array(
            raw.split(',')
                .map(|item| serde_json::Value::String(item.trim().to_string()))
                .collect(),
        )),
    }
}

fn bad(number: usize, detail: &str) -> Error {
    Error::BadArgs {
        verb: "batch".to_string(),
        detail: format!("step {number}: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of a batch is that a caller writes it the way they would write
    /// the calls, so a step is a verb and its arguments by name.
    #[test]
    fn a_step_is_a_verb_and_its_named_arguments() {
        let (name, args) = parse("goto url=https://example.test/", 1).expect("parses");
        assert_eq!(name, "goto");
        assert_eq!(args.opt_str("url"), Some("https://example.test/"));
    }

    /// A selector holds spaces and equals signs, which is exactly the value a
    /// batch has to carry: `role:button=Log in`.
    #[test]
    fn a_quoted_value_keeps_its_spaces_and_equals_signs() {
        let (_, args) = parse("click selector=\"role:button=Log in\"", 1).expect("parses");
        assert_eq!(args.opt_str("selector"), Some("role:button=Log in"));
    }

    /// Arguments are typed by the verb's own spec, or a batch would be the one
    /// face that can send a string where an integer belongs.
    #[test]
    fn values_take_the_type_the_verb_declared() {
        let (_, args) = parse("wait ms=250", 1).expect("parses");
        assert_eq!(args.as_map()["ms"], serde_json::json!(250));
        let err = parse("wait ms=soon", 2).expect_err("an integer is required");
        assert!(err.to_string().contains("step 2"), "{err}");
        assert!(err.to_string().contains("integer"), "{err}");
    }

    /// Every mistake a caller can make in a step has to be caught before the
    /// first step runs, because the page is mutated by then.
    #[test]
    fn a_bad_step_is_refused_by_number() {
        for (step, expected) in [
            ("teleport url=x", "teleport"),
            ("click typo=1", "typo"),
            ("click", "selector"),
            ("click selector=\"unclosed", "unclosed quote"),
            ("batch steps=x", "cannot run a batch"),
        ] {
            let err = parse(step, 3).expect_err(step);
            let text = err.to_string();
            assert!(text.contains(expected), "{step:?} -> {text}");
        }
    }

    /// A list argument still works from a single line.
    #[test]
    fn a_list_argument_is_comma_separated() {
        let (_, args) = parse("upload selector=#f files=/tmp/a.png,/tmp/b.png", 1).expect("parses");
        assert_eq!(
            args.str_list("files").expect("list"),
            vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()]
        );
    }

    /// The failure has to say what already happened, not just what broke.
    #[test]
    fn the_report_names_the_steps_that_ran() {
        let done = vec![
            serde_json::json!({"step": 1, "verb": "goto", "output": null}),
            serde_json::json!({"step": 2, "verb": "fill", "output": null}),
        ];
        assert_eq!(report(&done), "1 goto, 2 fill");
        assert_eq!(report(&[]), "nothing ran");
    }
}
