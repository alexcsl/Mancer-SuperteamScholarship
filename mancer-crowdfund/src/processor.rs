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
    state::{Campaign, ContributionRecord},
};

pub struct Processor;

impl Processor {
    // route the instruction to the correct handler
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

    // create a new campaign and its vault PDA
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

        // creator must sign the transaction
        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // goal must be nonzero
        if goal == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        // deadline must be in the future
        let clock = Clock::get()?;
        if deadline <= clock.unix_timestamp {
            return Err(CrowdfundError::DeadlineHasPassed.into());
        }

        // derive vault PDA and verify the account passed in matches
        let (vault_key, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        let rent = Rent::get()?;

        // create the campaign account with space for 58 bytes, owned by this program
        let campaign_space: u64 = 58;
        let campaign_lamports = rent.minimum_balance(campaign_space as usize);
        invoke(
            &system_instruction::create_account(
                creator.key,
                campaign_account.key,
                campaign_lamports,
                campaign_space,
                program_id,
            ),
            &[
                creator.clone(),
                campaign_account.clone(),
                system_program_account.clone(),
            ],
        )?;

        // create the vault PDA with zero data, owned by system program, to hold lamports
        let vault_lamports = rent.minimum_balance(0);
        invoke_signed(
            &system_instruction::create_account(
                creator.key,
                vault_pda.key,
                vault_lamports,
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

        // write the initial campaign state to the campaign account
        let campaign = Campaign {
            creator: *creator.key,
            goal,
            raised: 0,
            deadline,
            claimed: false,
            vault_bump,
        };
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!(
            "Campaign created: goal={}, deadline={}",
            goal,
            deadline
        );

        Ok(())
    }

    // accept a contribution from a donor and record it
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

        // donor must sign the transaction
        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // amount must be nonzero
        if amount == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        // campaign account must be owned by this program
        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        // read the current campaign state
        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        // campaign must still be open
        let clock = Clock::get()?;
        if clock.unix_timestamp >= campaign.deadline {
            return Err(CrowdfundError::DeadlineHasPassed.into());
        }

        // verify the vault PDA matches what is derived from the campaign account
        let (vault_key, _) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &vault_key {
            return Err(CrowdfundError::InvalidVaultPda.into());
        }

        // derive the contribution PDA for this donor and this campaign
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

        // check if the contribution PDA already exists for this donor
        if contribution_pda.lamports() == 0 {
            // first contribution from this donor, create the account
            let contrib_space: u64 = 40;
            let contrib_lamports = rent.minimum_balance(contrib_space as usize);
            invoke_signed(
                &system_instruction::create_account(
                    donor.key,
                    contribution_pda.key,
                    contrib_lamports,
                    contrib_space,
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
            // donor already contributed, add to the existing record
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

        // transfer lamports from donor to vault
        invoke(
            &system_instruction::transfer(donor.key, vault_pda.key, amount),
            &[
                donor.clone(),
                vault_pda.clone(),
                system_program_account.clone(),
            ],
        )?;

        // update the total raised on the campaign
        campaign.raised = campaign
            .raised
            .checked_add(amount)
            .ok_or(CrowdfundError::ArithmeticOverflow)?;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!(
            "Contributed: {} lamports, total={}",
            amount,
            campaign.raised
        );

        Ok(())
    }

    // creator withdraws all funds after a successful campaign
    // accounts: [creator, campaign_account, vault_pda, system_program]
    fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let creator = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        // creator must sign the transaction
        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // campaign account must be owned by this program
        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        // read the current campaign state
        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        // only the original creator can withdraw
        if creator.key != &campaign.creator {
            return Err(CrowdfundError::NotCreator.into());
        }

        // prevent double withdrawal
        if campaign.claimed {
            return Err(CrowdfundError::AlreadyClaimed.into());
        }

        // deadline must have passed
        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundError::DeadlineNotReached.into());
        }

        // goal must have been met
        if campaign.raised < campaign.goal {
            return Err(CrowdfundError::GoalNotReached.into());
        }

        // verify the vault PDA using the stored bump seed
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

        // compute how much to transfer, leaving the rent-exempt floor in the vault
        let rent = Rent::get()?;
        let vault_rent_floor = rent.minimum_balance(0);
        let vault_balance = vault_pda.lamports();
        let withdraw_amount = vault_balance
            .checked_sub(vault_rent_floor)
            .ok_or(ProgramError::InsufficientFunds)?;

        if withdraw_amount == 0 {
            return Err(ProgramError::InsufficientFunds);
        }

        // transfer funds from vault to creator
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

        // mark the campaign as claimed to prevent double withdrawal
        campaign.claimed = true;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Withdrawn: {} lamports", withdraw_amount);

        Ok(())
    }

    // donor reclaims their contribution from a failed campaign
    // accounts: [donor, campaign_account, vault_pda, contribution_pda, system_program]
    fn process_refund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let donor = next_account_info(accounts_iter)?;
        let campaign_account = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        // donor must sign the transaction
        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // campaign account must be owned by this program
        if campaign_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        // read the current campaign state
        let mut campaign = Campaign::try_from_slice(&campaign_account.try_borrow_data()?)?;

        // deadline must have passed before refunds are allowed
        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundError::DeadlineNotReached.into());
        }

        // goal must NOT have been reached for refunds to be valid
        if campaign.raised >= campaign.goal {
            return Err(CrowdfundError::GoalAlreadyReached.into());
        }

        // verify the vault PDA using the stored bump seed
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

        // derive and verify the contribution PDA for this donor
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

        // read the donor's contribution record
        let mut record =
            ContributionRecord::try_from_slice(&contribution_pda.try_borrow_data()?)?;

        // verify this record belongs to the caller
        if record.donor != *donor.key {
            return Err(CrowdfundError::NotDonor.into());
        }

        // nothing to refund if the amount is zero
        if record.amount == 0 {
            return Err(CrowdfundError::NothingToRefund.into());
        }

        let refund_amount = record.amount;

        // transfer the donor's lamports back from the vault
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

        // zero out the record so a second refund call fails with NothingToRefund
        record.amount = 0;
        record.serialize(&mut *contribution_pda.try_borrow_mut_data()?)?;

        // reduce the campaign raised total
        campaign.raised = campaign
            .raised
            .checked_sub(refund_amount)
            .ok_or(CrowdfundError::ArithmeticOverflow)?;
        campaign.serialize(&mut *campaign_account.try_borrow_mut_data()?)?;

        solana_program::msg!("Refunded: {} lamports", refund_amount);

        Ok(())
    }
}
