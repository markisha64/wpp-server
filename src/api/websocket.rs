use std::{
    collections::{hash_map::Entry, HashMap},
    env, io,
    net::{IpAddr, Ipv4Addr},
    num::{NonZeroU32, NonZeroU8},
    ops::RangeInclusive,
    pin::pin,
    str::FromStr,
    time::{Duration, Instant},
};

use actix_web::{error::ErrorInternalServerError, web, HttpRequest, Responder};
use actix_ws::{self, AggregatedMessage};
use anyhow::{anyhow, Context};
use futures_util::{
    future::{select, Either},
    StreamExt as _,
};
use mediasoup::{
    prelude::*,
    worker::{WorkerLogLevel, WorkerLogTag},
};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use shared::{
    api::{
        user::Claims,
        websocket::{
            MediaSoupMessage::{
                self, ConnectConsumerTransport, ConnectProducerTransport, Consume, ConsumerResume,
                FinishInit, LeaveRoom, Produce, SetRoom,
            },
            MediaSoupResponse, TransportOptions, WebsocketClientMessage,
            WebsocketClientMessageData, WebsocketServerMessage, WebsocketServerResData,
        },
    },
    models::user::UserSafe,
};
use uuid::Uuid;

use crate::{
    announcer::Announcer,
    mongodb::MongoDatabase,
    redis::{RedisHandle, RedisHandler},
};
use tokio::{
    sync::{
        mpsc::{self},
        oneshot,
    },
    time::interval,
};

use super::{
    chat::{self},
    message,
    user::{self, get_single},
};

type ConnId = Uuid;

pub struct Transports {
    consumer: WebRtcTransport,
    producer: WebRtcTransport,
}

pub struct ParticipantConnection {
    pub _user_id: String,
    pub room_id: String,
    pub client_rtp_capabilities: Option<RtpCapabilities>,
    pub consumers: HashMap<ConsumerId, Consumer>,
    pub producers: Vec<Producer>,
    pub transports: Transports,
}

pub struct Room {
    router: Router,
    clients: HashMap<String, Vec<Producer>>,
}

impl Room {
    fn producers(&self) -> Vec<(String, ProducerId)> {
        self.clients
            .iter()
            .flat_map(|(user_id, producers)| producers.iter().map(|x| (user_id.clone(), x.id())))
            .collect()
    }
}

enum Command {
    Connect {
        user_id: String,
        conn_tx: mpsc::UnboundedSender<WebsocketServerMessage>,
        res_tx: oneshot::Sender<ConnId>,
    },

    Disconnect {
        user_id: String,
        conn: ConnId,
    },

    Message {
        msg: WebsocketServerMessage,
        user_id: String,
        res_tx: oneshot::Sender<()>,
    },

    JoinRoom {
        user_id: String,
        room_id: String,
        res_tx: oneshot::Sender<
            anyhow::Result<(
                ParticipantConnection,
                RtpCapabilitiesFinalized,
                Vec<(String, ProducerId)>,
            )>,
        >,
    },

    ProducerAdded {
        user_id: String,
        room_id: String,
        producer: Producer,
        res_tx: oneshot::Sender<anyhow::Result<()>>,
    },

    RemoveParticipant {
        user_id: String,
        room_id: String,
        res_tx: oneshot::Sender<anyhow::Result<()>>,
    },

    GetRouter {
        chat_id: ObjectId,
        res_tx: oneshot::Sender<anyhow::Result<(Router, Vec<(String, ProducerId)>)>>,
    },
}

pub struct WebsocketServer {
    connections: HashMap<String, HashMap<Uuid, mpsc::UnboundedSender<WebsocketServerMessage>>>,
    rooms: HashMap<String, Room>,

    cmd_rx: mpsc::UnboundedReceiver<Command>,

    worker_manger: web::Data<WorkerManager>,

    #[allow(dead_code)]
    db: web::Data<MongoDatabase>,
    announcer: web::Data<Announcer>,
    redis: web::Data<RedisHandle>,

    server_port: String,

    port_min: u16,
    port_max: u16,

    plain_sync_port_min: u16,
    plain_sync_port_max: u16,
}

