//! One WebSocket session: connect, apply what the addin pushes, forward what the UI sends,
//! ping every two seconds for the latency readout, and start skin fetches.

use super::{Cmd, CmdRx};
use crate::model::{ConnState, Shared};
use commander_protocol::{ClientMessage, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

/// Why a session ended.
pub enum End {
    /// The UI asked to disconnect.
    Disconnect,
    /// The UI asked for another host.
    Reconnect(String, u16),
    /// The connection failed or dropped; the reason is for the log.
    Lost(String),
    /// The command channel closed: the app is exiting.
    Closed,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PING_PERIOD: Duration = Duration::from_secs(2);

pub async fn session(model: &Shared, rx: &mut CmdRx, host: &str, port: u16, attempt: u32) -> End {
    let url = format!("ws://{host}:{port}/ws");
    {
        let mut m = model.lock().unwrap();
        m.conn = ConnState::Connecting {
            host: host.to_string(),
            port,
            attempt,
        };
        m.log(format!("connecting to {url} (attempt {attempt})"));
    }
    let connect = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(&url));
    tokio::pin!(connect);
    let ws = loop {
        tokio::select! {
            r = &mut connect => match r {
                Ok(Ok((ws, _))) => break ws,
                Ok(Err(e)) => return End::Lost(format!("connect: {e}")),
                Err(_) => return End::Lost("connect: timed out".into()),
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Connect { host, port }) => return End::Reconnect(host, port),
                Some(Cmd::Disconnect) => {
                    model.lock().unwrap().conn = ConnState::Disconnected;
                    return End::Disconnect;
                }
                Some(Cmd::Send(_)) => {}
                None => return End::Closed,
            },
        }
    };
    {
        let mut m = model.lock().unwrap();
        m.reset_session();
        m.conn = ConnState::Connected {
            host: host.to_string(),
            port,
        };
        m.log(format!("connected to {url}"));
    }
    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(PING_PERIOD);
    let mut ping_sent: Option<Instant> = None;
    let end = loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<ServerMessage>(text.as_str()) {
                        Ok(ServerMessage::Pong) => {
                            if let Some(t) = ping_sent.take() {
                                model.lock().unwrap().latency_ms =
                                    Some(t.elapsed().as_secs_f32() * 1000.0);
                            }
                        }
                        Ok(m) => {
                            let wanted = {
                                let mut model = model.lock().unwrap();
                                model.apply(m);
                                std::mem::take(&mut model.skins_wanted)
                            };
                            for (id, uid) in wanted {
                                tokio::spawn(super::skin::fetch(
                                    model.clone(),
                                    host.to_string(),
                                    port,
                                    id,
                                    uid,
                                ));
                            }
                        }
                        Err(e) => model.lock().unwrap().log(format!("bad message: {e}")),
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    break End::Lost(format!("closed by the addin: {frame:?}"));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => break End::Lost(format!("socket: {e}")),
                None => break End::Lost("socket closed".into()),
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Send(m)) => {
                    let text = match serde_json::to_string(&m) {
                        Ok(t) => t,
                        Err(e) => {
                            model.lock().unwrap().log(format!("encode: {e}"));
                            continue;
                        }
                    };
                    if let Err(e) = sink.send(Message::text(text)).await {
                        break End::Lost(format!("send: {e}"));
                    }
                }
                Some(Cmd::Connect { host, port }) => break End::Reconnect(host, port),
                Some(Cmd::Disconnect) => break End::Disconnect,
                None => break End::Closed,
            },
            _ = ping.tick() => {
                let text = serde_json::to_string(&ClientMessage::Ping).unwrap_or_default();
                if let Err(e) = sink.send(Message::text(text)).await {
                    break End::Lost(format!("ping: {e}"));
                }
                ping_sent = Some(Instant::now());
            }
        }
    };
    let _ = sink.close().await;
    let mut m = model.lock().unwrap();
    m.conn = ConnState::Disconnected;
    m.latency_ms = None;
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    /// A session against a local addin stand-in: it gets `hello` and `plugins`, answers the
    /// first `ping` with `pong`, and forwards a `set` the UI sends.
    #[tokio::test]
    async fn session_talks_to_a_fake_addin() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut tx, mut rx) = ws.split();
            tx.send(Message::text(
                r#"{"t":"hello","protocol":1,"addin":"0.0.1","device":{"model":"m","mpc":"3"},"poll_ms":20,"text_ms":200}"#,
            ))
            .await
            .unwrap();
            tx.send(Message::text(
                r#"{"t":"plugins","plugins":[{"id":7,"name":"X","uid":"00000007","skin":false,"params":[{"i":0,"name":"A","value":0.5,"text":"50"}]}]}"#,
            ))
            .await
            .unwrap();
            let mut got_set = None;
            while let Some(Ok(m)) = rx.next().await {
                if let Message::Text(t) = m {
                    let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                    match v["t"].as_str() {
                        Some("ping") => tx.send(Message::text(r#"{"t":"pong"}"#)).await.unwrap(),
                        Some("set") => {
                            got_set = Some(v);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            let _ = tx.send(Message::Close(None)).await;
            got_set
        });
        let model = Arc::new(Mutex::new(Model::new()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let m2 = model.clone();
        let tx2 = tx.clone();
        tokio::spawn(async move {
            // Wait for the pong to be measured, then send a set.
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if m2.lock().unwrap().latency_ms.is_some() {
                    break;
                }
            }
            tx2.send(Cmd::Send(ClientMessage::Set {
                id: 7,
                i: 0,
                value: 0.25,
            }))
            .unwrap();
        });
        let end = tokio::time::timeout(
            Duration::from_secs(10),
            session(&model, &mut rx, "127.0.0.1", port, 1),
        )
        .await
        .expect("session ended");
        assert!(matches!(end, End::Lost(_)));
        let got = server.await.unwrap().expect("the addin saw a set");
        assert_eq!(got["id"], 7);
        assert_eq!(got["value"], 0.25);
        let m = model.lock().unwrap();
        assert_eq!(m.hello.as_ref().unwrap().addin, "0.0.1");
        assert_eq!(m.instances.len(), 1);
        assert!(m.conn.is_idle());
        assert!(m.log.iter().any(|l| l.contains("connected to ws://")));
    }

    #[tokio::test]
    async fn connect_failure_is_lost() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let model = Arc::new(Mutex::new(Model::new()));
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        match session(&model, &mut rx, "127.0.0.1", port, 1).await {
            End::Lost(why) => assert!(why.starts_with("connect:"), "{why}"),
            _ => panic!("expected Lost"),
        }
    }
}
