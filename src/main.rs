use sacn_unofficial::packet::ACN_SDT_MULTICAST_PORT;
use sacn_unofficial::receive::SacnReceiver;
use serde::Deserialize;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use indexmap::IndexMap;

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

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), ACN_SDT_MULTICAST_PORT);

    let mut dmx_receiver = SacnReceiver::with_ip(addr, None)
        .map_err(|err| color_eyre::eyre::eyre!("Unable to create sACN Receiver: {err:?}"))?;

    let universes = config.mappings.iter().map(|mapping| mapping.input_universes.iter().copied()).flatten().collect::<Vec<_>>();
    dmx_receiver.listen_universes(&universes)
        .map_err(|err| color_eyre::eyre::eyre!("Unable to listen to universes: {err:?}"))?;

    let mappings = config.mappings.into_iter().flat_map(|mapping| {
        mapping.input_universes.into_iter()
            .zip(mapping.output_universes.into_iter())
            .map(move |(input, output)| (input, OutputConfig {
                universe: output,
                address: mapping.address,
            }))
    }).collect::<IndexMap<u16, _>>();

    tracing::info!("Translating universes: {:#?}", mappings);

    let output_socket = UdpSocket::bind(("0.0.0.0", 0))?;

    loop {
        match dmx_receiver.recv(None) {
            Ok(packet) => {
                for dmx in packet {
                    let mapping = match mappings.get(&dmx.universe) {
                        Some(mapping) => mapping,
                        None => continue,
                    };
                    if let Err(err) = send(&output_socket, mapping, dmx.values) {
                        tracing::error!("Unable to send to artnet server {err:?}");
                    }
                }
            }
            Err(err) => {
                tracing::error!("Error receiving packet: {err:?}");
            }
        }
    }
}

fn send(output_socket: &UdpSocket, mapping: &OutputConfig, data: Vec<u8>) -> color_eyre::Result<()> {
    let msg = artnet_protocol::Output {
        port_address: artnet_protocol::PortAddress::try_from(mapping.universe)?,
        data: data.into_iter().take(512).collect::<Vec<_>>().into(),
        ..artnet_protocol::Output::default()
    };
    let msg = artnet_protocol::ArtCommand::Output(msg).write_to_buffer()?;

    output_socket.send_to(&msg, SocketAddr::new(mapping.address, 6454))?;

    Ok(())
}
