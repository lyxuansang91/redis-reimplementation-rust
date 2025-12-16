Redis reimplementation in Rust (minimal).

### Features

- TCP server speaking RESP over `127.0.0.1:6379`
- In-memory key-value store
- Supports `PING`, `SET`, `GET` commands
- Async IO multiplexing via `tokio`

### Configuration

Configuration is read from environment variables (optionally via a `.env` file using `dotenvy`):

- `REDIS_RS_ADDR` – address the server binds to.  
  - **Default**: `127.0.0.1:6379`

You can create a `.env` file based on `.env.example`:

```bash
cp .env.example .env
echo 'REDIS_RS_ADDR=0.0.0.0:6379' >> .env
```

### Running

Build and run:

```bash
cargo run
```

In another terminal, you can use `redis-cli` (matching the port you configured):

```bash
redis-cli -p 6379
PING
SET foo bar
GET foo
```
