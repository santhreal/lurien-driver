//! Where this session says it is.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "geolocation",
    aliases: &["profile.geolocation", "geo"],
    domain: Domain::Profile,
    summary: "Report the position pages read from this session, and whether they may read it.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &crate::verb::Args) -> Result<Output, Error> {
    let geo = session.geo()?;
    // The engine has to be running: what the report names is the channel that
    // serves the position and the policy the profile was launched with, and
    // neither exists until there is a browser.
    session.browser().await?;
    Ok(Output::Json(report(session, geo)?))
}

/// One shape for reading and for moving, so a caller never has to reconcile two.
///
/// # Errors
///
/// [`Error::GeolocationUnavailable`] when this session serves no position at
/// all: the persona names no region and nobody passed coordinates.
pub(crate) fn report(
    session: &Session,
    geo: &crate::geo::Geolocation,
) -> Result<serde_json::Value, Error> {
    let position = geo
        .position()
        .ok_or_else(|| Error::GeolocationUnavailable {
            detail: format!(
                "persona {:?} names no region, and no coordinates were passed",
                session.options().profile
            ),
        })?;
    Ok(serde_json::json!({
        "latitude": position.latitude,
        "longitude": position.longitude,
        "accuracy_m": position.accuracy_m,
        "source": if geo.is_persona() { "persona" } else { "override" },
        "permission": session
            .options()
            .permissions
            .grant_of("geolocation")
            .as_str(),
        "control_port": geo.control().port(),
    }))
}
