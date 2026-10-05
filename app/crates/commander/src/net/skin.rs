//! Fetches a plugin's skin files from the addin into `~/.cache/mpc-commander/<uid>/`, keyed by
//! ETag, and decodes the PNGs for the panel. A plugin's skin comes from `/skin/<id>/`, a stock
//! plugin's from `/stock/<folder>/` (Akai's files: cached on this computer, never shipped).

use super::http;
use crate::model::{Image, Shared, SkinBundle, SkinState, SkinWant};
use anyhow::{anyhow, bail, Context};
use commander_skin::Skin;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Files fetched at once per skin.
const PARALLEL: usize = 4;
/// HTTP requests in flight across every skin: the addin serves `max_clients` connections (6 by
/// default) and the WebSocket holds one, so a project's worth of stock skins at once must queue.
static IN_FLIGHT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);
/// Tries for a file the addin answers 503 (busy) for.
const BUSY_TRIES: u32 = 5;

pub async fn fetch(model: Shared, host: String, port: u16, want: SkinWant) {
    let SkinWant { prefix, uid } = want;
    let dir = crate::config::cache_dir().map(|d| d.join(&uid));
    match fetch_bundle(&model, &host, port, &prefix, &uid, dir.as_deref()).await {
        Ok(bundle) => {
            let mut m = model.lock().unwrap();
            m.log(format!(
                "skin {uid}: {} page(s), {} image(s)",
                bundle.skin.pages().len(),
                bundle.images.len()
            ));
            m.skins.insert(uid, SkinState::Ready(Arc::new(bundle)));
        }
        Err(e) => {
            let mut m = model.lock().unwrap();
            m.log(format!("skin {uid}: {e:#}"));
            m.skins.insert(uid, SkinState::Failed(format!("{e:#}")));
        }
    }
}

async fn fetch_bundle(
    model: &Shared,
    host: &str,
    port: u16,
    prefix: &str,
    uid: &str,
    dir: Option<&Path>,
) -> anyhow::Result<SkinBundle> {
    let tui = fetch_cached(host, port, prefix, "TUI.json", dir).await?;
    let skin = Skin::parse(std::str::from_utf8(&tui).context("TUI.json is not UTF-8")?)?;
    let files: Vec<String> = skin.image_files().into_iter().collect();
    let total = files.len();
    let progress = |done: usize| {
        let mut m = model.lock().unwrap();
        if let Some(SkinState::Loading { .. }) = m.skins.get(uid) {
            m.skins
                .insert(uid.to_string(), SkinState::Loading { done, total });
        }
    };
    progress(0);
    let mut images = HashMap::new();
    let mut done = 0usize;
    for batch in files.chunks(PARALLEL) {
        let mut tasks = Vec::new();
        for file in batch {
            let file = file.clone();
            let dir = dir.map(Path::to_path_buf);
            let host = host.to_string();
            let prefix = prefix.to_string();
            tasks.push(tokio::spawn(async move {
                let bytes = fetch_cached(&host, port, &prefix, &file, dir.as_deref()).await?;
                let image = tokio::task::spawn_blocking(move || decode_png(&bytes))
                    .await
                    .map_err(|e| anyhow!("decode task: {e}"))??;
                Ok::<_, anyhow::Error>((file, image))
            }));
        }
        for (t, file) in tasks.into_iter().zip(batch) {
            // One image the device doesn't have draws as a placeholder; the rest of the skin still loads.
            match t.await.map_err(|e| anyhow!("fetch task: {e}"))? {
                Ok((file, image)) => {
                    images.insert(file, Arc::new(image));
                }
                Err(e) => model
                    .lock()
                    .unwrap()
                    .log(format!("skin {uid}: {file}: {e:#}")),
            }
            done += 1;
            progress(done);
        }
    }
    Ok(SkinBundle { skin, images })
}

