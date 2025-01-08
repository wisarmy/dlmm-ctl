use anchor_lang::prelude::*;
use anyhow::Result;
use lb_clmm::state::bin::Bin;
use lb_clmm::{
    constants::{MAX_BIN_PER_ARRAY, MAX_BIN_PER_POSITION},
    state::bin::BinArray,
    state::position::{FeeInfo, PositionV2, UserRewardInfo},
};

pub fn parse_position_data(data: &[u8]) -> Result<PositionV2> {
    // Helper function to parse data of a specific type from a slice
    fn parse_slice<T: AnchorDeserialize>(data: &[u8]) -> Result<T> {
        T::try_from_slice(data).map_err(anyhow::Error::from)
    }

    let start = 8; // skip discriminator

    // Parse the liquidity_shares array
    let mut liquidity_shares = [0u128; MAX_BIN_PER_POSITION];
    for i in 0..MAX_BIN_PER_POSITION {
        liquidity_shares[i] = u128::from_le_bytes(
            data[start + 64 + i * 16..start + 64 + (i + 1) * 16]
                .try_into()
                .unwrap(),
        );
    }

    // Parse reward_infos array
    let reward_start = start + 64 + MAX_BIN_PER_POSITION * 16;
    let mut reward_infos = [UserRewardInfo::default(); MAX_BIN_PER_POSITION];
    for i in 0..MAX_BIN_PER_POSITION {
        reward_infos[i] = parse_slice(&data[reward_start + i * 48..reward_start + (i + 1) * 48])?;
    }

    // Parse fee_infos array
    let fee_start = reward_start + MAX_BIN_PER_POSITION * 48;
    let mut fee_infos = [FeeInfo::default(); MAX_BIN_PER_POSITION];
    for i in 0..MAX_BIN_PER_POSITION {
        fee_infos[i] = parse_slice(&data[fee_start + i * 48..fee_start + (i + 1) * 48])?;
    }

    // Parse the total_claimed_rewards array
    let claimed_rewards_start = fee_start + MAX_BIN_PER_POSITION * 48 + 32;
    let mut total_claimed_rewards = [0u64; 2];
    for i in 0..2 {
        total_claimed_rewards[i] =
            parse_slice(&data[claimed_rewards_start + i * 8..claimed_rewards_start + (i + 1) * 8])?;
    }

    let position = PositionV2 {
        lb_pair: parse_slice(&data[start..start + 32])?,
        owner: parse_slice(&data[start + 32..start + 64])?,
        liquidity_shares,
        reward_infos,
        fee_infos,
        lower_bin_id: parse_slice(
            &data[fee_start + MAX_BIN_PER_POSITION * 48..fee_start + MAX_BIN_PER_POSITION * 48 + 4],
        )?,
        upper_bin_id: parse_slice(
            &data[fee_start + MAX_BIN_PER_POSITION * 48 + 4
                ..fee_start + MAX_BIN_PER_POSITION * 48 + 8],
        )?,
        last_updated_at: parse_slice(
            &data[fee_start + MAX_BIN_PER_POSITION * 48 + 8
                ..fee_start + MAX_BIN_PER_POSITION * 48 + 16],
        )?,
        total_claimed_fee_x_amount: parse_slice(
            &data[fee_start + MAX_BIN_PER_POSITION * 48 + 16
                ..fee_start + MAX_BIN_PER_POSITION * 48 + 24],
        )?,
        total_claimed_fee_y_amount: parse_slice(
            &data[fee_start + MAX_BIN_PER_POSITION * 48 + 24
                ..fee_start + MAX_BIN_PER_POSITION * 48 + 32],
        )?,
        total_claimed_rewards,
        operator: parse_slice(&data[claimed_rewards_start + 16..claimed_rewards_start + 48])?,
        lock_release_point: parse_slice(
            &data[claimed_rewards_start + 48..claimed_rewards_start + 56],
        )?,
        _padding_0: parse_slice(&data[claimed_rewards_start + 56..claimed_rewards_start + 57])?,
        fee_owner: parse_slice(&data[claimed_rewards_start + 57..claimed_rewards_start + 89])?,
        _reserved: data[claimed_rewards_start + 89..claimed_rewards_start + 176]
            .try_into()
            .unwrap(),
    };

    Ok(position)
}

#[allow(dead_code)]
pub fn parse_bin_data(data: &[u8]) -> Result<BinArray> {
    // Helper function to parse data of a specific type from a slice
    fn parse_slice<T: AnchorDeserialize>(data: &[u8]) -> Result<T> {
        T::try_from_slice(data).map_err(anyhow::Error::from)
    }

    let start = 8; // skip discriminator

    let index: i64 = parse_slice(&data[start..start + 8])?;
    let version: u8 = parse_slice(&data[start + 8..start + 9])?;
    let _padding: [u8; 7] = data[start + 9..start + 16].try_into()?;
    let lb_pair: Pubkey = parse_slice(&data[start + 16..start + 48])?;

    // Allocate memory for the bins array
    let mut bins = [Bin::default(); MAX_BIN_PER_ARRAY];

    let bins_start = start + 48;
    let bin_size = 144; // Bin size in bytes

    for i in 0..MAX_BIN_PER_ARRAY {
        let bin_start = bins_start + i * bin_size;

        bins[i] = Bin {
            amount_x: parse_slice(&data[bin_start..bin_start + 8])?,
            amount_y: parse_slice(&data[bin_start + 8..bin_start + 16])?,
            price: parse_slice(&data[bin_start + 16..bin_start + 32])?,
            liquidity_supply: parse_slice(&data[bin_start + 32..bin_start + 48])?,
            reward_per_token_stored: [
                parse_slice(&data[bin_start + 48..bin_start + 64])?,
                parse_slice(&data[bin_start + 64..bin_start + 80])?,
            ],
            fee_amount_x_per_token_stored: parse_slice(&data[bin_start + 80..bin_start + 96])?,
            fee_amount_y_per_token_stored: parse_slice(&data[bin_start + 96..bin_start + 112])?,
            amount_x_in: parse_slice(&data[bin_start + 112..bin_start + 128])?,
            amount_y_in: parse_slice(&data[bin_start + 128..bin_start + 144])?,
        };
    }

    Ok(BinArray {
        index,
        version,
        _padding,
        lb_pair,
        bins,
    })
}
