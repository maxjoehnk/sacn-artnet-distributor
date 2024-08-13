use std::collections::HashMap;
use sacn_unofficial::packet::ACN_SDT_MULTICAST_PORT;
use sacn_unofficial::receive::SacnReceiver;
use serde::Deserialize;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

#[derive(Debug, Clone, Deserialize)]
pub struct Mapping {
    #[serde(alias = "in")]
    pub input_universes: Vec<u16>,
    #[serde(alias = "out")]
    pub output_universes: Vec<u16>,
    pub address: IpAddr,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(rename = "mapping")]
    pub mappings: Vec<Mapping>,
}

#[derive(Debug)]
struct OutputConfig {
    universe: u16,
    address: IpAddr,
}

fn main() -> color_eyre::Result<()> {
    tracing_subscriber::fmt::init();
    let file = fs::read_to_string("bindings.toml")?;
    let config: Config = toml::from_str(&file)?;

    tracing::info!("Translating universes: {:#?}", config);

    let mut threads = Vec::with_capacity(config.mappings.len());

    for mapping in config.mappings.into_iter() {
        let input_universes  = mapping.input_universes.iter().map(|u| u.to_string()).collect::<Vec<_>>().join("-");
        let handle = std::thread::Builder::new()
            .name(format!("universes-{input_universes}"))
            .spawn(move || {
                let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), ACN_SDT_MULTICAST_PORT);

                let mut dmx_receiver = SacnReceiver::with_ip(addr, None)
                    // .map_err(|err| color_eyre::eyre::eyre!("Unable to create sACN Receiver: {err:?}"))
                    .unwrap();

                dmx_receiver.listen_universes(&mapping.input_universes)
                    .unwrap();
                    // .map_err(|err| color_eyre::eyre::eyre!("Unable to listen to universes: {err:?}"))?;

                let output_socket = UdpSocket::bind(("0.0.0.0", 0)).unwrap();

                let mappings = mapping.input_universes.into_iter()
                    .zip(mapping.output_universes)
                    .collect::<HashMap<_, _>>();

                let artnet_addr = SocketAddr::new(mapping.address, 6454);

                loop {
                    match dmx_receiver.recv(None) {
                        Ok(packet) => {
                            for dmx in packet {
                                let universe = match mappings.get(&dmx.universe) {
                                    Some(mapping) => mapping,
                                    None => continue,
                                };
                                if let Err(err) = send(&output_socket, &artnet_addr, *universe, dmx.values) {
                                    tracing::error!("Unable to send to artnet server {err:?}");
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!("Error receiving packet: {err:?}");
                        }
                    }
                }
            })?;
        threads.push(handle);
    }

    for handle in threads.into_iter() {
        handle.join().unwrap();
    }

    Ok(())
}

fn send(output_socket: &UdpSocket, address: &SocketAddr, universe: u16, data: Vec<u8>) -> color_eyre::Result<()> {
    let msg = artnet_protocol::Output {
        port_address: artnet_protocol::PortAddress::try_from(universe)?,
        data: data.into_iter().take(512).collect::<Vec<_>>().into(),
        ..artnet_protocol::Output::default()
    };
    let msg = artnet_protocol::ArtCommand::Output(msg).write_to_buffer()?;

    output_socket.send_to(&msg, address)?;

    Ok(())
}
