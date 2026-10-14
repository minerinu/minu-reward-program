use anchor_lang::prelude::*;
use anchor_lang::system_program::{self, Transfer};
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
use anchor_spl::token_2022::spl_token_2022::{extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions}, state::Mint as Mint2022};

// Devnet test identity. Mainnet deployment requires a separate reviewed release.
declare_id!("2LfTVADJBFLwj9tR296ZQozB1x1cAT7CtNnAsvVBXys1");
const SCALE: u128 = 1_000_000_000_000_000_000;
const ROUND_SECONDS: i64 = 60;
const ALLOCATION_PERCENT: u128 = 69;
// 64 rounds exhaust every distributable u64 amount down to sub-rounding dust.
const MAX_ROUNDS: i64 = 64;
const LOCKS: [i64; 3] = [14_400, 28_800, 57_600];
// Integer units preserve the exact relative weights 1 / 1.25 / 1.5.
const WEIGHTS: [u128; 3] = [4, 5, 6];

#[program]
pub mod sol_fee_staking {
    use super::*;
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        validate_mint(&ctx.accounts.mint.to_account_info())?;
        require!(ctx.accounts.mint.freeze_authority.is_none(), StakingError::UnsupportedMint);
        let s = &mut ctx.accounts.state;
        s.mint = ctx.accounts.mint.key();
        s.token_program = ctx.accounts.token_program.key();
        s.token_vault = ctx.accounts.token_vault.key();
        s.bump = ctx.bumps.state;
        s.sol_bump = ctx.bumps.sol_vault;
        s.rent_floor = Rent::get()?.minimum_balance(0);
        ctx.accounts.reward_stream.state = s.key();
        ctx.accounts.reward_stream.last_wall = Clock::get()?.unix_timestamp;
        ctx.accounts.reward_stream.bump = ctx.bumps.reward_stream;
        emit!(Initialized { mint: s.mint, state: s.key(), sol_vault: ctx.accounts.sol_vault.key() });
        Ok(())
    }

    // Existing allocations are settled using the original rules before percentage distribution begins.
    pub fn initialize_streaming(ctx: Context<InitializeStreaming>) -> Result<()> {
        sync(&mut ctx.accounts.state, ctx.accounts.sol_vault.lamports())?;
        let stream = &mut ctx.accounts.reward_stream;
        stream.state = ctx.accounts.state.key(); stream.last_wall = Clock::get()?.unix_timestamp;
        stream.bump = ctx.bumps.reward_stream;
        Ok(())
    }

    pub fn sync_rewards(ctx: Context<SyncRewards>) -> Result<()> {
        stream_sync(&mut ctx.accounts.state, &mut ctx.accounts.reward_stream, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)
    }

    pub fn stake(ctx: Context<Stake>, id: u64, pool: u8, amount: u64) -> Result<()> {
        require!(pool < 3, StakingError::InvalidPool);
        require!(amount > 0, StakingError::ZeroAmount);
        validate_mint(&ctx.accounts.mint.to_account_info())?;
        require!(ctx.accounts.mint.freeze_authority.is_none(), StakingError::UnsupportedMint);
        let s = &mut ctx.accounts.state;
        stream_sync(s, &mut ctx.accounts.reward_stream, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        let before = ctx.accounts.token_vault.amount;
        token_interface::transfer_checked(CpiContext::new(ctx.accounts.token_program.to_account_info(), TransferChecked {
            from: ctx.accounts.source.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.token_vault.to_account_info(), authority: ctx.accounts.owner.to_account_info(),
        }), amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.token_vault.reload()?;
        require!(ctx.accounts.token_vault.amount.checked_sub(before) == Some(amount), StakingError::UnsupportedMint);
        let weight = (amount as u128).checked_mul(WEIGHTS[pool as usize]).ok_or(StakingError::Overflow)?;
        s.total_weight = add(s.total_weight, weight)?;
        s.pool_amounts[pool as usize] = s.pool_amounts[pool as usize].checked_add(amount).ok_or(StakingError::Overflow)?;
        let p = &mut ctx.accounts.position;
        p.state = s.key(); p.owner = ctx.accounts.owner.key(); p.id = id; p.pool = pool; p.amount = amount;
        p.weight = weight; p.checkpoint = s.reward_index; p.pending_scaled = 0;
        p.unlock_at = Clock::get()?.unix_timestamp.checked_add(LOCKS[pool as usize]).ok_or(StakingError::Overflow)?;
        p.bump = ctx.bumps.position;
        emit!(Staked { owner: p.owner, position: p.key(), pool, amount, unlock_at: p.unlock_at });
        Ok(())
    }

    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let s = &mut ctx.accounts.state;
        stream_sync(s, &mut ctx.accounts.reward_stream, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        settle(s.reward_index, &mut ctx.accounts.position)?;
        payout(s, &mut ctx.accounts.position, &mut ctx.accounts.vesting, &ctx.accounts.sol_vault, &ctx.accounts.owner, &ctx.accounts.system_program)
    }

    // Withdraw the whole independent position; claims remain available during a lock.
    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        require!(Clock::get()?.unix_timestamp >= ctx.accounts.position.unlock_at, StakingError::Locked);
        require!(ctx.accounts.position.amount > 0, StakingError::ZeroAmount);
        let s = &mut ctx.accounts.state;
        stream_sync(s, &mut ctx.accounts.reward_stream, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        let p = &mut ctx.accounts.position;
        settle(s.reward_index, p)?;
        payout(s, p, &mut ctx.accounts.vesting, &ctx.accounts.sol_vault, &ctx.accounts.owner, &ctx.accounts.system_program)?;
        let amount = p.amount;
        s.total_weight = sub(s.total_weight, p.weight)?;
        s.pool_amounts[p.pool as usize] = s.pool_amounts[p.pool as usize].checked_sub(amount).ok_or(StakingError::Insolvent)?;
        p.amount = 0; p.weight = 0;
        let mint_key = s.mint; let bump = [s.bump];
        let seeds: &[&[u8]] = &[b"state", mint_key.as_ref(), &bump];
        token_interface::transfer_checked(CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), TransferChecked {
            from: ctx.accounts.token_vault.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(), authority: s.to_account_info(),
        }, &[seeds]), amount, ctx.accounts.mint.decimals)?;
        emit!(Withdrawn { owner: p.owner, position: p.key(), amount });
        Ok(())
    }

    // Deterministic migration; legacy allocations are preserved before activating the new clock.
    pub fn initialize_flow(ctx: Context<InitializeFlow>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        stream_sync(&mut ctx.accounts.state, &mut ctx.accounts.reward_stream, ctx.accounts.sol_vault.lamports(), now)?;
        let f = &mut ctx.accounts.reward_flow;
        f.state = ctx.accounts.state.key(); f.bump = ctx.bumps.reward_flow;
        f.seconds_remaining = 60;
        f.budget_remaining = flow_budget(ctx.accounts.reward_stream.reserved);
        ctx.accounts.reward_stream.virtual_time = -1;
        Ok(())
    }
    pub fn sync_rewards_flow(ctx: Context<SyncRewardsFlow>) -> Result<()> {
        flow_sync(&mut ctx.accounts.state, &mut ctx.accounts.reward_stream, &mut ctx.accounts.reward_flow, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)
    }

    pub fn stake_flow(ctx: Context<StakeFlow>, id: u64, pool: u8, amount: u64) -> Result<()> {
        require!(pool < 3, StakingError::InvalidPool);
        require!(amount > 0, StakingError::ZeroAmount);
        validate_mint(&ctx.accounts.mint.to_account_info())?;
        require!(ctx.accounts.mint.freeze_authority.is_none(), StakingError::UnsupportedMint);
        let s = &mut ctx.accounts.state;
        flow_sync(s, &mut ctx.accounts.reward_stream, &mut ctx.accounts.reward_flow, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        let before = ctx.accounts.token_vault.amount;
        token_interface::transfer_checked(CpiContext::new(ctx.accounts.token_program.to_account_info(), TransferChecked {
            from: ctx.accounts.source.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.token_vault.to_account_info(), authority: ctx.accounts.owner.to_account_info(),
        }), amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.token_vault.reload()?;
        require!(ctx.accounts.token_vault.amount.checked_sub(before) == Some(amount), StakingError::UnsupportedMint);
        let weight = (amount as u128).checked_mul(WEIGHTS[pool as usize]).ok_or(StakingError::Overflow)?;
        s.total_weight = add(s.total_weight, weight)?;
        s.pool_amounts[pool as usize] = s.pool_amounts[pool as usize].checked_add(amount).ok_or(StakingError::Overflow)?;
        let p = &mut ctx.accounts.position;
        p.state = s.key(); p.owner = ctx.accounts.owner.key(); p.id = id; p.pool = pool; p.amount = amount;
        p.weight = weight; p.checkpoint = s.reward_index; p.pending_scaled = 0;
        p.unlock_at = Clock::get()?.unix_timestamp.checked_add(LOCKS[pool as usize]).ok_or(StakingError::Overflow)?;
        p.bump = ctx.bumps.position;
        emit!(Staked { owner: p.owner, position: p.key(), pool, amount, unlock_at: p.unlock_at });
        Ok(())
    }

    pub fn claim_flow(ctx: Context<ClaimFlow>) -> Result<()> {
        let s = &mut ctx.accounts.state;
        flow_sync(s, &mut ctx.accounts.reward_stream, &mut ctx.accounts.reward_flow, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        settle(s.reward_index, &mut ctx.accounts.position)?;
        payout(s, &mut ctx.accounts.position, &mut ctx.accounts.vesting, &ctx.accounts.sol_vault, &ctx.accounts.owner, &ctx.accounts.system_program)
    }

    // Withdraw the whole independent position; claims remain available during a lock.
    pub fn withdraw_flow(ctx: Context<WithdrawFlow>) -> Result<()> {
        require!(Clock::get()?.unix_timestamp >= ctx.accounts.position.unlock_at, StakingError::Locked);
        require!(ctx.accounts.position.amount > 0, StakingError::ZeroAmount);
        let s = &mut ctx.accounts.state;
        flow_sync(s, &mut ctx.accounts.reward_stream, &mut ctx.accounts.reward_flow, ctx.accounts.sol_vault.lamports(), Clock::get()?.unix_timestamp)?;
        let p = &mut ctx.accounts.position;
        settle(s.reward_index, p)?;
        payout(s, p, &mut ctx.accounts.vesting, &ctx.accounts.sol_vault, &ctx.accounts.owner, &ctx.accounts.system_program)?;
        let amount = p.amount;
        s.total_weight = sub(s.total_weight, p.weight)?;
        s.pool_amounts[p.pool as usize] = s.pool_amounts[p.pool as usize].checked_sub(amount).ok_or(StakingError::Insolvent)?;
        p.amount = 0; p.weight = 0;
        let mint_key = s.mint; let bump = [s.bump];
        let seeds: &[&[u8]] = &[b"state", mint_key.as_ref(), &bump];
        token_interface::transfer_checked(CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), TransferChecked {
            from: ctx.accounts.token_vault.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(), authority: s.to_account_info(),
        }, &[seeds]), amount, ctx.accounts.mint.decimals)?;
        emit!(Withdrawn { owner: p.owner, position: p.key(), amount });
        Ok(())
    }

    pub fn close_position(ctx: Context<ClosePosition>) -> Result<()> {
        require!(ctx.accounts.position.amount == 0, StakingError::ActivePosition);
        // Any fraction below one lamport remains in the reward vault; never paid to an admin.
        require!(ctx.accounts.position.pending_scaled < SCALE, StakingError::UnclaimedRewards);
        Ok(())
    }
}

