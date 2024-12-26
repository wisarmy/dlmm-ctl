use anchor_client::solana_client::{
    nonblocking::rpc_client::RpcClient, rpc_request::TokenAccountsFilter,
};
use anchor_lang::prelude::Pubkey;
use anchor_spl::token::spl_token;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_account_decoder::UiAccountData;
use tracing::trace;

pub type TokenAccounts = Vec<TokenAccount>;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenAccount {
    pub pubkey: String,
    pub mint: String,
    pub amount: String,
    pub ui_amount: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ParsedAccount {
    program: String,
    parsed: Parsed,
    space: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Parsed {
    info: TokenInfo,
    #[serde(rename = "type")]
    account_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenInfo {
    is_native: bool,
    mint: String,
    owner: String,
    state: String,
    token_amount: Amount,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Amount {
    amount: String,
    decimals: u8,
    ui_amount: f64,
    ui_amount_string: String,
}

pub async fn token_account(
    client: &RpcClient,
    owner: &Pubkey,
    mint: Pubkey,
) -> Result<TokenAccount> {
    let token_accounts =
        token_accounts_filter(client, owner, TokenAccountsFilter::Mint(mint)).await?;
    token_accounts
        .first()
        .cloned()
        .ok_or(anyhow!("NotFound: token account not found"))
}

pub async fn token_accounts(client: &RpcClient, owner: &Pubkey) -> Result<TokenAccounts> {
    token_accounts_filter(
        client,
        owner,
        TokenAccountsFilter::ProgramId(spl_token::id()),
    )
    .await
}

pub async fn token_accounts_filter(
    client: &RpcClient,
    owner: &Pubkey,
    filter: TokenAccountsFilter,
) -> Result<TokenAccounts> {
    let token_accounts = client
        .get_token_accounts_by_owner(owner, filter)
        .await
        .expect("Failed to get token accounts");

    trace!("token_accounts: {:#?}", token_accounts);

    let mut tas: TokenAccounts = vec![];
    for token_account in token_accounts.into_iter() {
        let account_data = token_account.account.data;
        match account_data {
            UiAccountData::Json(parsed_account) => {
                let parsed: Parsed = serde_json::from_value(parsed_account.parsed)?;
                tas.push(TokenAccount {
                    pubkey: token_account.pubkey,
                    mint: parsed.info.mint,
                    amount: parsed.info.token_amount.amount,
                    ui_amount: parsed.info.token_amount.ui_amount,
                });
            }
            UiAccountData::LegacyBinary(_) | UiAccountData::Binary(_, _) => {
                continue;
            }
        }
    }

    Ok(tas)
}
