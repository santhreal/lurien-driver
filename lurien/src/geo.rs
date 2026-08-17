//! Where the browser thinks it is.
//!
//! A position is not a page property and not a network answer: it lives in the
//! geolocation service of the process that owns the tab. Gecko asks a location
//! provider once and serves that fix to every later reader, so a driver that
//! answers as a provider can choose where a session starts and nothing more. The
//! engine therefore applies positions itself, through the actor that already
//! runs in the tab's process, and this module owns the driver's half: what the
//! session serves, and the call that moves it.
//!
//! The position a session starts from is the persona's: the timezone the persona
//! presents names a region, and that region's coordinates are what
//! [`guise::fingerprint::identity_geo_coherence`] checks a persona against. A
//! session therefore cannot report a city that contradicts its own clock.

use crate::control::Control;
use crate::error::Error;
use guise::StealthProfile;
use std::sync::{Mutex, PoisonError};

/// Accuracy in metres reported with a served fix. A network fix is a
/// neighbourhood, not a doorstep, so this is the plausible order of magnitude
/// for one derived from a region rather than from hardware.
pub const ACCURACY_M: f64 = 55.0;

/// A fix, as a page reads it off `coords`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Latitude in decimal degrees, -90 to 90.
    pub latitude: f64,
    /// Longitude in decimal degrees, -180 to 180.
    pub longitude: f64,
    /// Reported accuracy in metres, greater than zero.
    pub accuracy_m: f64,
}

impl Position {
    /// A fix, refusing coordinates no place has.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when a coordinate is outside its range, is not finite,
    /// or the accuracy is not positive.
    pub fn new(latitude: f64, longitude: f64, accuracy_m: f64) -> Result<Self, Error> {
        let bad = |detail: String| Error::BadArgs {
            verb: "geolocation-set".to_string(),
            detail,
        };
        if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            return Err(bad(format!(
                "latitude {latitude} is not a latitude; pass -90 to 90"
            )));
        }
        if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
            return Err(bad(format!(
                "longitude {longitude} is not a longitude; pass -180 to 180"
            )));
        }
        if !accuracy_m.is_finite() || accuracy_m <= 0.0 {
            return Err(bad(format!(
                "accuracy_m {accuracy_m} is not a distance; pass metres greater than 0"
            )));
        }
        Ok(Self {
            latitude,
            longitude,
            accuracy_m,
        })
    }
}

/// The position a session serves, and the channel that moves it.
///
/// Created before launch, because the engine reads the channel out of its own
/// environment and applies the starting position to the first window it opens. A
/// later change goes to the same channel, which reaches pages that are already
/// loaded: the geolocation service pushes the new fix to every watcher and
/// answers the next read with it.
#[derive(Debug)]
pub struct Geolocation {
    control: Control,
    /// Where the persona itself is, so a cleared override has somewhere to go.
    persona: Option<Position>,
    current: Mutex<Option<Position>>,
}

impl Geolocation {
    /// The geolocation state of a session that has not launched yet.
    ///
    /// `launch` is what a caller asked for and outranks the persona's own
    /// region; with neither, the session serves no position at all rather than
    /// inventing one.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when no control port can be reserved.
    pub fn new(persona: Option<Position>, launch: Option<Position>) -> Result<Self, Error> {
        Ok(Self {
            control: Control::reserve()?,
            persona,
            current: Mutex::new(launch.or(persona)),
        })
    }

    /// The channel this session's engine listens on.
    #[must_use]
    pub const fn control(&self) -> &Control {
        &self.control
    }

    /// The environment entry that hands the engine this channel and the position
    /// to start from.
    #[must_use]
    pub fn env_entry(&self) -> (String, String) {
        self.control.env_entry(self.position())
    }

