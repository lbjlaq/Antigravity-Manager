use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let mut port_override: Option<u16> = None;
    let mut host_override: Option<String> = None;
    let mut disable_web = false;

    // Check for help flags specifically for server first
    if args.iter().any(|a| a == "--help") {
        println!(
            "Antigravity Headless Server\n\n\
            Usage:\n  antigravity-server [OPTIONS] [COMMAND]\n\n\
            Options:\n  \
            -p, --port <PORT>        Set listening port (default: 8045)\n  \
            -h, --host <HOST>        Set listening host (e.g. 127.0.0.1 or 0.0.0.0)\n  \
            --no-web, --pure-proxy   Disable Web UI static console (pure proxy mode)\n  \
            --help                   Show help information\n\n\
            Commands:\n  \
            switch, list, current, import, export, sync  (Run 'antigravity-tools help' for CLI command details)\n"
        );
        return Ok(());
    }

    // Check for CLI subcommands (switch, list, current, status, etc.)
    match antigravity_tools_lib::modules::cli_handler::try_handle_cli(&args).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => {
            eprintln!("\x1b[31m[Antigravity CLI Error]\x1b[0m {}", e);
            std::process::exit(1);
        }
    }

    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                if let Some(val) = iter.next() {
                    if let Ok(p) = val.parse::<u16>() {
                        port_override = Some(p);
                    } else {
                        eprintln!("Invalid port number: {}", val);
                        std::process::exit(1);
                    }
                }
            }
            "--host" | "-h" => {
                if let Some(val) = iter.next() {
                    host_override = Some(val);
                }
            }
            "--no-web" | "--pure-proxy" => {
                disable_web = true;
            }
            _ => {}
        }
    }

    antigravity_tools_lib::run_headless_server(port_override, host_override, disable_web).await
}
