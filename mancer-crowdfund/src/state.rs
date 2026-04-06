use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

// borsh size of Campaign: 32 + 8 + 8 + 8 + 1 + 1
pub const CAMPAIGN_SIZE: usize = 58;

// borsh size of ContributionRecord: 32 + 8
pub const CONTRIBUTION_SIZE: usize = 40;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Campaign {
    pub creator: Pubkey,
    pub goal: u64,
    pub raised: u64,
    pub deadline: i64,
    pub claimed: bool,
    pub vault_bump: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ContributionRecord {
    pub donor: Pubkey,
    // zeroed after a successful refund
    pub amount: u64,
}
