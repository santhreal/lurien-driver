//! `lurien-vision`: the slider helper process.
//!
//! Started by whoever runs the browser, told a loopback port, and named in the
//! session's `LURIEN_CHALLENGE` as `helper`. It prints the port it bound so a
//! caller can pass 0 and let the kernel choose.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

/// Environment variable naming the session token, for a caller that would rather
/// not put it in a command line every other process on the host can read.
const TOKEN_ENV: &str = "LURIEN_HELPER_TOKEN";

fn usage() -> String {
    "usage: lurien-vision --token T [--host 127.0.0.1] [--port N] [--model DIR]\n\
     \n\
     Answers one JSON request per connection.\n\
     slider: {kind:\"slider\", task:\"axis\", png, width} -> {dx, dy, confidence}.\n\
     grid:   {kind:\"visual\", task:\"cells\", png, width, prompt, cells} -> {cells, scores}.\n\
     \n\
     A grid needs an object detector: --model DIR, or LURIEN_VISION_MODEL. Without\n\
     one, grids are refused by name and sliders are still measured.\n\
     Loopback only, and every request must name this session's token;\n\
     LURIEN_HELPER_TOKEN sets it without a command line.\n\
     See docs/HELPERS.md for the protocol."
        .to_string()
}

fn main() -> ExitCode {
    let mut host = "127.0.0.1".to_string();
    let mut port = 0u16;
    let mut token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let mut model = lurien_vision::detect::model_dir_from_env();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => match args.next() {
                Some(value) => host = value,
                None => {
                    eprintln!("--host needs a value\n{}", usage());
                    return ExitCode::from(2);
                }
            },
            "--port" => match args.next().and_then(|v| v.parse::<u16>().ok()) {
                Some(value) => port = value,
                None => {
                    eprintln!("--port needs a number\n{}", usage());
                    return ExitCode::from(2);
                }
            },
            "--model" => match args.next() {
                Some(value) => model = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--model needs a directory\n{}", usage());
                    return ExitCode::from(2);
                }
            },
            "--token" => match args.next() {
                Some(value) => token = value,
                None => {
                    eprintln!("--token needs a value\n{}", usage());
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument {other}\n{}", usage());
                return ExitCode::from(2);
            }
        }
    }
    if token.is_empty() {
        // Fail at startup rather than answering every request with a refusal: a
        // helper nobody can talk to looks exactly like a helper that is measuring
        // wrong, and the session it was started for would spend its whole budget
        // finding out.
        eprintln!("a session token is required: pass --token or set {TOKEN_ENV}\n{}", usage());
        return ExitCode::from(2);
    }
    let addr: SocketAddr = match format!("{host}:{port}").parse() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("{host}:{port} is not an address: {e}");
            return ExitCode::from(2);
        }
    };
    let listener = match lurien_vision::server::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let bound = match listener.local_addr() {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("cannot read the bound address: {e}");
            return ExitCode::FAILURE;
        }
    };
    // One line, parseable, so a script can start the helper and read its port.
    // It also names whether this helper can answer a grid: a session
    // that reads "model":null and then waits on a grid solve is waiting for a
    // refusal.
    let model_line = match &model {
        Some(dir) => format!("\"{}\"", dir.display()),
        None => "null".to_string(),
    };
    println!(
        "{{\"listening\":\"{bound}\",\"port\":{},\"model\":{model_line}}}",
        bound.port()
    );
    let mut helper = lurien_vision::Helper::new(model);
    lurien_vision::server::serve(&listener, &token, &mut helper);
    ExitCode::SUCCESS
}
