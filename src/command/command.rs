use crate::{Db, RespError, RespValue};

/// High-level representation of supported commands.
#[derive(Debug)]
pub enum Command {
    Ping,
    Get(String),
    Set { key: String, value: String },
}

impl Command {
    /// Deserialize a RESP value (typically an array) into a high-level `Command`.
    pub fn from_resp(frame: RespValue) -> Result<Self, RespError> {
        let items = match frame {
            RespValue::Array(Some(items)) if !items.is_empty() => items,
            _ => {
                return Err(RespError::Protocol(
                    "expected array frame for command".into(),
                ))
            }
        };

        let cmd_name = match &items[0] {
            RespValue::BulkString(Some(b)) => String::from_utf8_lossy(b).to_ascii_uppercase(),
            _ => {
                return Err(RespError::Protocol(
                    "first array element must be bulk string command name".into(),
                ))
            }
        };

        match cmd_name.as_str() {
            "PING" => Ok(Command::Ping),
            "GET" => Self::from_get(&items[1..]),
            "SET" => Self::from_set(&items[1..]),
            other => Err(RespError::Protocol(format!(
                "unknown command '{}'",
                other
            ))),
        }
    }

    fn from_get(args: &[RespValue]) -> Result<Self, RespError> {
        if args.is_empty() {
            return Err(RespError::Protocol(
                "wrong number of arguments for 'GET'".into(),
            ));
        }
        let key = match &args[0] {
            RespValue::BulkString(Some(b)) => String::from_utf8_lossy(b).to_string(),
            _ => {
                return Err(RespError::Protocol(
                    "invalid key type for 'GET' (expected bulk string)".into(),
                ))
            }
        };
        Ok(Command::Get(key))
    }

    fn from_set(args: &[RespValue]) -> Result<Self, RespError> {
        if args.len() < 2 {
            return Err(RespError::Protocol(
                "wrong number of arguments for 'SET'".into(),
            ));
        }
        let key = match &args[0] {
            RespValue::BulkString(Some(b)) => String::from_utf8_lossy(b).to_string(),
            _ => {
                return Err(RespError::Protocol(
                    "invalid key type for 'SET' (expected bulk string)".into(),
                ))
            }
        };
        let value = match &args[1] {
            RespValue::BulkString(Some(b)) => String::from_utf8_lossy(b).to_string(),
            _ => {
                return Err(RespError::Protocol(
                    "invalid value type for 'SET' (expected bulk string)".into(),
                ))
            }
        };
        Ok(Command::Set { key, value })
    }

    /// Execute the command against the in-memory DB and serialize as RESP.
    pub fn execute(self, db: &Db) -> RespValue {
        match self {
            Command::Ping => RespValue::SimpleString("PONG".into()),
            Command::Set { key, value } => {
                let mut guard = db.lock().unwrap();
                guard.insert(key, value);
                RespValue::SimpleString("OK".into())
            }
            Command::Get(key) => {
                let guard = db.lock().unwrap();
                match guard.get(&key) {
                    Some(v) => RespValue::BulkString(Some(v.clone().into_bytes())),
                    None => RespValue::BulkString(None),
                }
            }
        }
    }
}


