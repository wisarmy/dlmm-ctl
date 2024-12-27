use std::ops::Deref;

use anchor_client::solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};

use anchor_client::{solana_sdk::pubkey::Pubkey, solana_sdk::signer::Signer, Program};

use anchor_spl::token::Mint;
use anyhow::{Context, Result};

use lb_clmm::constants::FEE_PRECISION;
use lb_clmm::math::price_math::get_price_from_id;
use lb_clmm::state::bin::BinArray;
use lb_clmm::state::lb_pair::LbPair;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use solana_account_decoder::UiAccountEncoding;
use tracing::debug;

use crate::math::{price_per_lamport_to_price_per_token, q64x64_price_to_decimal};
use crate::parser::parse_bin_data;

#[derive(Debug, Clone)]
pub struct PairInfo {
    pub lb_pair: Pubkey,
    /// Active bin id
    pub active_id: i32,
    /// Bin step. Represent the price increment / decrement.
    pub bin_step: u16,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub x_mint: Mint,
    pub y_mint: Mint,
    pub token_price: Decimal,
    pub base_fee_rate: Decimal,
    pub variable_fee_rate: Decimal,
    pub total_fee_rate: Decimal,
}

fn fee_rate_to_fee_pct(fee_rate: u128) -> Option<Decimal> {
    let fee_rate = Decimal::from_u128(fee_rate)?.checked_div(Decimal::from(FEE_PRECISION))?;
    fee_rate.checked_mul(Decimal::ONE_HUNDRED)
}

/// get_bin_arrays returns all bin arrays for a given lb_pair
pub async fn get_bin_arrays<C: Deref<Target = impl Signer> + Clone>(
    lb_pair: Pubkey,
    program: &Program<C>,
) -> Result<Vec<(Pubkey, BinArray)>> {
    // let lb_pair_filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(16, &lb_pair.to_bytes()));
    // let mut bin_arrays: Vec<(Pubkey, BinArray)> = program.accounts(vec![lb_pair_filter]).await?;
    let filters = vec![
        RpcFilterType::DataSize((8 + 10128) as u64),
        RpcFilterType::Memcmp(Memcmp::new(
            24,
            MemcmpEncodedBytes::Base58(lb_pair.to_string()),
        )),
    ];

    let config = RpcProgramAccountsConfig {
        filters: Some(filters),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            ..Default::default()
        },
        ..Default::default()
    };

    let bin_accounts = program
        .async_rpc()
        .get_program_accounts_with_config(&lb_clmm::ID, config)
        .await?;

    let mut bin_arrays = bin_accounts
        .iter()
        .filter_map(|(pubkey, account)| {
            parse_bin_data(&account.data)
                .ok()
                .map(|bin_array| (*pubkey, bin_array))
        })
        .collect::<Vec<(Pubkey, BinArray)>>();
    bin_arrays.sort_by(|a, b| a.1.index.cmp(&b.1.index));
    debug!("bin_arrays: {:#?}", bin_arrays);
    // for (_, bin_array) in bin_arrays.iter() {
    //     let (mut lower_bin_id, _) =
    //         BinArray::get_bin_array_lower_upper_bin_id(bin_array.index as i32)?;
    //     for bin in bin_array.bins.iter() {
    //         let total_amount = bin.amount_x + bin.amount_y;
    //         if total_amount > 0 {
    //             debug!(
    //                 "Bin: {}, X: {}, Y: {}",
    //                 lower_bin_id, bin.amount_x, bin.amount_y
    //             );
    //         }
    //         lower_bin_id += 1;
    //     }
    // }
    Ok(bin_arrays)
}

pub fn caculate_bin_price(
    bin_step: u16,
    bin_id: i32,
    x_mint: &Mint,
    y_mint: &Mint,
) -> Result<Decimal> {
    let q64x64_price = get_price_from_id(bin_id, bin_step)?;
    let decimal_price_per_lamport =
        q64x64_price_to_decimal(q64x64_price).context("q64x64 price to decimal overflow")?;

    let token_price = price_per_lamport_to_price_per_token(
        decimal_price_per_lamport
            .to_f64()
            .context("Decimal conversion to f64 fail")?,
        x_mint.decimals,
        y_mint.decimals,
    )
    .context("price_per_lamport_to_price_per_token overflow")?;

    Ok(token_price)
}

pub async fn get_pair<C: Deref<Target = impl Signer> + Clone>(
    lb_pair: Pubkey,
    program: &Program<C>,
) -> Result<PairInfo> {
    let lb_pair_state: LbPair = program.account(lb_pair).await?;
    debug!("lb_pair_state: {:#?}", lb_pair_state);

    let x_mint: Mint = program.account(lb_pair_state.token_x_mint).await?;
    let y_mint: Mint = program.account(lb_pair_state.token_y_mint).await?;
    let token_price = caculate_bin_price(
        lb_pair_state.bin_step,
        lb_pair_state.active_id,
        &x_mint,
        &y_mint,
    )?;

    let base_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_base_fee()?)
        .context("get_base_fee convert to percentage overflow")?;
    let variable_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_variable_fee()?)
        .context("get_variable_fee convert to percentage overflow")?;
    let total_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_total_fee()?)
        .context("get_total_fee convert to percentage overflow")?;

    Ok(PairInfo {
        lb_pair,
        active_id: lb_pair_state.active_id,
        bin_step: lb_pair_state.bin_step,
        token_x_mint: lb_pair_state.token_x_mint,
        token_y_mint: lb_pair_state.token_y_mint,
        x_mint,
        y_mint,
        token_price,
        base_fee_rate,
        variable_fee_rate,
        total_fee_rate,
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct PairApiInfo {
    pub address: String,
    pub name: String,
    pub mint_x: String,
    pub mint_y: String,
    pub current_price: f64,
}

pub async fn get_pair_api_info(pair_address: &str) -> Result<PairApiInfo> {
    let url = format!("https://app.meteora.ag/clmm-api/pair/{}", pair_address);

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?.json::<PairApiInfo>().await?;

    Ok(response)
}
