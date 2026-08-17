//! lurien CLI. A transport over `Session::call`, exactly like `lurien-mcp`.
//!
//! Subcommands, flags, and help text are generated from the verb registry, so
//! the CLI cannot expose a different API from the MCP server: they read the same
//! specs. Adding a verb adds a subcommand with no edit here.

use clap::{Arg, ArgAction, ArgMatches, Command};
use guise::StealthProfile;
use lurien::verb::{self, schema, Args, Output};
use lurien::{version_line, BrowserLaunchOptions, Error, Session};
use std::io::Read;
use std::process::ExitCode;

const ABOUT: &str = "A Firefox you drive like Playwright. Engine required.";
const LONG_ABOUT: &str = "\
lurien is a Firefox you drive like Playwright. Persona is coherent from TLS to \
the pixel. Captchas are a property of goto. There is no challenge tool. Engine \
required. v1 is Linux x86_64, headful. Honest leaks: matched-host Linux Firefox \
only.\n\n\
Every verb below is the same verb lurien-mcp exposes: one registry, two faces. \
`lurien run` executes several verbs against one page.";

#[tokio::main]
async fn main() -> ExitCode {
    let matches = cli().get_matches();
    match run(&matches).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Top-level command: fixed subcommands plus one generated per verb.
fn cli() -> Command {
    let mut cmd = Command::new("lurien")
        .version(env!("CARGO_PKG_VERSION"))
        .about(ABOUT)
        .long_about(LONG_ABOUT)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .args(global_args())
        .subcommand(Command::new("version").about("Print crate and engine versions."))
        .subcommand(Command::new("verbs").about("List every verb, its arguments, and its domain."))
        .subcommand(
            Command::new("run")
                .about("Run several verbs against one page. Reads lines from a file or stdin.")
                .arg(
                    Arg::new("script")
                        .help("Script path, or - for stdin.")
                        .default_value("-"),
                ),
        )
        .subcommand(
            Command::new("serve")
                .about("Serve named sessions over HTTP. Every verb, one process, many browsers.")
                .arg(
                    Arg::new("bind")
                        .long("bind")
                        .help("Address to bind. Defaults to LURIEN_SERVE_BIND, else 127.0.0.1:7432."),
                ),
        );
    for spec in verb::registry() {
        cmd = cmd.subcommand(schema::clap_command(spec));
    }
    cmd
}

/// Session-wide flags. Global so they may precede or follow the verb.
fn global_args() -> Vec<Arg> {
    vec![
        Arg::new("persona")
            .long("persona")
            .global(true)
            .default_value("FirefoxLinux")
            .help("Persona to wear. v1 is matched-host FirefoxLinux."),
        Arg::new("headless")
            .long("headless")
            .global(true)
            .action(ArgAction::SetTrue)
            .help("Launch headless. Documented weaker mode; headful is the default."),
        Arg::new("profile-dir")
            .long("profile-dir")
            .global(true)
            .help("Reuse this Firefox profile directory instead of a fresh one."),
        Arg::new("proxy")
            .long("proxy")
            .global(true)
            .help("Proxy URL. An unreachable proxy is an error, never a direct fallback."),
        Arg::new("download-dir")
            .long("download-dir")
            .global(true)
            .help("Directory downloads land in. Default is a fresh one per session."),
        Arg::new("allow")
            .long("allow")
            .global(true)
            .help("Permissions granted without asking, comma separated. Default is deny."),
        Arg::new("prompt")
            .long("prompt")
            .global(true)
            .help("Permissions the browser asks about. A prompt nobody answers blocks the page."),
        Arg::new("geolocation")
            .long("geolocation")
            .global(true)
            .help("Position pages read, as lat,lon[,accuracy_m]. Default is the persona's region."),
    ]
}

async fn run(matches: &ArgMatches) -> Result<(), Error> {
    let (name, sub) = matches
        .subcommand()
        .ok_or_else(|| Error::Other("no subcommand".into()))?;
    match name {
        "version" => {
            lurien::resolve_engine_checked()?;
            println!("{}", version_line());
            Ok(())
        }
        "verbs" => {
            print!("{}", schema::markdown(verb::registry()));
            Ok(())
        }
        "run" => {
            let script = sub
                .get_one::<String>("script")
                .map(String::as_str)
                .unwrap_or("-");
            run_script(matches, script).await
        }
        "serve" => lurien::serve::run(sub.get_one::<String>("bind").map(String::as_str)).await,
        verb_name => {
            let spec = verb::lookup(verb_name).ok_or_else(|| Error::UnknownVerb {
                name: verb_name.to_string(),
            })?;
            let args = schema::args_from_matches(spec, sub)?;
            let session = Session::with_options(launch_options(matches)?);
            let result = session.call(spec.name, &args).await;
            let closed = session.close().await;
            emit(&result?);
            closed
        }
    }
}

/// One session, many verbs. Each line is `verb [positional...] [key=value...]`;
/// `#` starts a comment. A failing line stops the run and closes the page, so a
/// script cannot half-succeed silently.
async fn run_script(matches: &ArgMatches, script: &str) -> Result<(), Error> {
    let source = if script == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| Error::Other(format!("read stdin: {e}")))?;
        buf
    } else {
        std::fs::read_to_string(script).map_err(|e| Error::Other(format!("read {script}: {e}")))?
    };
    let session = Session::with_options(launch_options(matches)?);
    let mut failure = None;
    for (lineno, line) in source.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match call_line(&session, line).await {
            Ok(output) => emit(&output),
            Err(e) => {
                failure = Some(Error::Other(format!("line {}: {e}", lineno + 1)));
                break;
            }
        }
    }
    let closed = session.close().await;
    match failure {
        Some(e) => Err(e),
        None => closed,
    }
}

