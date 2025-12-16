mod command;

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::command::Command;

pub(crate) type Db = Arc<Mutex<HashMap<String, String>>>;

#[derive(Debug)]
pub(crate) enum RespValue {
    SimpleString(String),
    BulkString(Option<Vec<u8>>),
    Array(Option<Vec<RespValue>>),
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RespError {
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from `.env` if present.
    let _ = dotenvy::dotenv();

    let addr = env::var("REDIS_RS_ADDR").unwrap_or_else(|_| "127.0.0.1:6379".to_string());
    let listener = TcpListener::bind(addr).await?;
    println!("Redis-like server listening on {}", addr);

    let db: Db = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (socket, peer) = listener.accept().await?;
        println!("Accepted connection from {}", peer);
        let db = Arc::clone(&db);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, db).await {
                eprintln!("connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, db: Db) -> Result<(), RespError> {
    let mut buf = BytesMut::with_capacity(4096);

    loop {
        let mut temp = [0u8; 1024];
        let n = socket.read(&mut temp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&temp[..n]);

        while let Some(frame) = try_parse_resp(&mut buf)? {
            let cmd = match Command::from_resp(frame) {
                Ok(c) => c,
                Err(e) => {
                    let resp = RespValue::Error(format!("ERR {}", e));
                    let encoded = encode_resp(&resp);
                    socket.write_all(&encoded).await?;
                    continue;
                }
            };

            let response = cmd.execute(&db);
            let encoded = encode_resp(&response);
            socket.write_all(&encoded).await?;
        }
    }
}

fn try_parse_resp(buf: &mut BytesMut) -> Result<Option<RespValue>, RespError> {
    if buf.is_empty() {
        return Ok(None);
    }

    let mut slice = &buf[..];
    match slice.get_u8() as char {
        '*' => parse_array(buf),
        '$' => parse_bulk_string(buf).map(|o| o.map(RespValue::BulkString)),
        '+' => parse_simple_string(buf).map(|o| o.map(RespValue::SimpleString)),
        '-' => parse_error(buf).map(|o| o.map(RespValue::Error)),
        other => Err(RespError::Protocol(format!(
            "unexpected type byte: {}",
            other
        ))),
    }
}

fn parse_line(buf: &mut BytesMut) -> Option<Vec<u8>> {
    if let Some(pos) = buf.windows(2).position(|w| w == b"\r\n") {
        let line = buf.split_to(pos);
        buf.advance(2); // skip \r\n
        Some(line.to_vec())
    } else {
        None
    }
}

fn parse_simple_string(buf: &mut BytesMut) -> Result<Option<String>, RespError> {
    if buf.first().map(|b| *b as char) != Some('+') {
        return Err(RespError::Protocol("expected simple string".into()));
    }
    buf.advance(1);
    Ok(parse_line(buf).map(|bytes| {
        String::from_utf8_lossy(&bytes).to_string()
    }))
}

fn parse_error(buf: &mut BytesMut) -> Result<Option<String>, RespError> {
    if buf.first().map(|b| *b as char) != Some('-') {
        return Err(RespError::Protocol("expected error".into()));
    }
    buf.advance(1);
    Ok(parse_line(buf).map(|bytes| {
        String::from_utf8_lossy(&bytes).to_string()
    }))
}

fn parse_bulk_string(buf: &mut BytesMut) -> Result<Option<Option<Vec<u8>>>, RespError> {
    if buf.first().map(|b| *b as char) != Some('$') {
        return Err(RespError::Protocol("expected bulk string".into()));
    }
    buf.advance(1);
    let len_line = match parse_line(buf) {
        Some(l) => l,
        None => return Ok(None),
    };
    let len_str = String::from_utf8_lossy(&len_line);
    let len: isize = len_str
        .parse()
        .map_err(|_| RespError::Protocol("invalid bulk length".into()))?;

    if len == -1 {
        return Ok(Some(None));
    }

    let len = len as usize;
    if buf.len() < len + 2 {
        // not enough data yet
        // restore state by re-prepending?
        // For simplicity in this example, assume frames come whole.
        return Ok(None);
    }
    let data = buf.split_to(len).to_vec();
    // skip trailing \r\n
    buf.advance(2);
    Ok(Some(Some(data)))
}

fn parse_array(buf: &mut BytesMut) -> Result<Option<RespValue>, RespError> {
    if buf.first().map(|b| *b as char) != Some('*') {
        return Err(RespError::Protocol("expected array".into()));
    }
    buf.advance(1);
    let len_line = match parse_line(buf) {
        Some(l) => l,
        None => return Ok(None),
    };
    let len_str = String::from_utf8_lossy(&len_line);
    let len: isize = len_str
        .parse()
        .map_err(|_| RespError::Protocol("invalid array length".into()))?;

    if len == -1 {
        return Ok(Some(RespValue::Array(None)));
    }

    let len = len as usize;
    let mut items = Vec::with_capacity(len);
    for _ in 0..len {
        // For brevity, we only support bulk strings in arrays (typical for commands).
        if buf.first().map(|b| *b as char) != Some('$') {
            return Err(RespError::Protocol(
                "only bulk strings supported in arrays for now".into(),
            ));
        }
        match parse_bulk_string(buf)? {
            Some(Some(b)) => items.push(RespValue::BulkString(Some(b))),
            Some(None) => items.push(RespValue::BulkString(None)),
            None => return Ok(None),
        }
    }
    Ok(Some(RespValue::Array(Some(items))))
}

fn encode_resp(val: &RespValue) -> Vec<u8> {
    match val {
        RespValue::SimpleString(s) => format!("+{}\r\n", s).into_bytes(),
        RespValue::Error(e) => format!("-{}\r\n", e).into_bytes(),
        RespValue::BulkString(Some(b)) => {
            let mut out = format!("${}\r\n", b.len()).into_bytes();
            out.extend_from_slice(b);
            out.extend_from_slice(b"\r\n");
            out
        }
        RespValue::BulkString(None) => b"$-1\r\n".to_vec(),
        RespValue::Array(Some(items)) => {
            let mut out = format!("*{}\r\n", items.len()).into_bytes();
            for item in items {
                out.extend_from_slice(&encode_resp(item));
            }
            out
        }
        RespValue::Array(None) => b"*-1\r\n".to_vec(),
    }
}