    /// The fix a page reads right now, or `None` when the session serves none.
    #[must_use]
    pub fn position(&self) -> Option<Position> {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The persona's own fix, whatever the current position is.
    #[must_use]
    pub const fn persona_position(&self) -> Option<Position> {
        self.persona
    }

    /// Whether the current fix is still the persona's.
    #[must_use]
    pub fn is_persona(&self) -> bool {
        self.position() == self.persona
    }

    /// Serve `position` to every page of this session, loaded or not yet.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine did not take it. The
    /// remembered position is only updated once the engine has.
    pub async fn set(&self, position: Position) -> Result<(), Error> {
        self.control.set_position(position).await?;
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(position);
        Ok(())
    }

    /// Go back to the persona's own fix, or to no position when it has none.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine did not take it.
    pub async fn clear(&self) -> Result<(), Error> {
        match self.persona {
            Some(persona) => self.control.set_position(persona).await?,
            None => self.control.clear_position().await?,
        }
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = self.persona;
        Ok(())
    }
}

/// The persona's own fix: the coordinates of the region its timezone names.
///
/// `None` for a timezone the reference does not cover, which is the same
/// condition [`guise::fingerprint::identity_geo_coherence`] reports as unknown
/// rather than coherent. A session in that state serves no position at all
/// instead of inventing one.
#[must_use]
pub fn persona_position(profile: StealthProfile) -> Option<Position> {
    let timezone = guise::profile_to_overrides(&profile).timezone;
    let facts = guise::fingerprint::timezone_facts(&timezone)?;
    Position::new(facts.lat, facts.lon, ACCURACY_M).ok()
}

/// Prefs that leave the position to this session and nothing else.
///
/// Every platform provider is turned off by name, and the network provider is
/// pointed nowhere. Linux would otherwise reach the host's GeoClue daemon, which
/// answers with the real location and would make the persona's coordinates a lie
/// the browser itself contradicts. With an override in place no provider is
/// consulted at all; these prefs decide what happens in the gap before the
/// engine applies one, and for a session that serves no position.
#[must_use]
pub fn prefs() -> String {
    "user_pref(\"geo.enabled\", true);\n\
     user_pref(\"geo.provider.network.url\", \"\");\n\
     user_pref(\"geo.provider.network.scan\", false);\n\
     user_pref(\"geo.provider.network.logging.enabled\", false);\n\
     user_pref(\"geo.provider.use_geoclue\", false);\n\
     user_pref(\"geo.provider.use_corelocation\", false);\n\
     user_pref(\"geo.provider.use_gpsd\", false);\n\
     user_pref(\"geo.provider.use_gpsd_ns\", false);\n\
     user_pref(\"geo.provider.testing\", false);\n"
        .to_string()
}

/// Parse `lat,lon` or `lat,lon,accuracy_m`, as a flag or a wire argument carries
/// it. One parser, so the CLI and the HTTP face cannot disagree about a comma.
///
/// # Errors
///
/// [`Error::BadArgs`] naming the bad token, or the arity that was given.
pub fn parse_position(spec: &str) -> Result<Position, Error> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    let bad = |detail: String| Error::BadArgs {
        verb: "geolocation".to_string(),
        detail,
    };
    if parts.len() < 2 || parts.len() > 3 {
        return Err(bad(format!(
            "{spec:?} is not a position; pass lat,lon or lat,lon,accuracy_m"
        )));
    }
    let number = |token: &str, name: &str| {
        token.parse::<f64>().map_err(|_| {
            bad(format!(
                "{token:?} is not a number; {name} is decimal degrees, as in 52.52"
            ))
        })
    };
    let latitude = number(parts[0], "latitude")?;
    let longitude = number(parts[1], "longitude")?;
    let accuracy_m = match parts.get(2) {
        Some(token) => token.parse::<f64>().map_err(|_| {
            bad(format!(
                "{token:?} is not a number; accuracy_m is metres, as in 55"
            ))
        })?,
        None => ACCURACY_M,
    };
    Position::new(latitude, longitude, accuracy_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coordinate_no_place_has_is_refused() {
        for (lat, lon, acc, want) in [
            (91.0, 0.0, ACCURACY_M, "latitude"),
            (0.0, -181.0, ACCURACY_M, "longitude"),
            (0.0, 0.0, 0.0, "accuracy_m"),
            (f64::NAN, 0.0, ACCURACY_M, "latitude"),
        ] {
            let err = Position::new(lat, lon, acc).expect_err("must be refused");
            let text = err.to_string();
            assert!(text.contains(want), "{text}");
            assert!(
                text.contains("pass"),
                "a refusal must say what to pass: {text}"
            );
        }
    }

    #[test]
    fn every_firefox_persona_serves_coordinates_its_own_timezone_agrees_with() {
        // The persona's clock and its coordinates come from one table, so a
        // session can never present a city its timezone contradicts. The persona
        // list is read from guise at run time: a new Firefox persona with no
        // region turns this red instead of silently serving the host's location.
        use guise::fingerprint::{
            profile_user_agent, user_agent_facts, UserAgentBrowser, ALL_PROFILES,
        };
        let mut checked = 0;
        for &profile in ALL_PROFILES {
            if user_agent_facts(profile_user_agent(profile)).browser != UserAgentBrowser::Firefox {
                continue;
            }
            let position = persona_position(profile)
                .unwrap_or_else(|| panic!("{profile:?} has no region to serve a position from"));
            let timezone = guise::profile_to_overrides(&profile).timezone;
            let facts = guise::fingerprint::timezone_facts(&timezone)
                .unwrap_or_else(|| panic!("{profile:?} presents unknown zone {timezone}"));
            assert!(
                (position.latitude - facts.lat).abs() < f64::EPSILON
                    && (position.longitude - facts.lon).abs() < f64::EPSILON,
                "{profile:?} serves {position:?}, its zone {timezone} is at {}, {}",
                facts.lat,
                facts.lon
            );
            checked += 1;
        }
        assert!(checked > 0, "no Firefox persona was checked");
    }

    #[test]
    fn a_position_flag_is_read_the_same_way_on_every_face() {
        let two = parse_position("52.52,13.405").expect("lat,lon");
        assert_eq!(two.accuracy_m, ACCURACY_M);
        let three = parse_position(" 48.8566 , 2.3522 , 30 ").expect("lat,lon,accuracy");
        assert_eq!(
            (three.latitude, three.longitude, three.accuracy_m),
            (48.8566, 2.3522, 30.0)
        );
        for (bad, want) in [
            ("52.52", "lat,lon"),
            ("52.52,13.405,55,1", "lat,lon"),
            ("north,13.405", "not a number"),
            ("91,13.405", "latitude"),
        ] {
            let err = parse_position(bad).expect_err("must be refused");
            assert!(err.to_string().contains(want), "{bad}: {err}");
        }
    }

    #[test]
    fn prefs_leave_no_provider_that_could_answer_for_the_host() {
        let prefs = prefs();
        assert!(
            prefs.contains("user_pref(\"geo.provider.network.url\", \"\")"),
            "the network provider must point nowhere: {prefs}"
        );
        for host_provider in [
            "geo.provider.use_geoclue",
            "geo.provider.use_corelocation",
            "geo.provider.use_gpsd",
            "geo.provider.use_gpsd_ns",
        ] {
            assert!(
                prefs.contains(&format!("user_pref(\"{host_provider}\", false)")),
                "{host_provider} must be off, or the host answers: {prefs}"
            );
        }
        assert!(
            prefs.contains("user_pref(\"geo.enabled\", true)"),
            "a session that serves a position still needs the API on: {prefs}"
        );
    }

    /// A launch is where the two halves meet: the engine reads the position out
    /// of the environment, so what the session thinks it serves and what it puts
    /// in that variable have to be the same three numbers.
    #[test]
    fn a_launch_carries_the_position_the_session_says_it_serves() {
        let persona = Position::new(40.7128, -74.006, ACCURACY_M).expect("persona");
        let geo = Geolocation::new(Some(persona), None).expect("state");
        assert_eq!(geo.position(), Some(persona));
        assert!(geo.is_persona());
        let (key, value) = geo.env_entry();
        assert_eq!(key, crate::control::CONTROL_ENV);
        let json: serde_json::Value = serde_json::from_str(&value).expect("value parses");
        assert_eq!(json["position"]["latitude"], 40.7128);
        assert_eq!(json["port"], geo.control().port());

        // A caller's own coordinates outrank the persona's region, and the
        // persona's stay reachable so a clear has somewhere to go.
        let asked = Position::new(52.52, 13.405, 30.0).expect("asked");
        let with = Geolocation::new(Some(persona), Some(asked)).expect("state");
        assert_eq!(with.position(), Some(asked));
        assert!(!with.is_persona());
        assert_eq!(with.persona_position(), Some(persona));
        let json: serde_json::Value =
            serde_json::from_str(&with.env_entry().1).expect("value parses");
        assert_eq!(json["position"]["accuracy"], 30.0);
    }

    /// A persona whose zone names no region serves nothing, and the launch value
    /// then has no position key at all: an invented position is a session whose
    /// coordinates contradict its own clock.
    #[test]
    fn a_session_with_no_region_serves_no_position() {
        let geo = Geolocation::new(None, None).expect("state");
        assert_eq!(geo.position(), None);
        assert!(geo.is_persona());
        let json: serde_json::Value =
            serde_json::from_str(&geo.env_entry().1).expect("value parses");
        assert!(
            json.get("position").is_none(),
            "a session with no region sent a position anyway: {json}"
        );
    }
}
