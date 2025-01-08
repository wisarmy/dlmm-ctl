use std::ops::Deref;

use anchor_client::{solana_sdk::signer::Signer, Program};
use anchor_lang::prelude::Pubkey;
use anchor_spl::token::Mint;
use anyhow::{Context, Result};
use lb_clmm::{
    math::safe_math::SafeMath,
    state::{bin::BinArray, position::PositionV2},
    utils::pda::derive_bin_array_pda,
};
use rust_decimal::Decimal;

use crate::{
    pair::PairInfo, program_adapter, types::bin_array_manager::BinArrayManager,
    utils::price::get_token_prices,
};

#[derive(Debug)]
pub enum PositionMessage {
    ClaimFees(Pubkey),
    RemoveLiquidity(Pubkey, Pubkey),
    Close(Pubkey),
    SellAll(Pubkey),
}

pub enum UiMessage {
    Quit,
    ScrollUp,
    ScrollDown,
    UpdateStatus(String),
    UpdatePositions(Vec<PositionInfo>),
    Position(PositionMessage),
    UpdateSolPrice(f64),
}

#[derive(Debug, Clone)]
pub struct PositionInfo {
    pub position_state: PositionV2,
    pub pair_info: PairInfo,
    pub pubkey: Pubkey,
    pub lb_pair: Pubkey,
    pub pair_name: Option<String>,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub x_mint: Mint,
    pub y_mint: Mint,
    pub lower_bin_id: i32,
    pub lower_bin_price: Decimal,
    pub upper_bin_id: i32,
    pub upper_bin_price: Decimal,
    pub active_bin_id: i32,
    pub active_bin_price: Decimal,
    pub last_updated_at: i64,
    pub balance: Option<(f64, f64, f64, f64)>,
    pub token_x_price: Option<f64>,
    pub token_y_price: Option<f64>,
}

impl PositionInfo {
    pub async fn with_balance<C: Deref<Target = impl Signer> + Clone>(
        &mut self,
        program: &Program<C>,
    ) -> Result<()> {
        let lb_pair = self.lb_pair;
        let position_state = self.position_state.clone();

        let token_mint_base: Mint = self.pair_info.x_mint.clone();
        let token_mint_quote: Mint = self.pair_info.y_mint.clone();

        let mut total_amount_x = 0u64;
        let mut total_amount_y = 0u64;
        let mut total_fee_x_pending = 0u64;
        let mut total_fee_y_pending = 0u64;

        let lower_bin_array_idx = BinArray::bin_id_to_bin_array_index(self.lower_bin_id)?;
        let upper_bin_array_idx = lower_bin_array_idx.checked_add(1).context("MathOverflow")?;

        let mut bin_arrays = vec![];
        for i in lower_bin_array_idx..=upper_bin_array_idx {
            let (bin_array, _bump) = derive_bin_array_pda(lb_pair, i.into());

            // match program.account::<BinArray>(bin_array).await {
            match program_adapter::get_bin_array(bin_array, &program).await {
                Ok(bin_array_state) => bin_arrays.push(bin_array_state),
                Err(err) => {
                    println!("get bin array err: {}", err);
                }
            }
        }
        let bin_array_manager = BinArrayManager {
            bin_arrays: &bin_arrays,
        };
        for (i, &share) in position_state.liquidity_shares.iter().enumerate() {
            if share == 0 {
                continue;
            }
            let bin_id = position_state.from_idx_to_bin_id(i)?;
            let bin = bin_array_manager.get_bin(bin_id)?;
            let (amount_x, amount_y) = bin.calculate_out_amount(share)?;
            total_amount_x = total_amount_x.safe_add(amount_x).unwrap();
            total_amount_y = total_amount_y.safe_add(amount_y).unwrap();
        }

        let (fee_x_pending, fee_y_pending) =
            bin_array_manager.get_total_fee_pending(&position_state)?;
        total_fee_x_pending = total_fee_x_pending.checked_add(fee_x_pending).unwrap();
        total_fee_y_pending = total_fee_y_pending.checked_add(fee_y_pending).unwrap();

        let total_amount_x =
            total_amount_x as f64 / (10u64.pow(token_mint_base.decimals as u32) as f64);
        let total_amount_y =
            total_amount_y as f64 / (10u64.pow(token_mint_quote.decimals as u32) as f64);

        let total_fee_x_pending =
            total_fee_x_pending as f64 / (10u64.pow(token_mint_base.decimals as u32) as f64);
        let total_fee_y_pending =
            total_fee_y_pending as f64 / (10u64.pow(token_mint_quote.decimals as u32) as f64);

        self.balance = Some((
            total_amount_x,
            total_amount_y,
            total_fee_x_pending,
            total_fee_y_pending,
        ));
        Ok(())
    }

    pub fn get_pair_names(&self) -> Option<(String, String)> {
        if let Some(pair_name) = &self.pair_name {
            if let Some((x, y)) = pair_name.split_once('-') {
                Some((x.to_string(), y.to_string()))
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn with_price(&mut self) -> Result<()> {
        let prices = get_token_prices(&[self.token_x_mint, self.token_y_mint]).await?;
        let (x_price, y_price) = (prices[0].1, prices[1].1);

        self.token_x_price = x_price;
        self.token_y_price = y_price;

        Ok(())
    }
}
