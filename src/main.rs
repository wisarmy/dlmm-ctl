use std::{env, sync::Arc, time::Duration};

use anchor_client::{
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair, signer::Signer},
    Client, Cluster,
};
use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::ui_amount_to_amount;
use anyhow::Result;
use clap::{Parser, Subcommand};
use dlmm_ctl::{
    db::Database,
    get_payer,
    instructions::swap_exact_in::{self, SwapExactInParameters},
    logger,
    pair::get_pair,
    position::PositionManager,
};
use tracing::{debug, error, info};

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
    MonitorPositions {
        #[clap(
            long,
            default_value_t = 60,
            help = "Interval in seconds to check positions"
        )]
        interval: u64,
        #[clap(long, default_value = "false", help = "Enable stop loss")]
        stop_loss: bool,
    },
    /// Show information of the given liquidity pair.
    ShowPair { lb_pair: Pubkey },
    Swap {
        #[clap(help = "Liquidity pair pubkey")]
        lb_pair: Pubkey,
        #[clap(help = "Swap direction: [buy, sell]")]
        direction: String,
        #[clap(help = "UI amount to swap in")]
        amount_in: f64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Commands::MonitorPositions {
            interval: _,
            stop_loss: _,
        } => logger::init(false),
        _ => logger::init(true),
    }

    let payer = get_payer()?;
    let (rpc_url, wss_url) = dlmm_ctl::get_random_rpc_url()?;

    let commitment_config = CommitmentConfig::confirmed();
    let client = Client::new_with_options(
        Cluster::Custom(rpc_url, wss_url),
        Arc::new(Keypair::from_bytes(&payer.to_bytes())?),
        commitment_config,
    );

    let program = client.program(lb_clmm::ID)?;
    let program = Arc::new(program);
    let db = Database::new(
        &env::var("DATABASE_URL")
            .unwrap_or_else(|_| panic!("Environment variable {} is not set", "DATABASE_URL")),
    )
    .await
    .unwrap();

    match cli.command {
        Commands::Positions => {
            let position_manager = PositionManager::new(&program, payer.pubkey(), db);
            let positions = position_manager.get_positions().await?;
            for (pubkey, position) in positions {
                debug!("position: {}, {:#?}", pubkey, position);
                info!(
                    "position: {}, lb_pair: {}, lower_bin_id: {}, upper_bin_id: {}",
                    pubkey, position.lb_pair, position.lower_bin_id, position.upper_bin_id
                );
            }
        }
        Commands::MonitorPositions {
            interval,
            stop_loss,
        } => {
            let mut position_manager = PositionManager::new(&program, payer.pubkey(), db);
            if let Err(err) = position_manager
                .monitor_positions(Duration::from_secs(interval), stop_loss)
                .await
            {
                error!("monitor_positions: Error: {:?}", err);
            }
        }
        Commands::ShowPair { lb_pair } => {
            let pair_info = get_pair(lb_pair, &program).await?;
            info!("{:#?}", pair_info);
        }
        Commands::Swap {
            lb_pair,
            amount_in,
            direction,
        } => {
            let pair_info = get_pair(lb_pair, &program).await?;
            info!("{:#?}", pair_info);

            let commitment_config = CommitmentConfig::confirmed();
            let transaction_config = RpcSendTransactionConfig {
                skip_preflight: false,
                preflight_commitment: Some(commitment_config.commitment),
                encoding: None,
                max_retries: None,
                min_context_slot: None,
            };
            let (decimals, swap_for_y) = if direction == "buy" {
                (pair_info.y_mint.decimals, false)
            } else {
                (pair_info.x_mint.decimals, true)
            };
            let amount_in = ui_amount_to_amount(amount_in, decimals);
            let params = SwapExactInParameters {
                amount_in,
                lb_pair,
                swap_for_y,
            };
            info!("swap: {:#?}", params);

            if let Err(e) = swap_exact_in::swap(params, &program, transaction_config).await {
                error!("swap_exact_in: Error: {:?}", e);
            }
        }
    }

    Ok(())
}
