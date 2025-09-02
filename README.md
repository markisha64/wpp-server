
# WPP Server

# Requirements for building from source
- Rust 1.85^
- g++ >= 8 or clang (with C++ 17 support)
- cc and c++ symlinks pointing to gcc/g++ or clang/clang++

# Cloudflare

The project relies on Cloudflare TURN Server, if you want to run it you should sign up and enable the service (1000GB/month free);

# Env

Create .env file

```shell
MONGODB_URL=
SECRET_KEY=
PORT= # http/ws port
REDIS_URL=
CF_TOKEN=
CF_ID=
# binding ip (use 0.0.0.0 inside Docker)
HOST=127.0.0.1
# announce ip
HOST_URL=127.0.0.1
# UDP port range
PORT_MIN=40000
PORT_MAX=40100
```

# Building

`cargon run`

or

`cargo run --release`

# Docker

```shell
docker build -f Dockerfile -t wpp-server .
docker run --env-file .env -d wpp-server
```