fn add(a: u128, b: u128) -> Result<u128> { a.checked_add(b).ok_or_else(|| error!(StakingError::Overflow)) }
fn sub(a: u128, b: u128) -> Result<u128> { a.checked_sub(b).ok_or_else(|| error!(StakingError::Insolvent)) }
fn sync(s: &mut State, balance: u64) -> Result<()> {
    let spendable = balance.checked_sub(s.rent_floor).ok_or(StakingError::Insolvent)?;
    let fresh = spendable.checked_sub(s.liability).ok_or(StakingError::Insolvent)?;
    // With no active stakers, fees stay queued as unallocated SOL.
    if fresh == 0 || s.total_weight == 0 { return Ok(()); }
    let increment = (fresh as u128).checked_mul(SCALE).ok_or(StakingError::Overflow)? / s.total_weight;
    if increment == 0 { return Ok(()); }
    s.reward_index = add(s.reward_index, increment)?;
    s.liability = s.liability.checked_add(fresh).ok_or(StakingError::Overflow)?;
    s.total_funded = s.total_funded.checked_add(fresh).ok_or(StakingError::Overflow)?;
    emit!(RewardsSynced { lamports: fresh, reward_index: s.reward_index });
    Ok(())
}
// Only known, unallocated funding participates in elapsed rounds. New transfers are
// queued after settlement, so a delayed sync cannot backdate recently arrived SOL.
// Stake/withdraw settle before changing weight; repeated calls cannot add rounds.
fn stream_sync(s: &mut State, stream: &mut RewardStream, balance: u64, now: i64) -> Result<()> {
    require!(stream.virtual_time >= 0, StakingError::FlowRequired);
    require!(now >= stream.last_wall, StakingError::ClockRegression);
    let spendable = balance.checked_sub(s.rent_floor).and_then(|v|v.checked_sub(s.liability)).ok_or(StakingError::Insolvent)?;
    require!(spendable >= stream.reserved, StakingError::Insolvent);
    let fresh = spendable - stream.reserved;
    let delta = now.checked_sub(stream.last_wall).ok_or(StakingError::Overflow)?;
    let previous_round = stream.virtual_time / ROUND_SECONDS;
    if s.total_weight > 0 { stream.virtual_time = stream.virtual_time.checked_add(delta).ok_or(StakingError::Overflow)?; }
    stream.last_wall = now;
    let rounds = (stream.virtual_time / ROUND_SECONDS - previous_round).min(MAX_ROUNDS);
    for _ in 0..rounds {
        let released = ((stream.reserved as u128) * ALLOCATION_PERCENT / 100) as u64;
        if released == 0 { break; }
        let increment = (released as u128).checked_mul(SCALE).ok_or(StakingError::Overflow)? / s.total_weight;
        // Preserve funds if the reward index cannot represent this distribution.
        if increment == 0 { break; }
        stream.reserved = stream.reserved.checked_sub(released).ok_or(StakingError::Insolvent)?;
        s.reward_index = add(s.reward_index, increment)?;
        s.liability = s.liability.checked_add(released).ok_or(StakingError::Overflow)?;
        emit!(RewardsSynced { lamports: released, reward_index: s.reward_index });
    }
    if fresh > 0 {
        stream.reserved = stream.reserved.checked_add(fresh).ok_or(StakingError::Overflow)?;
        s.total_funded = s.total_funded.checked_add(fresh).ok_or(StakingError::Overflow)?;
        emit!(RewardsQueued { lamports: fresh, allocation_percent: 69, round_seconds: ROUND_SECONDS });
    }
    Ok(())
}
// Each minute's 1% budget is released over its 60 active seconds. Weight changes
// checkpoint elapsed earnings before changing the denominator. No participant loops.
// Deposits observed now start contributing now; never assigned to past seconds.
fn flow_budget(reserve: u64) -> u64 { reserve / 100 }
fn flow_sync(s: &mut State, stream: &mut RewardStream, f: &mut RewardFlow, balance: u64, now: i64) -> Result<()> {
    require!(stream.virtual_time == -1, StakingError::FlowRequired);
    require!(now >= stream.last_wall, StakingError::ClockRegression);
    require!(f.seconds_remaining > 0 && f.seconds_remaining <= 60 && f.budget_remaining <= stream.reserved, StakingError::Insolvent);
    let spendable = balance.checked_sub(s.rent_floor).and_then(|v| v.checked_sub(s.liability)).ok_or(StakingError::Insolvent)?;
    require!(spendable >= stream.reserved, StakingError::Insolvent);
    let fresh = spendable - stream.reserved;
    let mut elapsed = if s.total_weight > 0 { now.checked_sub(stream.last_wall).ok_or(StakingError::Overflow)? } else { 0 };
    stream.last_wall = now;
    let mut total_released: u64 = 0;
    // All possible u64 budgets reach zero within 4,600 minutes at 1% decay.
    for _ in 0..4601 {
        if elapsed == 0 { break; }
        if f.budget_remaining == 0 && stream.reserved < 100 {
            // May have a partial-minute budget rounded to zero: skip that minute,
            // then still preserve/distribute the full-minute budget if representable.
            if flow_budget(stream.reserved) == 0 {
                let remainder = elapsed % 60;
                f.seconds_remaining = ((f.seconds_remaining as i64 - remainder - 1).rem_euclid(60) + 1) as u8;
                break;
            }
        }
        let dt = elapsed.min(f.seconds_remaining as i64);
        let released = if dt == f.seconds_remaining as i64 { f.budget_remaining }
            else { ((f.budget_remaining as u128) * dt as u128 / f.seconds_remaining as u128) as u64 };
        if released > 0 {
            stream.reserved = stream.reserved.checked_sub(released).ok_or(StakingError::Insolvent)?;
            f.budget_remaining = f.budget_remaining.checked_sub(released).ok_or(StakingError::Insolvent)?;
            total_released = total_released.checked_add(released).ok_or(StakingError::Overflow)?;
        }
        elapsed -= dt; f.seconds_remaining -= dt as u8;
        if f.seconds_remaining == 0 {
            f.seconds_remaining = 60;
            f.budget_remaining = flow_budget(stream.reserved);
        }
    }
    require!(elapsed == 0 || flow_budget(stream.reserved) == 0, StakingError::StreamCapacity);
    // The denominator is constant throughout this sync. Aggregate before the
    // wide division to keep multi-day catch-up within the transaction budget.
    if total_released > 0 {
        let increment = (total_released as u128).checked_mul(SCALE).ok_or(StakingError::Overflow)? / s.total_weight;
        s.reward_index = add(s.reward_index, increment)?;
        s.liability = s.liability.checked_add(total_released).ok_or(StakingError::Overflow)?;
        emit!(RewardsSynced { lamports: total_released, reward_index: s.reward_index });
    }
    if fresh > 0 {
        stream.reserved = stream.reserved.checked_add(fresh).ok_or(StakingError::Overflow)?;
        s.total_funded = s.total_funded.checked_add(fresh).ok_or(StakingError::Overflow)?;
        let extra = ((fresh as u128) * 100 * f.seconds_remaining as u128 / 600_000) as u64;
        f.budget_remaining = f.budget_remaining.checked_add(extra).ok_or(StakingError::Overflow)?;
        emit!(FlowRewardsQueued { lamports: fresh, allocation_bps: 100, round_seconds: 60 });
    }
    Ok(())
}
fn settle(index: u128, p: &mut Position) -> Result<()> {
    let delta = sub(index, p.checkpoint)?;
    p.pending_scaled = add(p.pending_scaled, p.weight.checked_mul(delta).ok_or(StakingError::Overflow)?)?;
    p.checkpoint = index;
    Ok(())
}
fn payout<'info>(s: &mut Account<'info, State>, p: &mut Account<'info, Position>, vesting: &mut Account<'info, Vesting>, vault: &SystemAccount<'info>, owner: &Signer<'info>, system: &Program<'info, System>) -> Result<()> {
    let amount = earned_claimable(p)?;
    if amount == 0 { return Ok(()); }
    p.pending_scaled = sub(p.pending_scaled, (amount as u128).checked_mul(SCALE).ok_or(StakingError::Overflow)?)?;
    vesting.claimed = vesting.claimed.checked_add(amount).ok_or(StakingError::Overflow)?;
    s.liability = s.liability.checked_sub(amount).ok_or(StakingError::Insolvent)?;
    s.total_claimed = s.total_claimed.checked_add(amount).ok_or(StakingError::Overflow)?;
    let state_key = s.key(); let bump = [s.sol_bump];
    let seeds: &[&[u8]] = &[b"sol", state_key.as_ref(), &bump];
    system_program::transfer(CpiContext::new_with_signer(system.to_account_info(), Transfer {
        from: vault.to_account_info(), to: owner.to_account_info(),
    }, &[seeds]), amount)?;
    emit!(Claimed { owner: owner.key(), position: p.key(), lamports: amount });
    Ok(())
}
// All whole lamports already earned are immediately claimable.
// Preserve the existing ledger and account layouts: paid rewards stay paid,
// pending_scaled holds only unpaid rewards, and MINU withdrawal locks stay intact.
fn earned_claimable(p: &Position) -> Result<u64> {
    u64::try_from(p.pending_scaled / SCALE).map_err(|_| error!(StakingError::Overflow))
}
fn validate_mint(info: &AccountInfo) -> Result<()> {
    if *info.owner == anchor_spl::token::ID { return Ok(()); }
    require_keys_eq!(*info.owner, anchor_spl::token_2022::ID, StakingError::UnsupportedMint);
    let data = info.try_borrow_data()?;
    let mint = StateWithExtensions::<Mint2022>::unpack(&data).map_err(|_| error!(StakingError::UnsupportedMint))?;
    for ext in mint.get_extension_types().map_err(|_| error!(StakingError::UnsupportedMint))? {
        require!(matches!(ext, ExtensionType::MetadataPointer | ExtensionType::TokenMetadata), StakingError::UnsupportedMint);
    }
    Ok(())
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mint::token_program = token_program)] pub mint: InterfaceAccount<'info, Mint>,
    #[account(init, payer = payer, space = 8 + State::INIT_SPACE, seeds = [b"state", mint.key().as_ref()], bump)] pub state: Account<'info, State>,
    #[account(init, payer = payer, token::mint = mint, token::authority = state, token::token_program = token_program,
        seeds = [b"tokens", state.key().as_ref()], bump)] pub token_vault: InterfaceAccount<'info, TokenAccount>,
    /// CHECK: Empty system-owned PDA; only this program can sign transfers from it.
    #[account(init, payer = payer, space = 0, owner = system_program.key(), seeds = [b"sol", state.key().as_ref()], bump)] pub sol_vault: UncheckedAccount<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    #[account(init, payer = payer, space = 8 + RewardStream::INIT_SPACE, seeds = [b"stream", state.key().as_ref()], bump)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
