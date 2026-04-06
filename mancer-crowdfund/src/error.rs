use solana_program::program_error::ProgramError;

#[derive(Debug)]
pub enum CrowdfundError {
    // deadline has not been reached yet
    DeadlineNotReached,
    // deadline has already passed
    DeadlineHasPassed,
    // campaign did not reach its goal
    GoalNotReached,
    // campaign already reached its goal so refund is not allowed
    GoalAlreadyReached,
    // funds were already withdrawn
    AlreadyClaimed,
    // caller is not the campaign creator
    NotCreator,
    // caller is not the original donor
    NotDonor,
    // vault PDA does not match expected address
    InvalidVaultPda,
    // contribution PDA does not match expected address
    InvalidContribPda,
    // donor has nothing left to refund
    NothingToRefund,
    // integer overflow in lamport math
    ArithmeticOverflow,
}

impl From<CrowdfundError> for ProgramError {
    fn from(e: CrowdfundError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