impl WebsocketServer {
    pub fn new(
        db: web::Data<MongoDatabase>,
        worker_manager: web::Data<WorkerManager>,
        announcer: web::Data<Announcer>,
    ) -> anyhow::Result<(
        Self,
        web::Data<WebsocketServerHandle>,
        RedisHandler,
        web::Data<RedisHandle>,
    )> {
        let port_min = env::var("PORT_MIN")?.parse()?;
        let port_max = env::var("PORT_MAX")?.parse()?;

        let plain_sync_port_min = env::var("PLAIN_SYNC_PORT_MIN")
            .unwrap_or("50000".to_string())
            .parse()?;
        let plain_sync_port_max = env::var("PLAIN_SYNC_PORT_MAX")
            .unwrap_or("51000".to_string())
            .parse()?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let handle = web::Data::new(WebsocketServerHandle {
            cmd_tx,
            db: db.clone(),
        });

        let (redis_handler, handle_d) = RedisHandler::new(handle.clone())?;

        let redis_handle = web::Data::new(handle_d);

        let handler = Self {
            connections: HashMap::new(),
            cmd_rx,
            rooms: HashMap::new(),
            worker_manger: worker_manager.clone(),
            db,
            redis: redis_handle.clone(),
            announcer: announcer.clone(),
            server_port: env::var("PORT").unwrap_or("3030".to_string()),
            port_min,
            port_max,
            plain_sync_port_min,
            plain_sync_port_max,
        };

        Ok((handler, handle, redis_handler, redis_handle))
    }

    async fn connect(
        &mut self,
        user_id: String,
        tx: mpsc::UnboundedSender<WebsocketServerMessage>,
    ) -> ConnId {
        let id = Uuid::new_v4();

        self.connections
            .entry(user_id)
            .or_insert_with(HashMap::new)
            .insert(id, tx);

        id
    }

    async fn disconnect(&mut self, user_id: String, conn_id: ConnId) {
        if let Some(conns) = self.connections.get_mut(&user_id) {
            conns.remove(&conn_id);
        }
    }

    async fn send_message(&self, user_id: String, msg: WebsocketServerMessage) {
        if let Some(conns) = self.connections.get(&user_id) {
            for (_, connection) in conns {
                let _ = connection.send(msg.clone());
            }
        }
    }

