use solana_program::program_error::ProgramError;

// all instructions the program accepts
pub enum CrowdfundInstruction {
    // create a new campaign with a goal and deadline
    CreateCampaign { goal: u64, deadline: i64 },
    // contribute lamports to a campaign
    Contribute { amount: u64 },
    // creator withdraws funds after successful campaign
    Withdraw,
    // donor reclaims funds from a failed campaign
    Refund,
}

impl CrowdfundInstruction {
    // parse instruction bytes into an enum variant
    // byte 0 is the discriminant, remaining bytes are the payload
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (discriminant, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;

        match discriminant {
            // CreateCampaign: goal (8 bytes le) + deadline (8 bytes le)
            0 => {
                if rest.len() < 16 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let goal = u64::from_le_bytes(
                    rest[0..8]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                );
                let deadline = i64::from_le_bytes(
                    rest[8..16]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                );
                Ok(CrowdfundInstruction::CreateCampaign { goal, deadline })
            }
            // Contribute: amount (8 bytes le)
            1 => {
                if rest.len() < 8 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let amount = u64::from_le_bytes(
                    rest[0..8]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                );
                Ok(CrowdfundInstruction::Contribute { amount })
            }
            // Withdraw: no payload
            2 => Ok(CrowdfundInstruction::Withdraw),
            // Refund: no payload
            3 => Ok(CrowdfundInstruction::Refund),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}
