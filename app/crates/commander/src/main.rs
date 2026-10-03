//! Commander: the desktop app for the Commander addin. It connects to the addin's WebSocket,
//! lists the plugin instances on the device, draws the selected one's skin (or a generic
//! panel) and sends parameter changes back.

#![forbid(unsafe_code)]

mod config;
mod model;
mod net;
mod shell;
mod ui;

use std::sync::{Arc, Mutex};

struct Args {
    once: bool,
    host: Option<String>,
    port: Option<u16>,
}

const USAGE: &str = "usage: commander [--host HOST] [--port PORT] [--once]

  --host HOST   connect to this addin host (saved as the default)
  --port PORT   the addin's port (default 6730)
  --once        render one frame without a window and exit (for CI)
";

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        once: false,
        host: None,
        port: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--once" => args.once = true,
            "--host" => args.host = Some(it.next().ok_or_else(|| anyhow::anyhow!(USAGE))?),
            "--port" => {
                let p = it.next().ok_or_else(|| anyhow::anyhow!(USAGE))?;
                args.port = Some(p.parse().map_err(|_| anyhow::anyhow!("bad port {p:?}"))?);
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other:?}\n{USAGE}"),
        }
    }
    Ok(args)
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = parse_args()?;
    let mut config = config::Config::load();
    if let Some(h) = args.host {
        config.host = h;
    }
    if let Some(p) = args.port {
        config.port = p;
    }
    if args.once {
        // No network and no window: build the app and draw one frame.
        config.connect_on_start = false;
    }
    let model = Arc::new(Mutex::new(model::Model::new()));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    runtime.spawn(net::run(model.clone(), cmd_rx));
    let app = ui::App::new(model, cmd_tx, config);
    if args.once {
        let (shapes, prims) = ui::run_once(app)?;
        println!("commander --once: rendered one frame ({shapes} shapes, {prims} primitives)");
        return Ok(());
    }
    shell::run(app)
}
