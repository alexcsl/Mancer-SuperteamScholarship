use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program::invoke,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    system_program,
    sysvar::Sysvar,
};

use crate::{
    error::CrowdfundError,
    instruction::CrowdfundInstruction,
    state::{Campaign, ContributionRecord, CAMPAIGN_SIZE, CONTRIBUTION_SIZE},
};

pub struct Processor;

impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = CrowdfundInstruction::unpack(instruction_data)?;
        match instruction {
            CrowdfundInstruction::CreateCampaign { goal, deadline } => {
                Self::process_create_campaign(program_id, accounts, goal, deadline)
            }
            CrowdfundInstruction::Contribute { amount } => {
                Self::process_contribute(program_id, accounts, amount)
            }
            CrowdfundInstruction::Withdraw => Self::process_withdraw(program_id, accounts),
            CrowdfundInstruction::Refund => Self::process_refund(program_id, accounts),
        }
    }

    // accounts: [creator, campaign_account, vault_pda, system_program]
    fn process_create_campaign(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        goal: u64,
        deadline: i64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let creator = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if system_program_account.key != &system_program::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        if goal == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        let clock = Clock::get()?;
        if deadline <= clock.unix_timestamp {
            return Err(CrowdfundError::DeadlineHasPassed.into());
        }

        // block reinitialization
        if campaign_account.lamports() != 0 || !campaign_account.data_is_empty() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        let (vault_key, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        let rent = Rent::get()?;

        invoke(
            &system_instruction::create_account(
                creator.key,
                campaign_account.key,
                rent.minimum_balance(CAMPAIGN_SIZE),
                CAMPAIGN_SIZE as u64,
                program_id,
            ),
            &[
                creator.clone(),
                campaign_account.clone(),
                system_program_account.clone(),
            ],
        )?;

        invoke_signed(
            &system_instruction::create_account(
                creator.key,
                vault_pda.key,
                rent.minimum_balance(0),
                0,
                &system_program::ID,
            ),
            &[
                creator.clone(),
                vault_pda.clone(),
                system_program_account.clone(),
            ],
            &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
        )?;

        let campaign = Campaign {
            creator: *creator.key,
            goal,
            raised: 0,
            deadline,
            claimed: false,
            vault_bump,
        };
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Campaign created: goal={}, deadline={}", goal, deadline);

        Ok(())
    }

    // accounts: [donor, campaign_account, vault_pda, contribution_pda, system_program]
    fn process_contribute(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        amount: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let donor = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if system_program_account.key != &system_program::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        if amount == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        let clock = Clock::get()?;
        if clock.unix_timestamp >= campaign.deadline {
            return Err(CrowdfundError::DeadlineHasPassed.into());
        }

        let (vault_key, _) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        let (contrib_key, contrib_bump) = Pubkey::find_program_address(
            &[
                b"contribution",
                campaign_account.key.as_ref(),
                donor.key.as_ref(),
            ],
            program_id,
        );
        if contribution_pda.key != &contrib_key {
            return Err(CrowdfundError::InvalidContribPda.into());
        }

        let rent = Rent::get()?;

        if contribution_pda.lamports() == 0 {
            // first contribution from this donor
            invoke_signed(
                &system_instruction::create_account(
                    donor.key,
                    contribution_pda.key,
                    rent.minimum_balance(CONTRIBUTION_SIZE),
                    CONTRIBUTION_SIZE as u64,
                    program_id,
                ),
                &[
                    donor.clone(),
                    contribution_pda.clone(),
                    system_program_account.clone(),
                ],
                &[&[
                    b"contribution",
                    campaign_account.key.as_ref(),
                    donor.key.as_ref(),
                    &[contrib_bump],
                ]],
            )?;
            let record = ContributionRecord {
                donor: *donor.key,
                amount,
            };
            record.serialize(&mut *contribution_pda.try_borrow_mut_data()?)?;
        } else {
            // add to existing record
            let mut record =
                ContributionRecord::try_from_slice(&contribution_pda.try_borrow_data()?)?;
            if record.donor != *donor.key {
                return Err(CrowdfundError::NotDonor.into());
            }
            record.amount = record
                .amount
                .checked_add(amount)
                .ok_or(CrowdfundError::ArithmeticOverflow)?;
            record.serialize(&mut *contribution_pda.try_borrow_mut_data()?)?;
        }

        invoke(
            &system_instruction::transfer(donor.key, vault_pda.key, amount),
            &[
                donor.clone(),
                vault_pda.clone(),
                system_program_account.clone(),
            ],
        )?;

        campaign.raised = campaign
            .raised
            .checked_add(amount)
            .ok_or(CrowdfundError::ArithmeticOverflow)?;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Contributed: {} lamports, total={}", amount, campaign.raised);

        Ok(())
    }

    // accounts: [creator, campaign_account, vault_pda, system_program]
    fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let creator = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if system_program_account.key != &system_program::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        if creator.key != &campaign.creator {
            return Err(CrowdfundError::NotCreator.into());
        }

        if campaign.claimed {
            return Err(CrowdfundError::AlreadyClaimed.into());
        }

        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundError::DeadlineNotReached.into());
        }

        if campaign.raised < campaign.goal {
            return Err(CrowdfundError::GoalNotReached.into());
        }

        let vault_key = Pubkey::create_program_address(
            &[
                b"vault",
                campaign_account.key.as_ref(),
                &[campaign.vault_bump],
            ],
            program_id,
        )
        .map_err(|_| CrowdfundError::InvalidVaultPda)?;
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        let rent = Rent::get()?;
        let vault_balance = vault_pda.lamports();
        let withdraw_amount = vault_balance
            .checked_sub(rent.minimum_balance(0))
            .ok_or(ProgramError::InsufficientFunds)?;

        if withdraw_amount == 0 {
            return Err(ProgramError::InsufficientFunds);
        }

        invoke_signed(
            &system_instruction::transfer(vault_pda.key, creator.key, withdraw_amount),
            &[
                vault_pda.clone(),
                creator.clone(),
                system_program_account.clone(),
            ],
            &[&[
                b"vault",
                campaign_account.key.as_ref(),
                &[campaign.vault_bump],
            ]],
        )?;

        campaign.claimed = true;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Withdrawn: {} lamports", withdraw_amount);

        Ok(())
    }

    // accounts: [donor, campaign_account, vault_pda, contribution_pda, system_program]
    fn process_refund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let donor = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if system_program_account.key != &system_program::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundError::DeadlineNotReached.into());
        }

        if campaign.raised >= campaign.goal {
            return Err(CrowdfundError::GoalAlreadyReached.into());
        }

        let vault_key = Pubkey::create_program_address(
            &[
                b"vault",
                campaign_account.key.as_ref(),
                &[campaign.vault_bump],
            ],
            program_id,
        )
        .map_err(|_| CrowdfundError::InvalidVaultPda)?;
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        let (contrib_key, _) = Pubkey::find_program_address(
            &[
                b"contribution",
                campaign_account.key.as_ref(),
                donor.key.as_ref(),
            ],
            program_id,
        );
        if contribution_pda.key != &contrib_key {
            return Err(CrowdfundError::InvalidContribPda.into());
        }

        let mut record =
            ContributionRecord::try_from_slice(&contribution_pda.try_borrow_data()?)?;

        if record.donor != *donor.key {
            return Err(CrowdfundError::NotDonor.into());
        }

        if record.amount == 0 {
            return Err(CrowdfundError::NothingToRefund.into());
        }

        let refund_amount = record.amount;

        invoke_signed(
            &system_instruction::transfer(vault_pda.key, donor.key, refund_amount),
            &[
                vault_pda.clone(),
                donor.clone(),
                system_program_account.clone(),
            ],
            &[&[
                b"vault",
                campaign_account.key.as_ref(),
                &[campaign.vault_bump],
            ]],
        )?;

        // zero out to prevent a second refund
        record.amount = 0;
        record.serialize(&mut *contribution_pda.try_borrow_mut_data()?)?;

        campaign.raised = campaign
            .raised
            .checked_sub(refund_amount)
            .ok_or(CrowdfundError::ArithmeticOverflow)?;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Refunded: {} lamports", refund_amount);

        Ok(())
    }
}
