//! Move where this session says it is.

use crate::error::Error;
use crate::geo::{Position, ACCURACY_M};
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "geolocation-set",
    aliases: &["profile.geolocation-set", "geo-set"],
    domain: Domain::Profile,
    summary: "Serve a different position from the next fix on. The live page needs no reload.",
    args: &[
        ArgSpec {
            name: "latitude",
            ty: ArgType::Float,
            required: true,
            default: None,
            help: "Decimal degrees, -90 to 90.",
        },
        ArgSpec {
            name: "longitude",
            ty: ArgType::Float,
            required: true,
            default: None,
            help: "Decimal degrees, -180 to 180.",
        },
        ArgSpec {
            name: "accuracy_m",
            ty: ArgType::Float,
            required: false,
            default: Some("55"),
            help: "Reported accuracy in metres.",
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
    let geo = session.geo()?;
    let position = Position::new(
        args.f64("latitude", 0.0),
        args.f64("longitude", 0.0),
        args.f64("accuracy_m", ACCURACY_M),
    )?;
    // The engine has to be running: the position lives in the process that owns
    // the tab, so there is nothing to move until there is a tab.
    session.browser().await?;
    geo.set(position).await?;
    Ok(Output::Json(super::geolocation::report(session, geo)?))
}
