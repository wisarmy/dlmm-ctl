use anchor_client::{
    solana_client::{self, nonblocking::rpc_client::RpcClient},
    solana_sdk::signature::Keypair,
};
use anyhow::Result;
use rand::seq::SliceRandom;
use std::{env, sync::Arc};
use tracing::debug;

pub mod list_all_binstep;
pub mod logger;
pub mod math;
pub mod pair;
pub mod parser;
pub mod position;

pub fn get_random_rpc_url() -> Result<(String, String)> {
    let cluster_urls = env::var("RPC_ENDPOINTS")?
        .split(",")
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();
    let wss_urls = env::var("RPC_WEBSOCKET_ENDPOINTS")?
        .split(",")
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();

    let random_url = cluster_urls
        .choose(&mut rand::thread_rng())
        .expect("No RPC endpoints configured")
        .clone();
    let random_wss_url = wss_urls
        .choose(&mut rand::thread_rng())
        .expect("No RPC endpoints configured")
        .clone();

    debug!("Choose rpc: {}, {}", random_url, random_wss_url);
    return Ok((random_url, random_wss_url));
}

pub fn get_rpc_client() -> Result<Arc<RpcClient>> {
    let random_url = get_random_rpc_url()?;
    let client = RpcClient::new(random_url.0);
    return Ok(Arc::new(client));
}

pub fn get_rpc_client_blocking() -> Result<Arc<solana_client::rpc_client::RpcClient>> {
    let random_url = get_random_rpc_url()?;
    let client = solana_client::rpc_client::RpcClient::new(random_url.0);
    return Ok(Arc::new(client));
}

pub fn get_payer() -> Result<Arc<Keypair>> {
    let wallet = Keypair::from_base58_string(&env::var("PRIVATE_KEY")?);
    return Ok(Arc::new(wallet));
}

#[cfg(test)]
mod tests {
    #[ctor::ctor]
    fn init() {
        crate::logger::init();
        dotenvy::dotenv().ok();
    }
}