    async fn join_room(
        &mut self,
        user_id: String,
        room_id: String,
    ) -> anyhow::Result<(
        ParticipantConnection,
        RtpCapabilitiesFinalized,
        Vec<(String, ProducerId)>,
    )> {
        let entry = self.rooms.entry(room_id.clone());

        match entry {
            Entry::Vacant(v) => {
                let worker = self
                    .worker_manger
                    .create_worker({
                        let mut settings = WorkerSettings::default();

                        settings.enable_liburing = false;

                        settings.log_level = WorkerLogLevel::Debug;
                        settings.log_tags = vec![
                            WorkerLogTag::Info,
                            WorkerLogTag::Ice,
                            WorkerLogTag::Dtls,
                            WorkerLogTag::Rtp,
                            WorkerLogTag::Srtp,
                            WorkerLogTag::Rtcp,
                            WorkerLogTag::Rtx,
                            WorkerLogTag::Bwe,
                            WorkerLogTag::Score,
                            WorkerLogTag::Simulcast,
                            WorkerLogTag::Svc,
                            WorkerLogTag::Sctp,
                            WorkerLogTag::Message,
                        ];

                        settings
                    })
                    .await
                    .map_err(|error| anyhow!("Failed to create worker: {error}"))?;
                let router = worker
                    .create_router(RouterOptions::new(media_codecs()))
                    .await
                    .map_err(|error| anyhow!("Failed to create router: {error}"))?;

                let my_ip = self.announcer.current_ip();

                let servers = self.redis.get_servers(room_id.clone()).await?;

                let mut receive_transport_options = PlainTransportOptions::new(ListenInfo {
                    protocol: Protocol::Udp,
                    ip: IpAddr::V4(Ipv4Addr::from_str(
                        env::var("HOST").unwrap_or("0.0.0.0".to_string()).as_str(),
                    )?),
                    announced_address: Some(format!("{my_ip}:{}", &self.server_port)),
                    port: None,
                    port_range: Some(RangeInclusive::new(
                        self.plain_sync_port_min,
                        self.plain_sync_port_max,
                    )),
                    flags: None,
                    send_buffer_size: None,
                    recv_buffer_size: None,
                    expose_internal_ip: false,
                });

                receive_transport_options.comedia = true;

                for (ip, port) in servers {
                    let receive_transport = router
                        .create_plain_transport(receive_transport_options.clone())
                        .await?;

                    let conn = receive_transport.tuple();
                    let rtcp_conn = receive_transport
                        .rtcp_tuple()
                        .context("failed to get rtcp conn -> check mux disabled")?;

                    let client = reqwest::Client::new();
                    let res = client
                        .post(format!("http://{ip}:{port}/ws/plainSync"))
                        .json(&PlainSyncRequest {
                            chat_id: ObjectId::from_str(&room_id.as_str())?,
                            ip: IpAddr::V4(Ipv4Addr::from_str(my_ip.as_str())?),
                            port: conn.local_port(),
                            rtcp_port: rtcp_conn.local_port(),
                        })
                        .send()
                        .await?
                        .json::<PlainSyncResponse>()
                        .await?;

                    let send_transport_options = PlainTransportOptions::new(ListenInfo {
                        protocol: Protocol::Udp,
                        ip: IpAddr::V4(Ipv4Addr::from_str(
                            env::var("HOST").unwrap_or("0.0.0.0".to_string()).as_str(),
                        )?),
                        announced_address: None,
                        port: None,
                        port_range: Some(RangeInclusive::new(50000, 50000)),
                        flags: None,
                        send_buffer_size: None,
                        recv_buffer_size: None,
                        expose_internal_ip: false,
                    });

                    let send_transport = router
                        .create_plain_transport(send_transport_options)
                        .await?;

                    send_transport
                        .connect(PlainTransportRemoteParameters {
                            ip: Some(IpAddr::V4(Ipv4Addr::from_str(ip.as_str())?)),
                            port: Some(res.port),
                            rtcp_port: Some(res.rtcp_port),
                            srtp_parameters: None,
                        })
                        .await?;

                    for (participant_id, producer_id) in res.producers {
                        self.redis
                            .send_message_to_users(
                                &vec![ObjectId::from_str(user_id.as_str())?],
                                WebsocketServerMessage::ProducerAdded {
                                    participant_id,
                                    producer_id: producer_id.to_string(),
                                },
                            )
                            .await;
                    }
                }

                v.insert(Room {
                    router,
                    clients: HashMap::new(),
                });

                self.redis
                    .commit_server(my_ip, self.server_port.clone(), room_id.clone());
            }
            _ => {}
        };

        let room = self.rooms.get_mut(&room_id).unwrap();

        let transport_options =
            WebRtcTransportOptions::new(WebRtcTransportListenInfos::new(ListenInfo {
                protocol: Protocol::Udp,
                ip: IpAddr::V4(Ipv4Addr::from_str(
                    env::var("HOST").unwrap_or("0.0.0.0".to_string()).as_str(),
                )?),
                announced_address: Some(self.announcer.current_ip()),
                port: None,
                port_range: Some(RangeInclusive::new(self.port_min, self.port_max)),
                flags: None,
                send_buffer_size: None,
                recv_buffer_size: None,
                expose_internal_ip: false,
            }));

        let producer_transport = room
            .router
            .create_webrtc_transport(transport_options.clone())
            .await
            .map_err(|error| anyhow!("Failed to create producer transport: {error}"))?;

        let consumer_transport = room
            .router
            .create_webrtc_transport(transport_options)
            .await
            .map_err(|error| anyhow!("Failed to create consumer transport: {error}"))?;

        Ok((
            ParticipantConnection {
                _user_id: user_id,
                room_id,
                client_rtp_capabilities: None,
                consumers: HashMap::new(),
                producers: Vec::new(),
                transports: Transports {
                    consumer: consumer_transport,
                    producer: producer_transport,
                },
            },
            room.router.rtp_capabilities().clone(),
            room.producers(),
        ))
    }

