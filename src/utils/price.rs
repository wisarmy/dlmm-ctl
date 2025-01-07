use anchor_lang::prelude::Pubkey;
use anyhow::Result;
use serde_json::Value;
use tracing::trace;

pub async fn get_token_prices(token_mints: &[Pubkey]) -> Result<Vec<(Pubkey, Option<f64>)>> {
    let mints_str = token_mints
        .iter()
        .map(|mint| mint.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let price_url = format!("https://api.jup.ag/price/v2?ids={}", mints_str);
    let price_response = reqwest::get(&price_url).await?.json::<Value>().await?;

    trace!("price_response: {:#?}", price_response);

    let prices = token_mints
        .iter()
        .map(|mint| {
            let price = price_response
                .get("data")
                .and_then(|data| data.get(mint.to_string()))
                .and_then(|token| token.get("price"))
                .and_then(|price| price.as_str())
                .and_then(|p| p.parse::<f64>().ok());

            (*mint, price)
        })
        .collect();

    Ok(prices)
}

pub async fn get_sol_price() -> Result<Option<f64>> {
    let prices = get_token_prices(&[anchor_spl::token::spl_token::native_mint::id()]).await?;
    Ok(prices.first().map(|(_, price)| *price).flatten())
}
