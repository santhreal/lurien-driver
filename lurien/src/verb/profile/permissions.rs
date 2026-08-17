//! What this session answers when a page asks for a capability.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "permissions",
    aliases: &["profile.permissions"],
    domain: Domain::Profile,
    summary: "Report what this session answers for geolocation, notifications, camera and the rest. \
              Set at launch with --allow and --prompt; a live session cannot change it.",
    args: &[
        ArgSpec {
            name: "allow",
            ty: ArgType::StrList,
            required: false,
            default: None,
            help: "Refused here. Permissions are a launch property; relaunch with --allow.",
        },
        ArgSpec {
            name: "prompt",
            ty: ArgType::StrList,
            required: false,
            default: None,
            help: "Refused here. Permissions are a launch property; relaunch with --prompt.",
        },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    // Gecko reads permissions.default.* when it starts and nothing a driver can
    // send changes them, so an attempt to set one here is refused with the flag
    // that does work rather than silently ignored.
    for key in ["allow", "prompt"] {
        if args.as_map().contains_key(key) {
            let names = args.str_list(key)?.join(",");
            return Err(Error::BadArgs {
                verb: "permissions".to_string(),
                detail: format!(
                    "a permission cannot change in a live session; \
                     relaunch with --{key} {names} (CLI) or {key}: {names:?} at launch"
                ),
            });
        }
    }
    Ok(Output::Json(session.options().permissions.to_json()))
}