    pub async fn producer_added(
        &mut self,
        user_id: String,
        room_id: String,
        producer: Producer,
    ) -> anyhow::Result<()> {
        let room = self.rooms.get_mut(&room_id).context("missing room")?;

        let producers = room.clients.entry(user_id.clone()).or_default();
        let producer_id = producer.id();

        producers.push(producer);

        let user_ids: Vec<_> = room
            .clients
            .keys()
            .filter(|x| *x != &user_id)
            .map(|x| ObjectId::from_str(x.as_str()))
            .collect::<Result<_, _>>()?;

        self.redis
            .send_message_to_users(
                &user_ids,
                WebsocketServerMessage::ProducerAdded {
                    participant_id: user_id.clone(),
                    producer_id: producer_id.to_string(),
                },
            )
            .await;

        Ok(())
    }

    pub async fn remove_participant(
        &mut self,
        user_id: String,
        room_id: String,
    ) -> anyhow::Result<()> {
        let room = self.rooms.get_mut(&room_id).context("missing room")?;

        let producers = room.clients.remove(&user_id);

        if let Some(producers) = producers {
            let user_ids: Vec<_> = room
                .clients
                .keys()
                .map(|x| ObjectId::from_str(x.as_str()))
                .collect::<Result<_, _>>()?;

            for producer in producers {
                self.redis
                    .send_message_to_users(
                        &user_ids,
                        WebsocketServerMessage::ProducerRemove {
                            participant_id: user_id.clone(),
                            producer_id: producer.id().to_string(),
                        },
                    )
                    .await
            }
        }

        // to trigger drop
        if room.clients.is_empty() {
            let my_ip = self.announcer.current_ip();

            self.rooms.remove(&room_id);
            self.redis
                .remove_server(my_ip, self.server_port.clone(), room_id);
        }

        Ok(())
    }

