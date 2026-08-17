//! Wear a real Firefox profile.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "as",
    aliases: &["profile.as"],
    domain: Domain::Profile,
    summary: "Import a real Firefox profile (cookies, logins, localStorage) and switch to it.",
    args: &[
        ArgSpec { name: "profile", ty: ArgType::Path, required: true, default: None, help: "Source Firefox profile directory." },
        ArgSpec { name: "dest", ty: ArgType::Path, required: false, default: None, help: "Where to write the imported copy." },
        ArgSpec { name: "headless", ty: ArgType::Bool, required: false, default: None, help: "Launch headless. Weaker; headful is the default." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let src = args.path("profile")?;
    let dest = args.opt_path("dest");
    let headless = args.bool("headless", session.options().headless);
    // The imported profile joins this session; it does not start a new contract,
    // so permissions and the position provider come along unchanged.
    let opts = crate::launch::LaunchOptions {
        headless,
        ..session.options().clone()
    };
    let (browser, report) = crate::Browser::as_profile(&src, dest.as_deref(), opts).await?;
    session.adopt(browser).await;
    Ok(Output::Json(serde_json::json!({
        "cookies": report.cookies,
        "logins": report.logins,
        "local_storage": report.local_storage,
        "warnings": report.warnings,
    })))
}