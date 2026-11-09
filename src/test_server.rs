//! Minimal local QUIC + HTTP/3 server used only by tests, to check that
//! `migration_checker --multipath` correctly reports whether a peer
//! negotiates multipath. Adapted from quiche's own `examples/http3-server.rs`
//! (retry + HTTP/3 handling), trimmed down to a fixed 200 response and with
//! the multipath transport parameter toggled based on the `multipath` flag.
#![cfg(test)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use quiche::h3::NameValue;
use ring::rand::*;

const MAX_DATAGRAM_SIZE: usize = 1350;

struct Client {
    client_id: u64,
    conn: quiche::Connection,
    http3_conn: Option<quiche::h3::Connection>,
}

type ClientMap = HashMap<u64, Client>;
type ClientIdMap = HashMap<quiche::ConnectionId<'static>, u64>;

/// Spawns a local QUIC+HTTP/3 server on 127.0.0.1 in a background thread and
/// returns the address it is listening on. The server answers every request
/// with a fixed 200 response and lives for the lifetime of the test process.
pub fn spawn(multipath: bool) -> SocketAddr {
    spawn_with(Opts { multipath, ..Opts::default() }).expect("failed to bind 127.0.0.1")
}

/// Knobs for the test server.
pub struct Opts {
    /// Advertise the multipath transport parameter.
    pub multipath: bool,
    /// Advertise `disable_active_migration`.
    pub disable_active_migration: bool,
    /// Answer every path except `/final` with a 302 to `/final`.
    pub redirect: bool,
    /// Listen on `[::1]` instead of `127.0.0.1`.
    pub ipv6: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { multipath: false, disable_active_migration: false, redirect: false, ipv6: false }
    }
}