async fn call_line(session: &Session, line: &str) -> Result<Output, Error> {
    let (name, args) = parse_line(line)?;
    session.call(&name, &args).await
}

/// `goto https://example.com`, `fill "#user" alice`, `wait ms=250`.
/// Positional words fill the required arguments in declaration order; `k=v`
/// pairs set anything by name.
fn parse_line(line: &str) -> Result<(String, Args), Error> {
    let words = split_words(line);
    let mut words = words.into_iter();
    let name = words.next().ok_or_else(|| Error::Other("empty line".into()))?;
    let spec = verb::lookup(&name).ok_or_else(|| Error::UnknownVerb { name: name.clone() })?;
    let mut positional = spec.args.iter().filter(|a| a.required);
    let mut pairs: Vec<(String, String)> = Vec::new();
    for word in words {
        match word.split_once('=') {
            Some((key, value)) if spec.arg(key).is_some() => {
                pairs.push((key.to_string(), value.to_string()));
            }
            _ => {
                let arg = positional.next().ok_or_else(|| Error::BadArgs {
                    verb: spec.name.to_string(),
                    detail: format!("unexpected argument {word:?}"),
                })?;
                pairs.push((arg.name.to_string(), word));
            }
        }
    }
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let args = schema::args_from_pairs(spec, borrowed)?;
    Ok((spec.name.to_string(), args))
}

/// Split on whitespace, honoring single and double quotes so a selector may
/// contain spaces.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (None, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn emit(output: &Output) {
    match output {
        Output::Empty => println!("ok"),
        Output::Png(bytes) => println!("png {} bytes", bytes.len()),
        other => println!("{}", other.to_text()),
    }
}

fn launch_options(matches: &ArgMatches) -> Result<BrowserLaunchOptions, Error> {
    let persona = matches
        .get_one::<String>("persona")
        .map(String::as_str)
        .unwrap_or("FirefoxLinux");
    let proxy = match matches.get_one::<String>("proxy") {
        Some(url) => Some(runtime_foxdriver::ProxyConfig::from_url(url).map_err(|e| {
            Error::ProxyUnreachable {
                url: url.clone(),
                detail: e.to_string(),
            }
        })?),
        None => None,
    };
    let list = |key: &str| {
        matches
            .get_one::<String>(key)
            .map(|v| lurien::PermissionPolicy::split_list(v))
            .unwrap_or_default()
    };
    let geolocation = match matches.get_one::<String>("geolocation") {
        Some(spec) => Some(lurien::geo::parse_position(spec)?),
        None => None,
    };
    Ok(BrowserLaunchOptions {
        profile: parse_persona(persona)?,
        headless: matches.get_flag("headless"),
        profile_dir: matches.get_one::<String>("profile-dir").cloned(),
        proxy,
        download_dir: matches.get_one::<String>("download-dir").cloned(),
        permissions: lurien::PermissionPolicy::from_lists(&list("allow"), &list("prompt"))?,
        geolocation,
        geo: None,
    })
}

fn parse_persona(name: &str) -> Result<StealthProfile, Error> {
    match name {
        "FirefoxLinux" | "firefox-linux" | "linux" => Ok(StealthProfile::FirefoxLinux),
        "FirefoxWindows" | "firefox-windows" => Ok(StealthProfile::FirefoxWindows),
        "FirefoxMacStable" | "firefox-mac" => Ok(StealthProfile::FirefoxMacStable),
        other => Err(Error::Other(format!(
            "unknown persona {other:?}. lurien v1: FirefoxLinux (matched-host)."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_is_a_subcommand() {
        let cmd = cli();
        let names: Vec<&str> = cmd.get_subcommands().map(clap::Command::get_name).collect();
        for spec in verb::registry() {
            assert!(names.contains(&spec.name), "{} is not a subcommand", spec.name);
        }
    }

    #[test]
    fn cli_parses_a_verb_with_a_positional() {
        let m = cli().try_get_matches_from(["lurien", "goto", "https://example.com"]).expect("parse");
        let (name, sub) = m.subcommand().expect("subcommand");
        assert_eq!(name, "goto");
        let spec = verb::lookup(name).expect("goto");
        let args = schema::args_from_matches(spec, sub).expect("args");
        assert_eq!(args.str("url").expect("url"), "https://example.com");
    }

    #[test]
    fn script_lines_take_positionals_and_pairs() {
        let (name, args) = parse_line("fill '#user' alice").expect("fill");
        assert_eq!(name, "fill");
        assert_eq!(args.str("selector").expect("selector"), "#user");
        assert_eq!(args.str("text").expect("text"), "alice");

        let (name, args) = parse_line("wait ms=250").expect("wait");
        assert_eq!(name, "wait");
        assert_eq!(args.u64("ms", 0), 250);
    }

    #[test]
    fn script_line_with_an_unknown_verb_is_named() {
        let err = parse_line("teleport moon").expect_err("unknown verb");
        assert!(err.to_string().contains("teleport"), "{err}");
    }

    #[test]
    fn quoted_words_survive_spaces() {
        assert_eq!(
            split_words("click \"div .a b\" extra"),
            vec!["click", "div .a b", "extra"]
        );
    }
}