    pub async fn run(mut self) -> io::Result<()> {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                Command::Connect {
                    conn_tx,
                    res_tx,
                    user_id,
                } => {
                    let conn_id = self.connect(user_id, conn_tx).await;
                    let _ = res_tx.send(conn_id);
                }

                Command::Disconnect { user_id, conn } => {
                    self.disconnect(user_id, conn).await;
                }

                Command::Message {
                    user_id,
                    msg,
                    res_tx,
                } => {
                    self.send_message(user_id, msg).await;
                    let _ = res_tx.send(());
                }

                Command::JoinRoom {
                    user_id,
                    room_id,
                    res_tx,
                } => {
                    let connection = self.join_room(user_id, room_id).await;
                    let _ = res_tx.send(connection);
                }

                Command::ProducerAdded {
                    user_id,
                    room_id,
                    producer,
                    res_tx,
                } => {
                    let res = self.producer_added(user_id, room_id, producer).await;
                    let _ = res_tx.send(res);
                }

                Command::RemoveParticipant {
                    user_id,
                    room_id,
                    res_tx,
                } => {
                    let res = self.remove_participant(user_id, room_id).await;
                    let _ = res_tx.send(res);
                }

                Command::GetRouter { chat_id, res_tx } => {
                    let res = self
                        .rooms
                        .get(&chat_id.to_string())
                        .context("missing room")
                        .map(|x| (x.router.clone(), x.producers()));

                    let _ = res_tx.send(res);
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct WebsocketServerHandle {
    cmd_tx: mpsc::UnboundedSender<Command>,

    db: web::Data<MongoDatabase>,
}

impl WebsocketServerHandle {
    pub async fn connect(
        &self,
        user_id: String,
        conn_tx: mpsc::UnboundedSender<WebsocketServerMessage>,
    ) -> ConnId {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::Connect {
                user_id,
                conn_tx,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap()
    }

    pub fn disconnect(&self, user_id: String, conn: ConnId) {
        self.cmd_tx
            .send(Command::Disconnect { user_id, conn })
            .unwrap();
    }

    pub async fn send_message(&self, user_id: String, msg: WebsocketServerMessage) {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::Message {
                msg,
                user_id,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap();
    }

    pub async fn send_message_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        msg: WebsocketServerMessage,
    ) {
        for user_id in user_ids {
            self.send_message(user_id.to_string(), msg.clone()).await;
        }
    }

    pub async fn join_room(
        &self,
        user_id: String,
        room_id: String,
    ) -> (
        ParticipantConnection,
        RtpCapabilitiesFinalized,
        Vec<(String, ProducerId)>,
    ) {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::JoinRoom {
                user_id,
                room_id,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap().unwrap()
    }

    pub async fn producer_added(
        &self,
        user_id: String,
        room_id: String,
        producer: Producer,
    ) -> anyhow::Result<()> {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::ProducerAdded {
                user_id,
                room_id,
                producer,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap()
    }

    pub async fn remove_participant(&self, user_id: String, room_id: String) -> anyhow::Result<()> {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::RemoveParticipant {
                user_id,
                room_id,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap()
    }

    pub async fn get_router(
        &self,
        chat_id: ObjectId,
    ) -> anyhow::Result<(Router, Vec<(String, ProducerId)>)> {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx.send(Command::GetRouter { chat_id, res_tx })?;

        res_rx.await?
    }
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

fn to_request_response(
    res: anyhow::Result<WebsocketServerResData>,
    id: Uuid,
) -> WebsocketServerMessage {
    WebsocketServerMessage::RequestResponse {
        id,
        data: res.map_err(|err| format!("{}", err)),
    }
}

async fn request_handler(
    request: WebsocketClientMessage,
    ws_server: web::Data<WebsocketServerHandle>,
    redis_handle: web::Data<RedisHandle>,
    user: &mut UserSafe,
) -> WebsocketServerMessage {
    let ws_server = ws_server.clone();

    match request.data {
        WebsocketClientMessageData::CreateChat(req_data) => {
            let req_res = chat::create(ws_server.db.clone(), user, req_data)
                .await
                .map(|data| WebsocketServerResData::CreateChat(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::JoinChat(id) => {
            let req_res = chat::join(ws_server.db.clone(), user, redis_handle.clone(), id)
                .await
                .map(|data| WebsocketServerResData::JoinChat(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::GetChats => {
            let req_res = chat::get_chats(ws_server.db.clone(), user)
                .await
                .map(|data| WebsocketServerResData::GetChats(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::SetChatRead(id) => {
            let req_res = chat::set_chat_read(ws_server.db.clone(), user, redis_handle.clone(), id)
                .await
                .map(|data| WebsocketServerResData::SetChatRead(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::NewMessage(req_data) => {
            let req_res =
                message::create(ws_server.db.clone(), &user, redis_handle.clone(), req_data)
                    .await
                    .map(|data| WebsocketServerResData::NewMessage(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::GetMessages(req_data) => {
            let req_res = message::get_messages(ws_server.db.clone(), &user, req_data)
                .await
                .map(|data| WebsocketServerResData::GetMessages(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::ProfileUpdate(req_data) => {
            let req_res = crate::api::user::update(
                ws_server.db.clone(),
                &user,
                req_data,
                redis_handle.clone(),
            )
            .await
            .map(move |res| {
                *user = res.clone();

                WebsocketServerResData::ProfileUpdate(res)
            });

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::GetSelf => {
            let req_res = crate::api::user::get_single(ws_server.db.clone(), &user.id)
                .await
                .map(|data| WebsocketServerResData::GetSelf(data));

            to_request_response(req_res, request.id)
        }

        _ => to_request_response(Err(anyhow!("Unreachable")), request.id),
    }
}

async fn websocket(
    req: HttpRequest,
    body: web::Payload,
    user: web::ReqData<Claims>,
    ws_server: web::Data<WebsocketServerHandle>,
    redis_handle: web::Data<RedisHandle>,
) -> actix_web::Result<impl Responder> {
    let (res, mut session, msg_stream) = actix_ws::handle(&req, body)?;

    let mut user = get_single(ws_server.db.clone(), &user.user_id)
        .await
        .map_err(|e| ErrorInternalServerError(e))?;

    actix_web::rt::spawn(async move {
        let user_id = user.id;

        let mut participant_connection: Option<ParticipantConnection> = Option::None;

        let mut last_heartbeat = Instant::now();
        let mut interval = interval(HEARTBEAT_INTERVAL);

        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel();

        let conn_id = ws_server.connect(user_id.to_string(), conn_tx).await;

        let msg_stream_f = msg_stream
            .max_frame_size(128 * 1024)
            .aggregate_continuations()
            .max_continuation_size(2 * 1024 * 1024);

        let mut msg_stream = pin!(msg_stream_f);

        let mut ms_handler =
            async |media_soup: MediaSoupMessage| -> anyhow::Result<WebsocketServerResData> {
                match media_soup {
                    ConnectProducerTransport(dtls_parameters) => {
                        let conn = participant_connection.as_ref().context("missing p conn")?;

                        let transport = conn.transports.producer.clone();

                        transport
                            .connect(WebRtcTransportRemoteParameters { dtls_parameters })
                            .await
                            .and_then(|_| {
                                Ok(WebsocketServerResData::MS(
                                    MediaSoupResponse::ConnectProducerTransport,
                                ))
                            })
                            .map_err(|err| anyhow!(err))
                    }
                    Produce((media_kind, rtp_params)) => {
                        let conn = participant_connection.as_mut().context("missing p conn")?;

                        let transport = conn.transports.producer.clone();

                        let producer = transport
                            .produce(ProducerOptions::new(media_kind, rtp_params))
                            .await;

                        let producer = producer?;
                        let id = producer.id();

                        conn.producers.push(producer.clone());

                        ws_server
                            .producer_added(user_id.to_string(), conn.room_id.clone(), producer)
                            .await
                            .map(|_| {
                                WebsocketServerResData::MS(MediaSoupResponse::Produce(
                                    id.to_string(),
                                ))
                            })
                    }
                    ConnectConsumerTransport(dtls_parameters) => {
                        let conn = participant_connection.as_ref().context("missing p conn")?;

                        conn.transports
                            .consumer
                            .clone()
                            .connect(WebRtcTransportRemoteParameters { dtls_parameters })
                            .await
                            .map(|_| {
                                WebsocketServerResData::MS(
                                    MediaSoupResponse::ConnectConsumerTransport,
                                )
                            })
                            .map_err(|err| anyhow!(err))
                    }
                    Consume(producer_id) => {
                        let conn = participant_connection.as_mut().context("missing p conn")?;

                        let rtp_capabilities = conn.client_rtp_capabilities.clone();

                        let mut options = ConsumerOptions::new(
                            ProducerId::from_str(producer_id.as_str())?,
                            rtp_capabilities.context("missing client rtp options")?,
                        );
                        options.paused = true;

                        let consumer = conn.transports.consumer.clone().consume(options).await?;

                        let id = consumer.id();
                        let kind = consumer.kind();
                        let rtp_parameters = consumer.rtp_parameters().clone();

                        conn.consumers.insert(id, consumer);

                        Ok(WebsocketServerResData::MS(MediaSoupResponse::Consume {
                            id: id.to_string(),
                            producer_id,
                            kind,
                            rtp_parameters,
                        }))
                    }
                    ConsumerResume(consumer_id) => {
                        let conn = participant_connection.as_ref().context("missing p conn")?;

                        let consumer_id = ConsumerId::from_str(consumer_id.as_str())?;

                        // we ignore error here
                        let _ = conn
                            .consumers
                            .get(&consumer_id)
                            .context("missing consumer")?
                            .clone()
                            .resume()
                            .await;

                        Ok(WebsocketServerResData::MS(
                            MediaSoupResponse::ConsumerResume,
                        ))
                    }
                    SetRoom(chat_id) => {
                        if let Some(conn) = participant_connection.as_ref() {
                            if chat_id.to_string() != conn.room_id {
                                let _ = ws_server
                                    .remove_participant(user_id.to_string(), conn.room_id.clone())
                                    .await;
                            }
                        }

                        let (conn, router_rtp_capabilities, producers) = ws_server
                            .join_room(user_id.to_string(), chat_id.to_string())
                            .await;

                        let consumer = &conn.transports.consumer;
                        let producer = &conn.transports.producer;

                        let r = WebsocketServerResData::MS(MediaSoupResponse::SetRoom {
                            room_id: chat_id.to_string(),
                            consumer_transport_options: TransportOptions {
                                id: consumer.id().to_string(),
                                dtls_parameters: consumer.dtls_parameters(),
                                ice_candidates: consumer.ice_candidates().clone(),
                                ice_parameters: consumer.ice_parameters().clone(),
                            },
                            producer_transport_options: TransportOptions {
                                id: producer.id().to_string(),
                                dtls_parameters: producer.dtls_parameters(),
                                ice_candidates: producer.ice_candidates().clone(),
                                ice_parameters: producer.ice_parameters().clone(),
                            },
                            router_rtp_capabilities,
                            producers: producers
                                .into_iter()
                                .map(|(k, v)| (k, v.to_string()))
                                .collect(),
                        });

                        participant_connection.replace(conn);

                        Ok(r)
                    }
                    FinishInit(capabilities) => {
                        let conn = participant_connection.as_mut().context("missing p conn")?;

                        conn.client_rtp_capabilities.replace(capabilities);

                        let turn_creds = user::get_turn_creds().await?;

                        Ok(WebsocketServerResData::MS(MediaSoupResponse::FinishInit(
                            turn_creds,
                        )))
                    }
                    LeaveRoom => {
                        let conn = participant_connection.as_ref().context("missing p conn")?;

                        let _ = ws_server
                            .remove_participant(user_id.to_string(), conn.room_id.clone())
                            .await;

                        participant_connection = None;

                        Ok(WebsocketServerResData::MS(MediaSoupResponse::LeaveRoom))
                    }
                }
            };

        let close_reason = loop {
            let tick = pin!(interval.tick());
            let msg_rx = pin!(conn_rx.recv());

            let messages = pin!(select(msg_stream.next(), msg_rx));

            match select(messages, tick).await {
                // commands & messages received from client
                Either::Left((Either::Left((Some(Ok(msg)), _)), _)) => match msg {
                    AggregatedMessage::Ping(bytes) => {
                        last_heartbeat = Instant::now();

                        session.pong(&bytes).await.unwrap();
                    }

                    AggregatedMessage::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }

                    AggregatedMessage::Close(reason) => break reason,

                    AggregatedMessage::Text(payload) => {
                        last_heartbeat = Instant::now();

                        if let Ok(request) = serde_json::from_str::<WebsocketClientMessage>(
                            payload.to_string().as_str(),
                        ) {
                            match request.data {
                                WebsocketClientMessageData::MS(media_soup) => {
                                    let r = ms_handler(media_soup).await;
                                    if let Ok(string_payload) =
                                        serde_json::to_string(&to_request_response(r, request.id))
                                    {
                                        session.text(string_payload).await.unwrap();
                                    }
                                }
                                _ => {
                                    let res = request_handler(
                                        request,
                                        ws_server.clone(),
                                        redis_handle.clone(),
                                        &mut user,
                                    )
                                    .await;

                                    if let Ok(string_payload) = serde_json::to_string(&res) {
                                        session.text(string_payload).await.unwrap();
                                    }
                                }
                            }
                        }
                    }

                    AggregatedMessage::Binary(_) => {
                        last_heartbeat = Instant::now();
                    }
                },

                // ws stream error
                Either::Left((Either::Left((Some(Err(_err)), _)), _)) => {
                    break None;
                }

                // ws stream end
                Either::Left((Either::Left((None, _)), _)) => break None,

                Either::Left((Either::Right((Some(ws_msg), _)), _)) => {
                    if let Ok(notif) = serde_json::to_string(&ws_msg) {
                        let _ = session.text(notif).await;
                    }
                }

                Either::Left((Either::Right((None, _)), _)) => unreachable!(
                    "all connection message senders were dropped; ws server may have panicked"
                ),

                Either::Right((_inst, _)) => {
                    if Instant::now().duration_since(last_heartbeat) > CLIENT_TIMEOUT {
                        break None;
                    }

                    let _ = session.ping(b"").await;
                }
            }
        };

        // handle mediasoup disconnect
        if let Some(conn) = participant_connection {
            let _ = ws_server
                .remove_participant(user_id.to_string(), conn.room_id)
                .await;
        }

        ws_server.disconnect(user_id.to_string(), conn_id);

        let _ = session.close(close_reason).await;
    });

    Ok(res)
}

#[derive(Serialize, Deserialize)]
pub struct PlainSyncRequest {
    pub chat_id: ObjectId,
    pub ip: IpAddr,
    pub port: u16,
    pub rtcp_port: u16,
}

#[derive(Serialize, Deserialize)]
pub struct PlainSyncResponse {
    pub port: u16,
    pub rtcp_port: u16,
    pub producers: Vec<(String, ProducerId)>,
}

pub async fn plain_sync(
    request: web::Json<PlainSyncRequest>,
    ws_server: web::Data<WebsocketServerHandle>,
) -> actix_web::Result<web::Json<PlainSyncResponse>> {
    let (router, producers) = ws_server
        .get_router(request.chat_id)
        .await
        .map_err(ErrorInternalServerError)?;

    let plain_sync_port_min = env::var("PLAIN_SYNC_PORT_MIN")
        .unwrap_or("50000".to_string())
        .parse()
        .map_err(ErrorInternalServerError)?;
    let plain_sync_port_max = env::var("PLAIN_SYNC_PORT_MAX")
        .unwrap_or("51000".to_string())
        .parse()
        .map_err(ErrorInternalServerError)?;

    let send_transport_options = PlainTransportOptions::new(ListenInfo {
        protocol: Protocol::Udp,
        ip: IpAddr::V4(
            Ipv4Addr::from_str(env::var("HOST").unwrap_or("0.0.0.0".to_string()).as_str())
                .map_err(ErrorInternalServerError)?,
        ),
        announced_address: None,
        port: None,
        port_range: Some(RangeInclusive::new(
            plain_sync_port_min,
            plain_sync_port_max,
        )),
        flags: None,
        send_buffer_size: None,
        recv_buffer_size: None,
        expose_internal_ip: false,
    });

    let send_transport = router
        .create_plain_transport(send_transport_options)
        .await
        .map_err(ErrorInternalServerError)?;

    send_transport
        .connect(PlainTransportRemoteParameters {
            ip: Some(request.ip),
            port: Some(request.port),
            rtcp_port: Some(request.rtcp_port),
            srtp_parameters: None,
        })
        .await
        .map_err(ErrorInternalServerError)?;

    let mut receive_transport_options = PlainTransportOptions::new(ListenInfo {
        protocol: Protocol::Udp,
        ip: IpAddr::V4(
            Ipv4Addr::from_str(env::var("HOST").unwrap_or("0.0.0.0".to_string()).as_str())
                .map_err(ErrorInternalServerError)?,
        ),
        announced_address: None,
        port: None,
        port_range: Some(RangeInclusive::new(
            plain_sync_port_min,
            plain_sync_port_max,
        )),
        flags: None,
        send_buffer_size: None,
        recv_buffer_size: None,
        expose_internal_ip: false,
    });

    receive_transport_options.comedia = true;

    let receive_transport = router
        .create_plain_transport(receive_transport_options)
        .await
        .map_err(ErrorInternalServerError)?;

    let conn = receive_transport.tuple();
    let rtcp_conn = receive_transport
        .rtcp_tuple()
        .context("failed to get rtcp conn -> check mux disabled")
        .map_err(ErrorInternalServerError)?;

    Ok(web::Json(PlainSyncResponse {
        port: conn.local_port(),
        rtcp_port: rtcp_conn.local_port(),
        producers,
    }))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::get().to(websocket))
        .route("/plainSync", web::post().to(plain_sync));
}

fn media_codecs() -> Vec<RtpCodecCapability> {
    vec![
        RtpCodecCapability::Audio {
            mime_type: MimeTypeAudio::Opus,
            preferred_payload_type: None,
            clock_rate: NonZeroU32::new(48000).unwrap(),
            channels: NonZeroU8::new(2).unwrap(),
            parameters: RtpCodecParametersParameters::from([("useinbandfec", 1_u32.into())]),
            rtcp_feedback: vec![RtcpFeedback::TransportCc],
        },
        RtpCodecCapability::Video {
            mime_type: MimeTypeVideo::Vp8,
            preferred_payload_type: None,
            clock_rate: NonZeroU32::new(90000).unwrap(),
            parameters: RtpCodecParametersParameters::default(),
            rtcp_feedback: vec![
                RtcpFeedback::Nack,
                RtcpFeedback::NackPli,
                RtcpFeedback::CcmFir,
                RtcpFeedback::GoogRemb,
                RtcpFeedback::TransportCc,
            ],
        },
    ]
}