pub struct InitializeStreaming<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(init, payer = payer, space = 8 + RewardStream::INIT_SPACE, seeds = [b"stream", state.key().as_ref()], bump)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
pub struct SyncRewards<'info> {
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
#[instruction(id: u64)]
pub struct Stake<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump, has_one = mint, has_one = token_vault, has_one = token_program)] pub state: Account<'info, State>,
    #[account(mint::token_program = token_program)] pub mint: InterfaceAccount<'info, Mint>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = owner, token::token_program = token_program)] pub source: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = state, token::token_program = token_program)] pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(init, payer = owner, space = 8 + Position::INIT_SPACE, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &id.to_le_bytes()], bump)] pub position: Account<'info, Position>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(mut, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &position.id.to_le_bytes()], bump = position.bump, has_one = owner, has_one = state)] pub position: Account<'info, Position>,
    #[account(mut, seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = 8 + Vesting::INIT_SPACE,
        seeds = [b"vesting", position.key().as_ref()], bump)] pub vesting: Account<'info, Vesting>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump, has_one = mint, has_one = token_vault, has_one = token_program)] pub state: Account<'info, State>,
    #[account(mut, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &position.id.to_le_bytes()], bump = position.bump, has_one = owner, has_one = state)] pub position: Account<'info, Position>,
    #[account(mut, seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mint::token_program = token_program)] pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = state, token::token_program = token_program)] pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = owner, token::token_program = token_program)] pub destination: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = 8 + Vesting::INIT_SPACE,
        seeds = [b"vesting", position.key().as_ref()], bump)] pub vesting: Account<'info, Vesting>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
}
#[derive(Accounts)]
pub struct ClosePosition<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(mut, close = owner, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &position.id.to_le_bytes()], bump = position.bump, has_one = owner, has_one = state)] pub position: Account<'info, Position>,
    #[account(mut, close = owner, seeds = [b"vesting", position.key().as_ref()], bump)] pub vesting: Account<'info, Vesting>,
}
#[derive(Accounts)]
pub struct InitializeFlow<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
    #[account(init, payer = payer, space = 8 + RewardFlow::INIT_SPACE, seeds = [b"flow", state.key().as_ref()], bump)] pub reward_flow: Box<Account<'info, RewardFlow>>,
}
#[derive(Accounts)]
pub struct SyncRewardsFlow<'info> {
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
    #[account(mut, seeds = [b"flow", state.key().as_ref()], bump = reward_flow.bump, has_one = state)] pub reward_flow: Box<Account<'info, RewardFlow>>,
}
#[derive(Accounts)]
#[instruction(id: u64)]
pub struct StakeFlow<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump, has_one = mint, has_one = token_vault, has_one = token_program)] pub state: Account<'info, State>,
    #[account(mint::token_program = token_program)] pub mint: InterfaceAccount<'info, Mint>,
    #[account(seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = owner, token::token_program = token_program)] pub source: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = state, token::token_program = token_program)] pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(init, payer = owner, space = 8 + Position::INIT_SPACE, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &id.to_le_bytes()], bump)] pub position: Account<'info, Position>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
    #[account(mut, seeds = [b"flow", state.key().as_ref()], bump = reward_flow.bump, has_one = state)] pub reward_flow: Box<Account<'info, RewardFlow>>,
}
#[derive(Accounts)]
pub struct ClaimFlow<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump)] pub state: Account<'info, State>,
    #[account(mut, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &position.id.to_le_bytes()], bump = position.bump, has_one = owner, has_one = state)] pub position: Account<'info, Position>,
    #[account(mut, seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = 8 + Vesting::INIT_SPACE,
        seeds = [b"vesting", position.key().as_ref()], bump)] pub vesting: Account<'info, Vesting>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
    #[account(mut, seeds = [b"flow", state.key().as_ref()], bump = reward_flow.bump, has_one = state)] pub reward_flow: Box<Account<'info, RewardFlow>>,
}
#[derive(Accounts)]
pub struct WithdrawFlow<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    #[account(mut, seeds = [b"state", state.mint.as_ref()], bump = state.bump, has_one = mint, has_one = token_vault, has_one = token_program)] pub state: Account<'info, State>,
    #[account(mut, seeds = [b"position", state.key().as_ref(), owner.key().as_ref(), &position.id.to_le_bytes()], bump = position.bump, has_one = owner, has_one = state)] pub position: Account<'info, Position>,
    #[account(mut, seeds = [b"sol", state.key().as_ref()], bump = state.sol_bump)] pub sol_vault: SystemAccount<'info>,
    #[account(mint::token_program = token_program)] pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = state, token::token_program = token_program)] pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = owner, token::token_program = token_program)] pub destination: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = 8 + Vesting::INIT_SPACE,
        seeds = [b"vesting", position.key().as_ref()], bump)] pub vesting: Account<'info, Vesting>,
    #[account(mut, seeds = [b"stream", state.key().as_ref()], bump = reward_stream.bump, has_one = state)] pub reward_stream: Box<Account<'info, RewardStream>>,
    #[account(mut, seeds = [b"flow", state.key().as_ref()], bump = reward_flow.bump, has_one = state)] pub reward_flow: Box<Account<'info, RewardFlow>>,
}
#[account]
#[derive(InitSpace)]
pub struct State {
    pub mint: Pubkey, pub token_program: Pubkey, pub token_vault: Pubkey,
    pub bump: u8, pub sol_bump: u8, pub pool_amounts: [u64; 3],
    pub total_weight: u128, pub reward_index: u128, pub liability: u64,
    pub total_funded: u64, pub total_claimed: u64, pub rent_floor: u64,
}
#[account]
#[derive(InitSpace)]
pub struct Position {
    pub state: Pubkey, pub owner: Pubkey, pub id: u64, pub pool: u8, pub amount: u64,
    pub weight: u128, pub checkpoint: u128, pub pending_scaled: u128, pub unlock_at: i64, pub bump: u8,
}
#[account]
#[derive(InitSpace)]
pub struct Vesting { pub claimed: u64 }
#[account]
#[derive(InitSpace)]
pub struct RewardStream {
    pub state: Pubkey, pub last_wall: i64, pub virtual_time: i64, pub reserved: u64,
    pub bump: u8,
}
#[account]
#[derive(InitSpace)]
pub struct RewardFlow { pub state: Pubkey, pub budget_remaining: u64, pub seconds_remaining: u8, pub bump: u8 }
#[event] pub struct FlowRewardsQueued { pub lamports: u64, pub allocation_bps: u16, pub round_seconds: i64 }
#[event] pub struct RewardsQueued { pub lamports: u64, pub allocation_percent: u8, pub round_seconds: i64 }
#[event] pub struct Initialized { pub mint: Pubkey, pub state: Pubkey, pub sol_vault: Pubkey }
#[event] pub struct Staked { pub owner: Pubkey, pub position: Pubkey, pub pool: u8, pub amount: u64, pub unlock_at: i64 }
#[event] pub struct RewardsSynced { pub lamports: u64, pub reward_index: u128 }
#[event] pub struct Claimed { pub owner: Pubkey, pub position: Pubkey, pub lamports: u64 }
#[event] pub struct Withdrawn { pub owner: Pubkey, pub position: Pubkey, pub amount: u64 }
#[error_code]
pub enum StakingError {
    #[msg("Invalid pool")] InvalidPool,
    #[msg("Amount must be positive")] ZeroAmount,
    #[msg("Tokens are still locked")] Locked,
    #[msg("Arithmetic overflow")] Overflow,
    #[msg("Vault accounting mismatch")] Insolvent,
    #[msg("Mint extensions are unsupported")] UnsupportedMint,
    #[msg("Position still has staked tokens")] ActivePosition,
    #[msg("Claim rewards first")] UnclaimedRewards,
    #[msg("Clock moved backwards")] ClockRegression,
    #[msg("Reward stream capacity exceeded")] StreamCapacity,
    #[msg("Use the active reward flow instruction")] FlowRequired,
}