/// Like [`spawn`], but configurable. Returns `None` when the requested address
/// family is unavailable on the machine (e.g. no IPv6 loopback).
pub fn spawn_with(opts: Opts) -> Option<SocketAddr> {
    let multipath = opts.multipath;
    let redirect = opts.redirect;
    let bind = if opts.ipv6 { "[::1]:0" } else { "127.0.0.1:0" };
    let mut socket = mio::net::UdpSocket::bind(bind.parse().unwrap()).ok()?;
    let local_addr = socket.local_addr().unwrap();

    let mut poll = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(1024);
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
        .unwrap();

    let cert_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/quiche/quiche/examples/cert.crt");
    let key_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/quiche/quiche/examples/cert.key");

    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
    config.load_cert_chain_from_pem_file(cert_path).unwrap();
    config.load_priv_key_from_pem_file(key_path).unwrap();
    config
        .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
        .unwrap();
    config.set_max_idle_timeout(5000);
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(100);
    config.set_initial_max_streams_uni(100);
    if multipath {
        config.set_initial_max_path_id(2);
    }
    config.set_disable_active_migration(opts.disable_active_migration);

    let h3_config = quiche::h3::Config::new().unwrap();
    let rng = SystemRandom::new();
    let conn_id_seed =
        ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    thread::spawn(move || {
        let mut buf = [0; 65535];
        let mut out = [0; MAX_DATAGRAM_SIZE];
        let mut clients: ClientMap = HashMap::new();
        let mut clients_ids: ClientIdMap = HashMap::new();
        let mut next_client_id = 0;

        loop {
            let timeout = clients
                .values()
                .filter_map(|c| c.conn.timeout())
                .min()
                .or(Some(Duration::from_millis(100)));

            poll.poll(&mut events, timeout).unwrap();

            'read: loop {
                if events.is_empty() {
                    clients.values_mut().for_each(|c| c.conn.on_timeout());
                    break 'read;
                }

                let (len, from) = match socket.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            break 'read;
                        }
                        break 'read;
                    },
                };

                let pkt_buf = &mut buf[..len];

                let hdr = match quiche::Header::from_slice(
                    pkt_buf,
                    quiche::MAX_CONN_ID_LEN,
                ) {
                    Ok(v) => v,
                    Err(_) => continue 'read,
                };

                let conn_id = ring::hmac::sign(&conn_id_seed, &hdr.dcid);
                let conn_id = &conn_id.as_ref()[..quiche::MAX_CONN_ID_LEN];
                let conn_id: quiche::ConnectionId = conn_id.to_vec().into();

                let client = if !clients_ids.contains_key(&hdr.dcid) &&
                    !clients_ids.contains_key(&conn_id)
                {
                    if hdr.ty != quiche::Type::Initial {
                        continue 'read;
                    }

                    if !quiche::version_is_supported(hdr.version) {
                        continue 'read;
                    }

                    let mut scid = [0; quiche::MAX_CONN_ID_LEN];
                    scid.copy_from_slice(&conn_id);
                    let scid: quiche::ConnectionId = scid.to_vec().into();

                    let conn = quiche::accept(
                        &scid,
                        None,
                        local_addr,
                        from,
                        &mut config,
                    )
                    .unwrap();

                    let client_id = next_client_id;
                    next_client_id += 1;
                    let client = Client {
                        client_id,
                        conn,
                        http3_conn: None,
                    };

                    clients.insert(client_id, client);
                    clients_ids.insert(scid.clone(), client_id);
                    clients.get_mut(&client_id).unwrap()
                } else {
                    let id = match clients_ids.get(&hdr.dcid) {
                        Some(v) => v,
                        None => clients_ids.get(&conn_id).unwrap(),
                    };
                    clients.get_mut(id).unwrap()
                };

                let recv_info = quiche::RecvInfo {
                    to: local_addr,
                    from,
                };

                if client.conn.recv(pkt_buf, recv_info).is_err() {
                    continue 'read;
                }

                // Needed for the client to probe/migrate to a new path: hand
                // out spare connection IDs, and forget the retired ones.
                while let Some(retired) = client.conn.retired_scid_next() {
                    clients_ids.remove(&retired);
                }
                for path_id in client.conn.path_ids() {
                    while client.conn.scids_left_on_path(path_id) > 0 {
                        let mut cid = [0; quiche::MAX_CONN_ID_LEN];
                        SystemRandom::new().fill(&mut cid[..]).unwrap();
                        let mut token = [0; 16];
                        SystemRandom::new().fill(&mut token[..]).unwrap();
                        let cid = quiche::ConnectionId::from_vec(cid.to_vec());
                        if client
                            .conn
                            .new_scid_on_path(path_id, &cid, u128::from_be_bytes(token), false)
                            .is_err()
                        {
                            break;
                        }
                        clients_ids.insert(cid, client.client_id);
                    }
                }

                if (client.conn.is_in_early_data() ||
                    client.conn.is_established()) &&
                    client.http3_conn.is_none()
                {
                    if let Ok(h3) = quiche::h3::Connection::with_transport(
                        &mut client.conn,
                        &h3_config,
                    ) {
                        client.http3_conn = Some(h3);
                    }
                }

                if let Some(http3_conn) = client.http3_conn.as_mut() {
                    loop {
                        match http3_conn.poll(&mut client.conn) {
                            Ok((stream_id, quiche::h3::Event::Headers { list, .. })) => {
                                let path = list
                                    .iter()
                                    .find(|h| h.name() == b":path")
                                    .map(|h| h.value().to_vec())
                                    .unwrap_or_default();
                                let headers = if redirect && path != b"/final" {
                                    vec![
                                        quiche::h3::Header::new(b":status", b"302"),
                                        quiche::h3::Header::new(b"location", b"/final"),
                                    ]
                                } else {
                                    vec![
                                        quiche::h3::Header::new(b":status", b"200"),
                                        quiche::h3::Header::new(
                                            b"server",
                                            b"quiche-test",
                                        ),
                                    ]
                                };
                                let _ = http3_conn.send_response(
                                    &mut client.conn,
                                    stream_id,
                                    &headers,
                                    true,
                                );
                            },
                            Ok(_) => (),
                            Err(quiche::h3::Error::Done) => break,
                            Err(_) => break,
                        }
                    }
                }
            }

            for client in clients.values_mut() {
                loop {
                    let (write, send_info) = match client.conn.send(&mut out) {
                        Ok(v) => v,
                        Err(quiche::Error::Done) => break,
                        Err(_) => break,
                    };

                    let _ = socket.send_to(&out[..write], send_info.to);
                }
            }

            clients.retain(|_, c| !c.conn.is_closed());
            clients_ids.retain(|_, id| clients.contains_key(id));
        }
    });

    Some(local_addr)
}
