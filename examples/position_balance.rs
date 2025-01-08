use anchor_client::{solana_sdk::signer::Signer, Client, Cluster};
use anyhow::Result;
use dlmm_ctl::{get_payer, position_manager::PositionManager};
use std::{env, sync::Arc};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // init
    dotenvy::dotenv().ok();
    dlmm_ctl::logger::init(true);

    let payer = get_payer()?;
    let (rpc_url, wss_url) = dlmm_ctl::get_random_rpc_url()?;
    let client = Client::new_with_options(
        Cluster::Custom(rpc_url, wss_url),
        payer.clone(),
        Default::default(),
    );
    let program = Arc::new(client.program(lb_clmm::ID)?);

    let db = dlmm_ctl::db::Database::new(&env::var("DATABASE_URL")?).await?;
    let position_manager = PositionManager::new(&program, payer.pubkey(), db);
    let positions = position_manager.get_positions().await?;
    for (pubkey, position) in positions {
        if let Ok(mut position_info) = position_manager.get_info(pubkey, position).await {
            position_info.with_balance(&program).await?;
            let (amount_x, amount_y, fee_x_pending, fee_y_pending) =
                position_info.balance.unwrap_or_default();

            info!(
                "amount_x {amount_x} amount_y {amount_y} fee_x_pending {fee_x_pending} fee_y_pending {fee_y_pending}"
            );
        }
    }

    Ok(())
}