#[cfg(test)]
mod tests {
 use super::*;
 fn state() -> State { State { mint: Pubkey::default(), token_program: Pubkey::default(), token_vault: Pubkey::default(), bump: 0, sol_bump: 0, pool_amounts: [0;3], total_weight: 0, reward_index: 0, liability: 0, total_funded: 0, total_claimed: 0, rent_floor: 890880 } }
 fn pos(weight: u128, checkpoint: u128) -> Position { Position { state: Pubkey::default(), owner: Pubkey::default(), id: 0, pool: 0, amount: 1000, weight, checkpoint, pending_scaled: 0, unlock_at: 0, bump: 0 } }
 #[test] fn account_layouts() { assert_eq!(8+State::INIT_SPACE,194);assert_eq!(8+Position::INIT_SPACE,146); }
 #[test] fn weights_and_locks() { assert_eq!(WEIGHTS,[4,5,6]); assert_eq!(LOCKS,[14400,28800,57600]); }
 #[test] fn proportional_rewards() { let mut s=state(); s.total_weight=15000; sync(&mut s,890880+15_000_000_000).unwrap(); for (w,expected) in [(4000,4_000_000_000u128),(5000,5_000_000_000),(6000,6_000_000_000)] { let mut p=pos(w,0);settle(s.reward_index,&mut p).unwrap(); assert_eq!(p.pending_scaled/SCALE,expected); } assert_eq!(s.liability,15_000_000_000); }
 #[test] fn newcomer_excludes_history() { let mut s=state(); s.total_weight=4000; sync(&mut s,890880+1_000_000_000).unwrap(); let mut p=pos(6000,s.reward_index); settle(s.reward_index,&mut p).unwrap(); assert_eq!(p.pending_scaled,0); s.total_weight+=6000;sync(&mut s,890880+2_000_000_000).unwrap();settle(s.reward_index,&mut p).unwrap();assert_eq!(p.pending_scaled/SCALE,600_000_000); }
 #[test] fn no_double_accounting() { let mut s=state();s.total_weight=4000;sync(&mut s,890880+100).unwrap();let index=s.reward_index;sync(&mut s,890880+100).unwrap();assert_eq!(s.reward_index,index);assert_eq!(s.total_funded,100); }
 #[test] fn no_stakers_queue_fees() { let mut s=state();sync(&mut s,890880+100).unwrap();assert_eq!(s.liability,0);s.total_weight=4;sync(&mut s,890880+100).unwrap();assert_eq!(s.liability,100); }
 #[test] fn accounting_deficit_fails() { let mut s=state();s.liability=100;assert!(sync(&mut s,890880+99).is_err());assert!(sync(&mut s,0).is_err()); }
 #[test] fn overflow_fails() { let mut p=pos(u128::MAX,0);assert!(settle(2,&mut p).is_err());assert!(add(u128::MAX,1).is_err()); }
 #[test] fn settle_is_idempotent() { let mut p=pos(4,0);settle(SCALE/3,&mut p).unwrap();let pending=p.pending_scaled;settle(SCALE/3,&mut p).unwrap();assert_eq!(p.pending_scaled,pending); }
}
