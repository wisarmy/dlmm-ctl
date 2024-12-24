use std::rc::Rc;

use anchor_client::{
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair, signer::Signer},
    Client, Cluster,
};
use anchor_lang::prelude::*;
use anyhow::Result;
use clap::{Parser, Subcommand};
use dlmm_ctl::{get_payer, logger, pair::get_pair, position::PositionManager};
use tracing::{debug, info};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get positions for the current wallet
    Positions,
    /// Monitor positions
    MonitorPositions,
    /// Show information of the given liquidity pair.
    ShowPair { lb_pair: Pubkey },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    logger::init();

    let payer = get_payer()?;
    let (rpc_url, wss_url) = dlmm_ctl::get_random_rpc_url()?;

    let commitment_config = CommitmentConfig::confirmed();
    let client = Client::new_with_options(
        Cluster::Custom(rpc_url, wss_url),
        Rc::new(Keypair::from_bytes(&payer.to_bytes())?),
        commitment_config,
    );

    let program = client.program(lb_clmm::ID)?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Positions => {
            let position_manager = PositionManager::new(&program, payer.pubkey());
            let positions = position_manager.get_positions().await?;
            for (pubkey, position) in positions {
                debug!("position: {}, {:#?}", pubkey, position);
                info!(
                    "position: {}, lb_pair: {}, lower_bin_id: {}, upper_bin_id: {}",
                    pubkey, position.lb_pair, position.lower_bin_id, position.upper_bin_id
                );
            }
        }
        Commands::MonitorPositions => {
            let position_manager = PositionManager::new(&program, payer.pubkey());
            position_manager.monitor_positions().await?;
        }
        Commands::ShowPair { lb_pair } => {
            let pair_info = get_pair(lb_pair, &program).await?;
            info!("{:#?}", pair_info);
        }
    }

    Ok(())
}