/// A skin file from the addin, or the cached copy when the addin says it is unchanged (304)
/// or cannot be reached.
async fn fetch_cached(
    host: &str,
    port: u16,
    prefix: &str,
    file: &str,
    dir: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    if file.contains("..") || file.starts_with('/') {
        bail!("{file}: refusing a path outside the skin folder");
    }
    let cached = dir.map(|d| d.join(file));
    let etag_path: Option<PathBuf> = cached.as_ref().map(|p| {
        let mut s = p.as_os_str().to_owned();
        s.push(".etag");
        PathBuf::from(s)
    });
    let etag = match (&cached, &etag_path) {
        (Some(c), Some(e)) if c.is_file() => std::fs::read_to_string(e).ok(),
        _ => None,
    };
    let etag = etag.map(|e| e.trim().to_string()).filter(|e| !e.is_empty());
    let path = format!("{prefix}/{file}");
    let mut tries = 0;
    let reply = loop {
        let r = {
            let _permit = IN_FLIGHT.acquire().await?;
            http::get(host, port, &path, etag.as_deref()).await
        };
        tries += 1;
        match r {
            Ok(r) if r.status == 503 && tries < BUSY_TRIES => {
                tokio::time::sleep(std::time::Duration::from_millis(200 * tries as u64)).await;
            }
            r => break r,
        }
    };
    match reply {
        Ok(r) if r.status == 304 => {
            let c = cached.ok_or_else(|| anyhow!("{file}: 304 without a cache"))?;
            std::fs::read(&c).with_context(|| format!("{}", c.display()))
        }
        Ok(r) if r.status == 200 => {
            if let (Some(c), Some(e)) = (&cached, &etag_path) {
                if let Err(err) = store(c, e, &r) {
                    log::warn!("cache {}: {err:#}", c.display());
                }
            }
            Ok(r.body)
        }
        Ok(r) => match cached.filter(|c| c.is_file()) {
            Some(c) => {
                log::warn!("{path}: HTTP {}; using the cached copy", r.status);
                Ok(std::fs::read(c)?)
            }
            None => bail!("{path}: HTTP {}", r.status),
        },
        Err(e) => match cached.filter(|c| c.is_file()) {
            Some(c) => {
                log::warn!("{path}: {e:#}; using the cached copy");
                Ok(std::fs::read(c)?)
            }
            None => Err(e),
        },
    }
}

fn store(cached: &Path, etag_path: &Path, r: &http::Response) -> anyhow::Result<()> {
    if let Some(parent) = cached.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = cached.with_extension("new");
    std::fs::write(&tmp, &r.body)?;
    std::fs::rename(&tmp, cached)?;
    match r.header("etag") {
        Some(etag) => std::fs::write(etag_path, etag)?,
        None => {
            let _ = std::fs::remove_file(etag_path);
        }
    }
    Ok(())
}

/// Decodes a PNG to straight (non-premultiplied) RGBA.
pub fn decode_png(bytes: &[u8]) -> anyhow::Result<Image> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8();
    Ok(Image {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_png() {
        let mut png = Vec::new();
        {
            let img =
                image::RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
        }
        let d = decode_png(&png).unwrap();
        assert_eq!((d.width, d.height), (3, 2));
        assert_eq!(&d.rgba[4..8], &[1, 0, 7, 255]);
        assert!(decode_png(b"nope").is_err());
    }

    /// A one-request HTTP server: replies with `reply` and records the request head.
    async fn serve_once(reply: &'static str) -> (u16, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let mut head = Vec::new();
            loop {
                let n = s.read(&mut buf).await.unwrap();
                head.extend_from_slice(&buf[..n]);
                if n == 0 || head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            s.write_all(reply.as_bytes()).await.unwrap();
            s.shutdown().await.unwrap();
            String::from_utf8_lossy(&head).into_owned()
        });
        (port, task)
    }

    #[tokio::test]
    async fn fetch_caches_by_etag_and_falls_back_to_the_cache() {
        let dir = std::env::temp_dir().join(format!(
            "commander-skin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 200: stored with its ETag.
        let (port, req) =
            serve_once("HTTP/1.1 200 OK\r\nETag: \"42-7\"\r\nConnection: close\r\n\r\nbody-one")
                .await;
        let got = fetch_cached("127.0.0.1", port, "/skin/3", "a b.png", Some(&dir))
            .await
            .unwrap();
        assert_eq!(got, b"body-one");
        let head = req.await.unwrap();
        assert!(
            head.starts_with("GET /skin/3/a%20b.png HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(!head.contains("If-None-Match"));
        assert_eq!(std::fs::read(dir.join("a b.png")).unwrap(), b"body-one");
        // 304: the request carried the ETag and the cached body is returned.
        let (port, req) =
            serve_once("HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n").await;
        let got = fetch_cached("127.0.0.1", port, "/skin/3", "a b.png", Some(&dir))
            .await
            .unwrap();
        assert_eq!(got, b"body-one");
        assert!(req.await.unwrap().contains("If-None-Match: \"42-7\"\r\n"));
        // Unreachable: the cached body is still returned; an uncached file is an error.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = listener.local_addr().unwrap().port();
        drop(listener);
        let got = fetch_cached("127.0.0.1", dead, "/skin/3", "a b.png", Some(&dir))
            .await
            .unwrap();
        assert_eq!(got, b"body-one");
        assert!(
            fetch_cached("127.0.0.1", dead, "/skin/3", "other.png", Some(&dir))
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refuses_paths_outside_the_skin() {
        let e = fetch_cached("localhost", 1, "/skin/1", "../x.png", None)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("refusing"));
    }
}
