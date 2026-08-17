//! `lurien-vision`: the slider helper process.
//!
//! Started by whoever runs the browser, told a loopback port, and named in the
//! session's `LURIEN_CHALLENGE` as `helper`. It prints the port it bound so a
//! caller can pass 0 and let the kernel choose.

use std::net::SocketAddr;
use std::process::ExitCode;

fn usage() -> String {
    "usage: lurien-vision [--host 127.0.0.1] [--port N]\n\
     \n\
     Answers one JSON request per connection: {kind, task, png, width, height}.\n\
     Replies {dx, dy, confidence} or {error}. Loopback only."
        .to_string()
}

fn main() -> ExitCode {
    let mut host = "127.0.0.1".to_string();
    let mut port = 0u16;
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
    println!("{{\"listening\":\"{bound}\",\"port\":{}}}", bound.port());
    lurien_vision::server::serve(&listener);
    ExitCode::SUCCESS
}
