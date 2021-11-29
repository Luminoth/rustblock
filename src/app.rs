use std::time::Duration;

use futures::StreamExt;
use libp2p::{
    core::muxing::StreamMuxerBox,
    core::transport,
    core::upgrade,
    mplex, noise,
    swarm::{Swarm, SwarmBuilder},
    tcp::TokioTcpConfig,
    PeerId, Transport,
};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    sync::mpsc,
    time::sleep,
};
use tracing::{debug, error, info};

use crate::chain::Chain;
use crate::p2p;

#[derive(Debug, Default)]
pub struct App;

impl App {
    fn create_transport() -> anyhow::Result<transport::Boxed<(PeerId, StreamMuxerBox)>> {
        // use noise for authentication
        let auth_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&p2p::KEYS)?;

        // create a tokio-based TCP transport
        let transp = TokioTcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(auth_keys).into_authenticated())
            .multiplex(mplex::MplexConfig::new())
            .boxed();

        Ok(transp)
    }

    async fn create_swarm(
        response_sender: mpsc::UnboundedSender<p2p::ChainResponse>,
    ) -> anyhow::Result<Swarm<p2p::Behaviour>> {
        let transp = Self::create_transport()?;
        let behaviour = p2p::Behaviour::new(Chain::new(), response_sender).await?;

        let mut swarm = SwarmBuilder::new(transp, behaviour, *p2p::PEER_ID)
            // spawn background tasks on the tokio runtime
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build();

        // start the swarm listening
        Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

        Ok(swarm)
    }

    async fn handle_event(
        event: p2p::EventType,
        swarm: &mut Swarm<p2p::Behaviour>,
    ) -> anyhow::Result<()> {
        match event {
            p2p::EventType::Init => {
                // init the chain
                swarm.behaviour_mut().chain().write().await.genesis();

                let peers = p2p::get_list_peers(&swarm);
                info!("connected nodes: {}", peers.len());

                if !peers.is_empty() {
                    // request the chain from the last peer in the list
                    let req = p2p::LocalChainRequest {
                        from_peer_id: peers.iter().last().unwrap().to_string(),
                    };

                    let json = serde_json::to_string(&req)?;
                    swarm
                        .behaviour_mut()
                        .protocol_mut()
                        .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
                }
            }
            p2p::EventType::LocalChainResponse(resp) => {
                let json = serde_json::to_string(&resp)?;
                swarm
                    .behaviour_mut()
                    .protocol_mut()
                    .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
            }
            p2p::EventType::Input(line) => match line.as_str() {
                "ls p" => p2p::handle_print_peers(&swarm),
                cmd if cmd.starts_with("ls c") => p2p::handle_print_chain(&swarm).await,
                cmd if cmd.starts_with("create b") => p2p::handle_create_block(cmd, swarm).await?,
                _ => error!("unknown command"),
            },
        }

        Ok(())
    }

    async fn do_run(
        mut init_rcv: mpsc::UnboundedReceiver<bool>,
        mut response_rcv: mpsc::UnboundedReceiver<p2p::ChainResponse>,
        mut swarm: Swarm<p2p::Behaviour>,
    ) -> anyhow::Result<()> {
        let mut stdin = BufReader::new(stdin()).lines();

        loop {
            let evt = {
                tokio::select! {
                    line = stdin.next_line() => Some(p2p::EventType::Input(line?.unwrap())),
                    _ = init_rcv.recv() => {
                        Some(p2p::EventType::Init)
                    },
                    response = response_rcv.recv() => {
                        Some(p2p::EventType::LocalChainResponse(response.unwrap()))
                    },
                    event = swarm.select_next_some() => {
                        debug!("Unhandled Swarm Event: {:?}", event);
                        None
                    },
                }
            };

            if let Some(event) = evt {
                Self::handle_event(event, &mut swarm).await?;
            }
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Peer Id: {}", p2p::PEER_ID.clone());

        // create event channels
        let (init_sender, init_rcv) = mpsc::unbounded_channel();
        let (response_sender, response_rcv) = mpsc::unbounded_channel();

        // create the swarm
        let swarm = Self::create_swarm(response_sender).await?;

        // spawn a delayed init event
        tokio::spawn(async move {
            sleep(Duration::from_secs(1)).await;

            info!("sending init event");
            init_sender.send(true).unwrap();
        });

        Self::do_run(init_rcv, response_rcv, swarm).await
    }
}
