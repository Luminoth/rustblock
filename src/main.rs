mod block;
mod chain;
mod p2p;

use std::io::{stdin, BufReader};

use chrono::Duration;
use libp2p::core::upgrade;
use libp2p::identity::Keypair;
use libp2p::mplex;
use libp2p::noise::*;
use libp2p::swarm::*;
use libp2p::tcp::TokioTcpConfig;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::chain::Chain;

// TODO: see the article for details on adding a retry mechanism for failed blocks

// TODO: this is overly simplified
// see the article for more details on how to go deeper here
// ("00" is relatively easy and quick to mine)
const DIFFICULTY_PREFIX: &str = "00";

fn init_logging() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;

    info!("Peer Id: {}", p2p::PEER_ID.clone());

    // create event channels
    let (init_sender, mut init_rcv) = mpsc::unbounded_channel();
    let (response_sender, mut response_rcv) = mpsc::unbounded_channel();

    // create auth keys
    let auth_keys = Keypair::new().into_authentic(&p2p::KEYS)?;

    // create transport
    let transp = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    // create p2p behavior
    let behaviour = p2p::Behavior::new(Chain::new(), init_sender.clone(), response_sender).await;

    // create the p2p swarm
    let mut swarm = SwarmBuilder::new(transp, behaviour, *p2p::PEER_ID)
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        }))
        .build();

    // start the swawrm
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

    // spawn a delayed init event
    tokio::spawn(async move {
        sleep(Duration::from_secs(1)).await;

        info!("sending init event");
        init_sender.send(true)?;
    });

    let mut stdin = BufReader::new(stdin()).lines();

    loop {
        let evt = {
            tokio::select! {
                line = stdin.next_line() => Some(p2p::EventType::Input(line??)),
                _ = init_rcv.recv() => {
                    Some(p2p::EventType::Init)
                },
                response = response_rcv.recv() => {
                    Some(p2p::EventType::LocalChainResponse(response.unwrap()))
                },
                event = swarm.select_next_some() => {
                    info!("Unhandled Swarm Event: {:?}", event);
                    None
                },
            }
        };

        if let Some(event) = evt {
            match event {
                p2p::EventType::Init => {
                    let peers = p2p::get_list_peers(&swarm);
                    swarm.behaviour_mut().chain.genesis();

                    info!("connected nodes: {}", peers.len());
                    if !peers.is_empty() {
                        let req = p2p::LocalChainRequest {
                            from_peer_id: peers.iter().last()?.to_string(),
                        };

                        let json = serde_json::to_string(&req)?;
                        swarm
                            .behaviour_mut()
                            .floodsub
                            .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
                    }
                }
                p2p::EventType::LocalChainResponse(resp) => {
                    let json = serde_json::to_string(&resp)?;
                    swarm
                        .behaviour_mut()
                        .floodsub
                        .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
                }
                p2p::EventType::Input(line) => match line.as_str() {
                    "ls p" => p2p::handle_print_peers(&swarm),
                    cmd if cmd.starts_with("ls c") => p2p::handle_print_chain(&swarm),
                    cmd if cmd.starts_with("create b") => p2p::handle_create_block(cmd, &mut swarm),
                    _ => error!("unknown command"),
                },
            }
        }
    }
}
