
# WPP Server

# Requirements for building from source
- Rust 1.85^
- g++ >= 8 or clang (with C++ 17 support)
- cc and c++ symlinks pointing to gcc/g++ or clang/clang++

# Cloudflare

By default this project relies on Cloudflare TURN Server. If you don't want to use Cloudflare TURN define the ICE_SERVERS env variable
and compile with the flag `--no-default-feautres`.

# Env

Create .env file

```shell
MONGODB_URL=
SECRET_KEY=
PORT= # http/ws port
REDIS_URL=

# Cloudflare
CF_TOKEN=
CF_ID=
# no Cloudflare
ICE_SERVERS={\"iceServers\":[{\"urls\":\"stun:stun.l.google.com:19302\"}]}

# binding ip (use 0.0.0.0 inside Docker)
HOST=127.0.0.1
# announce ip
HOST_URL=127.0.0.1
# UDP port range
PORT_MIN=40000
PORT_MAX=40100
```

# Building

`cargo run`

or

`cargo run --release`

No Cloudflare TURN

`cargo run --no-default-features`

or

`cargo run --release --no-default-features`

# Docker

```shell
docker build -f Dockerfile -t wpp-server .
docker run --env-file .env -d wpp-server
```
