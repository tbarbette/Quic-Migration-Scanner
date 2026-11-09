//! Standalone local QUIC + HTTP/3 server with multipath enabled, for manual
//! testing against `migration_checker --multipath`. Adapted from quiche's own
//! `examples/http3-server.rs`, trimmed to a fixed 200 response and with the
//! `initial_max_path_id` transport parameter always set.
//!
//! Usage: cargo run --example multipath_server [listen_addr] [--no-multipath]
//! Default listen address: 127.0.0.1:4433

use std::collections::HashMap;
use std::time::Duration;

use ring::rand::*;

const MAX_DATAGRAM_SIZE: usize = 1350;

struct Client {
    conn: quiche::Connection,
    http3_conn: Option<quiche::h3::Connection>,
}

type ClientMap = HashMap<quiche::ConnectionId<'static>, Client>;

fn main() {
    env_logger::init();

    let mut listen_addr = "127.0.0.1:4433".to_string();
    let mut multipath = true;
    for arg in std::env::args().skip(1) {
        if arg == "--no-multipath" {
            multipath = false;
        } else {
            listen_addr = arg;
        }
    }

    let mut buf = [0; 65535];
    let mut out = [0; MAX_DATAGRAM_SIZE];

    let mut poll = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(1024);

    let mut socket =
        mio::net::UdpSocket::bind(listen_addr.parse().unwrap()).unwrap();
    let local_addr = socket.local_addr().unwrap();
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
    config.set_active_connection_id_limit(4);
    if multipath {
        config.set_initial_max_path_id(2);
    }

    println!(
        "multipath_server: listening on {} (multipath {})",
        local_addr,
        if multipath { "ENABLED" } else { "disabled" }
    );
    println!("Try: ./target/debug/migration_checker --multipath -a 127.0.0.1 <input file with {}>", local_addr);

    let h3_config = quiche::h3::Config::new().unwrap();
    let rng = SystemRandom::new();
    let conn_id_seed =
        ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    let mut clients: ClientMap = HashMap::new();

    loop {
        let timeout = clients
            .values()
            .filter_map(|c| c.conn.timeout())
            .min()
            .or(Some(Duration::from_millis(200)));

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
                Err(e) => {
                    eprintln!("parsing packet header failed: {:?}", e);
                    continue 'read;
                },
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

                println!("new connection from {}", from);

                let conn =
                    quiche::accept(&scid, None, local_addr, from, &mut config)
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

            let was_established = client.conn.is_established();
            if let Err(e) = client.conn.recv(pkt_buf, recv_info) {
                eprintln!("{} recv failed: {:?}", client.conn.trace_id(), e);
                continue 'read;
            }

            if !was_established && client.conn.is_established() {
                println!(
                    "{} handshake done, multipath_enabled={}",
                    client.conn.trace_id(),
                    client.conn.is_multipath_enabled()
                );
            }

            if (client.conn.is_in_early_data() || client.conn.is_established()) &&
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
                                    b"quiche-multipath-test",
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
                        Err(e) => {
                            eprintln!("http/3 error: {:?}", e);
                            break;
                        },
                    }
                }
            }
        }

        for client in clients.values_mut() {
            loop {
                let (write, send_info) = match client.conn.send(&mut out) {
                    Ok(v) => v,
                    Err(quiche::Error::Done) => break,
                    Err(e) => {
                        eprintln!("send failed: {:?}", e);
                        break;
                    },
                };

                let _ = socket.send_to(&out[..write], send_info.to);
            }
        }

        clients.retain(|_, c| {
            if c.conn.is_closed() {
                println!(
                    "{} connection closed, multipath_enabled={} stats={:?}",
                    c.conn.trace_id(),
                    c.conn.is_multipath_enabled(),
                    c.conn.stats()
                );
            }
            !c.conn.is_closed()
        });
    }
}
