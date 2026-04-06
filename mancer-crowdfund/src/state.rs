use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

// stored in campaign_account, 58 bytes total
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Campaign {
    // wallet that created the campaign
    pub creator: Pubkey,
    // target amount in lamports
    pub goal: u64,
    // total lamports contributed so far
    pub raised: u64,
    // unix timestamp when the campaign ends
    pub deadline: i64,
    // true once the creator has withdrawn funds
    pub claimed: bool,
    // bump seed for the vault PDA
    pub vault_bump: u8,
}

// stored in contribution_pda per donor, 40 bytes total
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ContributionRecord {
    // wallet that made the contribution
    pub donor: Pubkey,
    // total lamports this donor has contributed
    pub amount: u64,
}
