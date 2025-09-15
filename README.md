
# WPP Server

WPPServer poslužiteljski je dio fullstack aplikacije za real-time komunikaciju
pomoću [WebSocketa](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API)
(poruke i signalizacija) i [WebRTC](https://webrtc.org/) (video pozivi).
Izgrađen je pomoću [Actix Web](https://actix.rs/) frameworka za izradu Rest API-ja.
Server sluša na definiranom portu za HTTP requestove te na ruti /ws/ dopušta
[UPGRADE](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Upgrade)
na WebSocket konekciju. Serveru su također potrebni UDP portovi za
[Mediasoup](https://mediasoup.org/) konekcije (audio/video). 

## Build Requirements

Prije no što možeš build-ati cijeli projekt, moraš instalirati neke alate.
Treba ti [Rust](https://www.rust-lang.org/) (verzija 1.85 ili veća),
[g++](https://gcc.gnu.org/) (verzija 8 ili veća) ili [clang](https://clang.llvm.org/)
(sa podrškom za C++ 17) te cc i c++ moraju biti sym-linkani te pokazivati na gcc/g++ ili
clang/clang++.

## Run Requirements

Poslužitelj ovisi o nekim drugim alatima.

### Cloudflare

Zbog jednostavnosti, pri developmentu poslužitelja, oslonio sam se na
[Cloudflare TURN Server](https://developers.cloudflare.com/realtime/turn/).
Ako ne želite ovisiti o Cloudflare-u, dovoljno je postaviti `ICE_SERVERS`
env varijablu te compile-ati sa flag-om `--no-default-features`. Više informacija
o [TURN-u](https://en.wikipedia.org/wiki/Traversal_Using_Relay_NAT)
i [STUN-u](https://en.wikipedia.org/wiki/STUN). 

### Redis

Za potrebe sinkronizacija WebSocket konkecija između Actix radnika, koristimo
[Redis](https://redis.io/). Redis je in-memory key-value baza podataka,
najčešće korištena kao ditribuirani cache ili broker poruke. Poslužitelj koristi
Redis-ov [Pub/Sub](https://redis.io/docs/latest/develop/pubsub/) mehanizam za
dostavljanje poruka između raznih Actix radnika ili više instanci servera.
Definiramo poslužitelju Redis pomoću `REDIS_URL` env varijable.

### MongoDB

Zbog jednostavnosti i brzine razvoja, odabran je [MongoDB](https://www.mongodb.com/)
kao glavna baza podataka. MongoDB je dokument orijentirana baza podataka klasificirana
kao NoSQL. Definiramo poslužitelju MongoDB pomoću `MONGODB_URL` env varijable.

# Env

Za pokretanje poslužitalja potrebno nam je mnoštvo env varijabli. Možemo ih koristiti
na standardni način ili definirati .env datoteku.

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

Osnovna komanda za pokretanje izvornog koda je

```shell
cargo run
```

Ako želimo compile-ati sa svim optimizacijama koristimo flag `--release`

```shell
cargo run --release
```

Isto vrijedi za flag `--no-default-features`.
Također možemo umjesto direktnog pokretanja samo buildati binary.

```shell
cargo build --release
```

Novo izgraženi binary nalazi se u `./target/<debug|release>/wpp-server(.exe)`

# Docker

Za standardiziranije pokretanje (npr. ako želimo koristiti neki VPS ili Google Cloud run)
možemo koristiti [Docker](https://www.docker.com/).

```shell
docker build -f Dockerfile -t wpp-server .
docker run --env-file .env -d wpp-server
```
