use std::ops::Deref;

use anchor_client::{solana_sdk::signer::Signer, Program};
use anchor_lang::prelude::*;
use anyhow::Result;
use lb_clmm::state::position::PositionV2;
use tracing::error;

use crate::parser::parse_position_data;

pub async fn get_position<C: Deref<Target = impl Signer> + Clone>(
    position: Pubkey,
    program: &Program<C>,
) -> Result<PositionV2> {
    let position_data = program
        .async_rpc()
        .get_account_data(&position)
        .await
        .inspect_err(|err| {
            error!("get position account err: {}", err);
        })?;
    let position = parse_position_data(&position_data).inspect_err(|err| {
        error!("parse position data err: {}", err);
    })?;
    Ok(position)
}
