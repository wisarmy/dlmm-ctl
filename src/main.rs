use std::rc::Rc;

use anchor_client::{
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair, signer::Signer},
    Client, Cluster,
};
use anchor_lang::prelude::*;
use anyhow::Result;
use clap::{Parser, Subcommand};
use dlmm_ctl::{get_payer, logger};
use tracing::info;

use crate::position::PositionManager;

mod parser;
mod position;

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
                info!("position: {}, lb_pair: {}", pubkey, position.lb_pair);
            }
        }
        Commands::ShowPair { lb_pair } => {
            dlmm_ctl::pair::show_pair(lb_pair, &program).await?;
        }
    }

    Ok(())
}
