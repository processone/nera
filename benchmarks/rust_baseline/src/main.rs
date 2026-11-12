// Minimal in-memory memcached-text server for benchmarking Nera / BEAM
// onecached against a native Rust baseline on the same workload.
//
// Supports the subset memtier_benchmark uses: get / set / delete, plus
// version, quit, and stats (stubbed). Keys and values are bytes.
// Storage is a Mutex<HashMap> — simple and fast enough for the comparison.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

type Store = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(11212);
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("rust_mcached_baseline listening on {}", addr);

    let store: Store = Arc::new(Mutex::new(HashMap::with_capacity(100_000)));

    loop {
        let (socket, _) = listener.accept().await?;
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            if let Err(e) = handle_conn(socket, store).await {
                let _ = e;
            }
        });
    }
}

async fn handle_conn(socket: TcpStream, store: Store) -> std::io::Result<()> {
    socket.set_nodelay(true)?;
    let (rd, mut wr) = socket.into_split();
    let mut reader = BufReader::with_capacity(16 * 1024, rd);
    let mut line = Vec::with_capacity(256);

    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line).await?;
        if n == 0 {
            return Ok(());
        }
        // strip trailing \r\n or \n
        while let Some(&b) = line.last() {
            if b == b'\n' || b == b'\r' {
                line.pop();
            } else {
                break;
            }
        }

        let mut parts = line.split(|&b| b == b' ');
        let cmd = match parts.next() {
            Some(c) => c,
            None => continue,
        };

        match cmd {
            b"set" => {
                // set <key> <flags> <exptime> <bytes> [noreply]\r\n<data>\r\n
                let key = match parts.next() {
                    Some(k) => k.to_vec(),
                    None => {
                        wr.write_all(b"ERROR\r\n").await?;
                        continue;
                    }
                };
                let _flags = parts.next();
                let _exptime = parts.next();
                let bytes: usize = match parts
                    .next()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .and_then(|s| s.parse().ok())
                {
                    Some(n) => n,
                    None => {
                        wr.write_all(b"ERROR\r\n").await?;
                        continue;
                    }
                };
                let noreply = parts.next().map(|b| b == b"noreply").unwrap_or(false);

                let mut value = vec![0u8; bytes];
                reader.read_exact(&mut value).await?;
                // consume trailing \r\n
                let mut crlf = [0u8; 2];
                reader.read_exact(&mut crlf).await?;

                {
                    let mut s = store.lock().await;
                    s.insert(key, value);
                }
                if !noreply {
                    wr.write_all(b"STORED\r\n").await?;
                }
            }
            b"get" | b"gets" => {
                // get <key>+\r\n — respond with VALUE lines then END
                let mut out: Vec<u8> = Vec::with_capacity(512);
                {
                    let s = store.lock().await;
                    for key in parts {
                        if let Some(value) = s.get(key) {
                            out.extend_from_slice(b"VALUE ");
                            out.extend_from_slice(key);
                            out.extend_from_slice(b" 0 ");
                            out.extend_from_slice(value.len().to_string().as_bytes());
                            out.extend_from_slice(b"\r\n");
                            out.extend_from_slice(value);
                            out.extend_from_slice(b"\r\n");
                        }
                    }
                }
                out.extend_from_slice(b"END\r\n");
                wr.write_all(&out).await?;
            }
            b"delete" => {
                let key = match parts.next() {
                    Some(k) => k,
                    None => {
                        wr.write_all(b"ERROR\r\n").await?;
                        continue;
                    }
                };
                let noreply = parts.next().map(|b| b == b"noreply").unwrap_or(false);
                let existed = {
                    let mut s = store.lock().await;
                    s.remove(key).is_some()
                };
                if !noreply {
                    wr.write_all(if existed {
                        b"DELETED\r\n"
                    } else {
                        b"NOT_FOUND\r\n"
                    })
                    .await?;
                }
            }
            b"version" => {
                wr.write_all(b"VERSION rust_mcached_baseline 0.1\r\n").await?;
            }
            b"stats" => {
                wr.write_all(b"END\r\n").await?;
            }
            b"flush_all" => {
                store.lock().await.clear();
                wr.write_all(b"OK\r\n").await?;
            }
            b"quit" => return Ok(()),
            _ => {
                wr.write_all(b"ERROR\r\n").await?;
            }
        }
    }
}
