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

use ring::rand::*;

const MAX_DATAGRAM_SIZE: usize = 1350;

struct Client {
    conn: quiche::Connection,
    http3_conn: Option<quiche::h3::Connection>,
}

type ClientMap = HashMap<quiche::ConnectionId<'static>, Client>;

/// Spawns a local QUIC+HTTP/3 server on 127.0.0.1 in a background thread and
/// returns the address it is listening on. The server answers every request
/// with a fixed 200 response and lives for the lifetime of the test process.
pub fn spawn(multipath: bool) -> SocketAddr {
    let mut socket =
        mio::net::UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
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

    let h3_config = quiche::h3::Config::new().unwrap();
    let rng = SystemRandom::new();
    let conn_id_seed =
        ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    thread::spawn(move || {
        let mut buf = [0; 65535];
        let mut out = [0; MAX_DATAGRAM_SIZE];
        let mut clients: ClientMap = HashMap::new();

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

                let client = if !clients.contains_key(&hdr.dcid) &&
                    !clients.contains_key(&conn_id)
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

                    let client = Client {
                        conn,
                        http3_conn: None,
                    };

                    clients.insert(scid.clone(), client);
                    clients.get_mut(&scid).unwrap()
                } else {
                    match clients.get_mut(&hdr.dcid) {
                        Some(v) => v,
                        None => clients.get_mut(&conn_id).unwrap(),
                    }
                };

                let recv_info = quiche::RecvInfo {
                    to: local_addr,
                    from,
                };

                if client.conn.recv(pkt_buf, recv_info).is_err() {
                    continue 'read;
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
                            Ok((stream_id, quiche::h3::Event::Headers { .. })) => {
                                let headers = vec![
                                    quiche::h3::Header::new(b":status", b"200"),
                                    quiche::h3::Header::new(
                                        b"server",
                                        b"quiche-test",
                                    ),
                                ];
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
        }
    });

    local_addr
}
