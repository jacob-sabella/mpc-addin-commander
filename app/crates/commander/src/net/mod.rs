//! The network side: one task owning the WebSocket connection (with reconnect and backoff),
//! and skin fetches over plain HTTP into the disk cache.

pub mod http;
pub mod skin;
pub mod ws;

use crate::model::Shared;
use commander_protocol::ClientMessage;
use std::time::Duration;
use tokio::sync::mpsc;

/// What the UI asks the network task to do.
#[derive(Debug)]
pub enum Cmd {
    Connect { host: String, port: u16 },
    Disconnect,
    Send(ClientMessage),
}

pub type CmdTx = mpsc::UnboundedSender<Cmd>;
pub type CmdRx = mpsc::UnboundedReceiver<Cmd>;

/// Keeps the connection the UI asked for alive: a lost connection is retried with a backoff
/// from one second to fifteen, a `Disconnect` stops it, a new `Connect` replaces it.
pub async fn run(model: Shared, mut rx: CmdRx) {
    let mut target: Option<(String, u16)> = None;
    let mut backoff = Duration::from_secs(1);
    let mut attempt = 0u32;
    loop {
        let (host, port) = match target.clone() {
            Some(t) => t,
            None => match rx.recv().await {
                Some(Cmd::Connect { host, port }) => {
                    target = Some((host.clone(), port));
                    attempt = 0;
                    (host, port)
                }
                Some(_) => continue,
                None => return,
            },
        };
        attempt += 1;
        match ws::session(&model, &mut rx, &host, port, attempt).await {
            ws::End::Disconnect => {
                target = None;
                backoff = Duration::from_secs(1);
                model.lock().unwrap().log("disconnected");
            }
            ws::End::Reconnect(h, p) => {
                target = Some((h, p));
                attempt = 0;
                backoff = Duration::from_secs(1);
            }
            ws::End::Lost(why) => {
                {
                    let mut m = model.lock().unwrap();
                    m.log(format!("{why}; retrying in {} s", backoff.as_secs()));
                    m.conn = crate::model::ConnState::Connecting {
                        host: host.clone(),
                        port,
                        attempt,
                    };
                }
                // Wait out the backoff, unless the UI changes its mind meanwhile.
                let sleep = tokio::time::sleep(backoff);
                tokio::pin!(sleep);
                loop {
                    tokio::select! {
                        _ = &mut sleep => break,
                        cmd = rx.recv() => match cmd {
                            Some(Cmd::Connect { host, port }) => {
                                target = Some((host, port));
                                attempt = 0;
                                backoff = Duration::from_secs(1);
                                break;
                            }
                            Some(Cmd::Disconnect) => {
                                target = None;
                                let mut m = model.lock().unwrap();
                                m.conn = crate::model::ConnState::Disconnected;
                                m.log("disconnected");
                                break;
                            }
                            Some(Cmd::Send(_)) => {}
                            None => return,
                        },
                    }
                }
                backoff = (backoff * 2).min(Duration::from_secs(15));
            }
            ws::End::Closed => return,
        }
    }
}
