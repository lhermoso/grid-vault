use anchor_lang::prelude::*;
use anchor_lang::accounts::account_loader::AccountLoader;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};

declare_id!("521NYDkSEV1htFy6iAkwCfkZrAvaaw7YYDd4dhtfnXQ7");

// Configuration constants
const PERFORMANCE_FEE_BPS: u16 = 2500; // 25%
const FEE_COLLECTION_INTERVAL: i64 = 30 * 24 * 60 * 60; // 30 days in seconds
const TRADING_ALLOCATION_BPS: u16 = 9000; // 90% can be used for trading

/// Helper to calculate user balance (internal)
fn calculate_user_balance_internal(
    config: &ProtocolConfig,
    position: &UserPosition,
    treasury_balance: u64,
) -> Result<(u64, u64, u64)> {
    let deployed_capital = config.total_trading_deployed;
    let total_value = treasury_balance.checked_add(deployed_capital).ok_or(VaultError::MathOverflow)?;
    let user_value_pool = total_value.checked_sub(config.accumulated_fees).ok_or(VaultError::MathOverflow)?;

    if config.total_shares == 0 {
        return Ok((0, user_value_pool, total_value));
    }

    let user_balance = ((position.user_shares as u128)
        .checked_mul(user_value_pool as u128)
        .ok_or(VaultError::MathOverflow)?
        .checked_div(config.total_shares as u128)
        .ok_or(VaultError::MathOverflow)?) as u64;

    Ok((user_balance, user_value_pool, total_value))
}

#[program]
pub mod vault_with_treasury {
    use super::*;

    /// Initialize the protocol's treasury and configuration
    pub fn initialize_protocol(
        ctx: Context<InitializeProtocol>,
        admin: Pubkey,
        trading_bot: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.protocol_config;
        config.admin = admin;
        config.trading_bot = trading_bot;
        config.treasury = ctx.accounts.treasury_account.key();
        config.total_shares = 0;
        config.total_trading_deployed = 0;
        config.accumulated_fees = 0;
        config.performance_fee_bps = PERFORMANCE_FEE_BPS;
        config.is_paused = false;
        config.bump = ctx.bumps.protocol_config;
        config.last_fee_sweep = 0;

        msg!("Protocol initialized with treasury: {}", config.treasury);
        Ok(())
    }

    /// User creates their position in the vault
    pub fn create_user_position(ctx: Context<CreateUserPosition>) -> Result<()> {
        let position = &mut ctx.accounts.user_position.load_init()?;
        position.owner = ctx.accounts.owner.key();
        position.deposited_amount = 0;
        position.user_shares = 0;
        position.high_water_mark = 0;
        position.last_fee_collection = Clock::get()?.unix_timestamp;
        position.lifetime_fees_paid = 0;
        position.is_active = 1; // true
        position._padding = [0; 7];

        msg!("User position created for: {}", position.owner);
        Ok(())
    }

    /// User deposits funds - goes to TREASURY
    pub fn deposit(ctx: Context<Deposit>, amount: u64, min_shares: u64) -> Result<()> {
        require!(!ctx.accounts.protocol_config.is_paused, VaultError::ProtocolPaused);
        require!(amount > 0, VaultError::InvalidAmount);

        // Transfer from user to TREASURY
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.treasury_account.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Calculate current total value and user value pool
        let config = &mut ctx.accounts.protocol_config;
        let treasury_balance = ctx.accounts.treasury_account.amount;
        let deployed_capital = config.total_trading_deployed;
        let total_value = treasury_balance.checked_add(deployed_capital).ok_or(VaultError::MathOverflow)?;
        let user_value_pool = total_value.checked_sub(config.accumulated_fees).ok_or(VaultError::MathOverflow)?;

        // Calculate shares to mint
        let shares_to_mint: u64 = if config.total_shares == 0 {
            amount
        } else {
            ((amount as u128)
                .checked_mul(config.total_shares as u128)
                .ok_or(VaultError::MathOverflow)?
                .checked_div(user_value_pool as u128)
                .ok_or(VaultError::MathOverflow)?) as u64
        };
        require!(shares_to_mint >= min_shares, VaultError::SlippageExceeded);

        // Update user position
        let mut position = ctx.accounts.user_position.load_mut()?;
        position.deposited_amount = position.deposited_amount
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;
        position.user_shares = position.user_shares
            .checked_add(shares_to_mint)
            .ok_or(VaultError::MathOverflow)?;
        // HWM increase by deposited amount (exact value added)
        position.high_water_mark = position.high_water_mark
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;

        // Update protocol totals
        config.total_shares = config.total_shares
            .checked_add(shares_to_mint)
            .ok_or(VaultError::MathOverflow)?;

        emit!(DepositEvent {
            user: ctx.accounts.owner.key(),
            amount,
            shares_minted: shares_to_mint,
            treasury_balance: ctx.accounts.treasury_account.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Deposited {} to treasury. Minted {} shares. User shares: {}",
            amount, shares_to_mint, position.user_shares);
        Ok(())
    }

    /// User withdraws their share from the treasury
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64, max_shares: u64) -> Result<()> {
        require!(!ctx.accounts.protocol_config.is_paused, VaultError::ProtocolPaused);

        // Calculate current user balance
        let position = ctx.accounts.user_position.load()?;
        let (user_balance, user_value_pool, _total_value) = calculate_user_balance_internal(
            &ctx.accounts.protocol_config,
            &position,
            ctx.accounts.treasury_account.amount,
        )?;
        drop(position); // Release the immutable borrow
        require!(user_balance >= amount, VaultError::InsufficientBalance);

        // Check liquidity
        let treasury_balance = ctx.accounts.treasury_account.amount;
        require!(treasury_balance >= amount, VaultError::InsufficientLiquidity);

        // Calculate shares to burn
        let config = &mut ctx.accounts.protocol_config;
        let shares_to_burn = ((amount as u128)
            .checked_mul(config.total_shares as u128)
            .ok_or(VaultError::MathOverflow)?
            .checked_div(user_value_pool as u128)
            .ok_or(VaultError::MathOverflow)?) as u64;
        require!(shares_to_burn <= max_shares, VaultError::SlippageExceeded);

        // Update user position
        let mut position = ctx.accounts.user_position.load_mut()?;
        let original_shares = position.user_shares;
        position.user_shares = position.user_shares
            .checked_sub(shares_to_burn)
            .ok_or(VaultError::MathOverflow)?;
        // Proportional HWM reduction
        if original_shares > 0 {
            position.high_water_mark = ((position.high_water_mark as u128)
                * (position.user_shares as u128)
                / (original_shares as u128)) as u64;
        } else {
            position.high_water_mark = 0;
        }

        // Update protocol totals
        config.total_shares = config.total_shares
            .checked_sub(shares_to_burn)
            .ok_or(VaultError::MathOverflow)?;

        // Transfer from TREASURY to user
        let config_seeds: &[&[&[u8]]] = &[&[
            b"protocol_config",
            &[config.bump],
        ]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.treasury_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.protocol_config.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, config_seeds);
        token::transfer(cpi_ctx, amount)?;

        emit!(WithdrawEvent {
            user: ctx.accounts.owner.key(),
            amount,
            shares_burned: shares_to_burn,
            remaining_shares: position.user_shares,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Withdrew {} from treasury. Burned {} shares", amount, shares_to_burn);
        Ok(())
    }

    /// Trading bot deploys capital from treasury
    pub fn deploy_capital_for_trading(
        ctx: Context<DeployCapital>,
        amount: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.protocol_config;

        require!(
            ctx.accounts.trading_bot.key() == config.trading_bot,
            VaultError::UnauthorizedTradingBot
        );

        let treasury_balance = ctx.accounts.treasury_account.amount;
        let max_deployable = ((treasury_balance as u128)
            .checked_mul(TRADING_ALLOCATION_BPS as u128)
            .ok_or(VaultError::MathOverflow)?
            .checked_div(10000)
            .ok_or(VaultError::MathOverflow)?) as u64;

        require!(amount <= max_deployable, VaultError::ExceedsMaxDeployment);

        let config_seeds: &[&[&[u8]]] = &[&[
            b"protocol_config",
            &[config.bump],
        ]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.treasury_account.to_account_info(),
            to: ctx.accounts.trading_account.to_account_info(),
            authority: ctx.accounts.protocol_config.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, config_seeds);
        token::transfer(cpi_ctx, amount)?;

        let config = &mut ctx.accounts.protocol_config;
        config.total_trading_deployed = config.total_trading_deployed
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;

        emit!(CapitalDeployedEvent {
            amount,
            total_deployed: config.total_trading_deployed,
            treasury_remaining: treasury_balance - amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Trading bot returns capital with profits/losses (supports partial)
    pub fn return_capital_from_trading(
        ctx: Context<ReturnCapital>,
        returned_amount: u64,
        original_deployed: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.protocol_config;

        require!(
            ctx.accounts.trading_bot.key() == config.trading_bot,
            VaultError::UnauthorizedTradingBot
        );

        require!(original_deployed <= config.total_trading_deployed, VaultError::InvalidAmount);

        let cpi_accounts = Transfer {
            from: ctx.accounts.trading_account.to_account_info(),
            to: ctx.accounts.treasury_account.to_account_info(),
            authority: ctx.accounts.trading_bot.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, returned_amount)?;

        let profit_or_loss = if returned_amount > original_deployed {
            (returned_amount - original_deployed) as i64
        } else {
            -((original_deployed - returned_amount) as i64)
        };

        config.total_trading_deployed = config.total_trading_deployed
            .checked_sub(original_deployed)
            .ok_or(VaultError::MathOverflow)?;

        if profit_or_loss > 0 {
            let profit = profit_or_loss as u64;
            let fee = ((profit as u128)
                .checked_mul(config.performance_fee_bps as u128)
                .ok_or(VaultError::MathOverflow)?
                .checked_div(10000)
                .ok_or(VaultError::MathOverflow)?) as u64;

            config.accumulated_fees = config.accumulated_fees
                .checked_add(fee)
                .ok_or(VaultError::MathOverflow)?;

            msg!("Partial profit: {}, Fee accrued: {}", profit, fee);
        } else if profit_or_loss < 0 {
            let loss = (-profit_or_loss) as u64;
            msg!("Partial loss: {}", loss);
        }

        emit!(CapitalReturnedEvent {
            amount: returned_amount,
            profit_or_loss,
            new_treasury_balance: ctx.accounts.treasury_account.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Collect monthly performance fees for a single user
    pub fn collect_user_fees(ctx: Context<CollectUserFees>) -> Result<()> {
        // Read-only first
        let cfg_ref = &ctx.accounts.protocol_config;
        require!(
            ctx.accounts.caller.key() == cfg_ref.admin || ctx.accounts.caller.key() == cfg_ref.trading_bot,
            VaultError::UnauthorizedCaller
        );

        let now = Clock::get()?.unix_timestamp;
        // First, use immutable borrow for checks and calculations
        let (current_balance, user_value_pool, high_water_mark, perf_bps) = {
            let position = ctx.accounts.user_position.load()?; // immutable borrow
            require!(
                now >= position.last_fee_collection + FEE_COLLECTION_INTERVAL,
                VaultError::FeeCollectionTooSoon
            );
            let (cb, uvp, _) = calculate_user_balance_internal(
                cfg_ref,
                &position,
                ctx.accounts.treasury_account.amount,
            )?;
            (cb, uvp, position.high_water_mark, cfg_ref.performance_fee_bps)
        }; // immutable borrows end here

        // Now use mutable borrow for mutations
        let mut position = ctx.accounts.user_position.load_mut()?;
        let profit = current_balance.saturating_sub(high_water_mark);

        // Compute fee from the copied perf_bps to avoid holding cfg_ref
        let fee = ((profit as u128)
            .checked_mul(perf_bps as u128)
            .ok_or(VaultError::MathOverflow)?
            .checked_div(10000)
            .ok_or(VaultError::MathOverflow)?) as u64;

        if fee > 0 {
            // Borrow ProtocolConfig mutably only when needed for writes and reading total_shares
            let config = &mut ctx.accounts.protocol_config;
            let shares_to_reduce = ((fee as u128)
                .checked_mul(config.total_shares as u128)
                .ok_or(VaultError::MathOverflow)?
                .checked_div(user_value_pool as u128)
                .ok_or(VaultError::MathOverflow)?) as u64;

            position.user_shares = position.user_shares
                .checked_sub(shares_to_reduce)
                .ok_or(VaultError::MathOverflow)?;
            position.high_water_mark = current_balance - fee;
            position.lifetime_fees_paid = position.lifetime_fees_paid
                .checked_add(fee)
                .ok_or(VaultError::MathOverflow)?;

            config.total_shares = config.total_shares
                .checked_sub(shares_to_reduce)
                .ok_or(VaultError::MathOverflow)?;
            config.accumulated_fees = config.accumulated_fees
                .checked_add(fee)
                .ok_or(VaultError::MathOverflow)?;

            emit!(FeeCollectedEvent {
                user: position.owner,
                fee,
                shares_reduced: shares_to_reduce,
                timestamp: now,
            });
        }

        position.last_fee_collection = now;

        msg!("Collected fee {} for user {}", fee, position.owner);
        Ok(())
    }

    /// Batch collect fees for multiple users
    pub fn collect_batch_fees<'info>(ctx: Context<'_, '_, 'info, 'info, CollectBatchFees<'info>>) -> Result<()> {
        // Read-only first to check authorization
        {
            let cfg_ref = &ctx.accounts.protocol_config;
            require!(
                ctx.accounts.caller.key() == cfg_ref.admin || ctx.accounts.caller.key() == cfg_ref.trading_bot,
                VaultError::UnauthorizedCaller
            );
        }

        let now = Clock::get()?.unix_timestamp;
        let treasury_balance = ctx.accounts.treasury_account.amount;
        let mut total_fees = 0u64;
        let mut _total_shares_reduced = 0u64;

        // Process each account separately to avoid lifetime issues
        let num_accounts = ctx.remaining_accounts.len();
        for i in 0..num_accounts {
            let account_info = &ctx.remaining_accounts[i];
            let loader: AccountLoader<UserPosition> = AccountLoader::try_from(account_info)?;
            // First, check eligibility and calculate with immutable borrow
            let (current_balance, user_value_pool, high_water_mark, owner, perf_bps) = {
                let position = loader.load()?; // immutable borrow
                if now < position.last_fee_collection + FEE_COLLECTION_INTERVAL {
                    continue;
                }
                let cfg_ref = &ctx.accounts.protocol_config;
                let (cb, uvp, _) = calculate_user_balance_internal(
                    cfg_ref,
                    &position,
                    treasury_balance,
                )?;
                (cb, uvp, position.high_water_mark, position.owner, cfg_ref.performance_fee_bps)
            }; // immutable borrows end here
            
            // Now use mutable borrow for mutations
            let profit = current_balance.saturating_sub(high_water_mark);
            // Compute fee from the copied perf_bps to avoid holding cfg_ref while mut-borrowing
            let fee = ((profit as u128)
                .checked_mul(perf_bps as u128)
                .ok_or(VaultError::MathOverflow)?
                .checked_div(10000)
                .ok_or(VaultError::MathOverflow)?) as u64;

            let mut position = loader.load_mut()?; // mutable borrow
            
            if fee > 0 {
                // Borrow ProtocolConfig mutably only now (for current total_shares and to write)
                let config = &mut ctx.accounts.protocol_config;
                let shares_to_reduce = ((fee as u128)
                    .checked_mul(config.total_shares as u128)
                    .ok_or(VaultError::MathOverflow)?
                    .checked_div(user_value_pool as u128)
                    .ok_or(VaultError::MathOverflow)?) as u64;

                position.user_shares = position.user_shares.checked_sub(shares_to_reduce).ok_or(VaultError::MathOverflow)?;
                position.high_water_mark = current_balance - fee;
                position.lifetime_fees_paid = position.lifetime_fees_paid.checked_add(fee).ok_or(VaultError::MathOverflow)?;
                position.last_fee_collection = now;

                config.total_shares = config.total_shares.checked_sub(shares_to_reduce).ok_or(VaultError::MathOverflow)?;
                config.accumulated_fees = config.accumulated_fees.checked_add(fee).ok_or(VaultError::MathOverflow)?;

                total_fees += fee;
                _total_shares_reduced += shares_to_reduce;

                emit!(FeeCollectedEvent {
                    user: owner,
                    fee,
                    shares_reduced: shares_to_reduce,
                    timestamp: now,
                });
            } else {
                position.last_fee_collection = now;
            }
            // Loader auto-stores on drop
        }

        msg!("Batch collected total fees: {}", total_fees);
        Ok(())
    }

    /// Admin collects accumulated fees
    pub fn collect_performance_fees(ctx: Context<CollectFees>) -> Result<()> {
        let config = &ctx.accounts.protocol_config;
        require!(
            ctx.accounts.admin.key() == config.admin,
            VaultError::UnauthorizedAdmin
        );

        let fees = config.accumulated_fees;
        require!(fees > 0, VaultError::NoFeesToCollect);

                 let config_seeds: &[&[&[u8]]] = &[&[
                     b"protocol_config",
                   &[config.bump],
                 ]];


        let cpi_accounts = Transfer {
            from: ctx.accounts.treasury_account.to_account_info(),
            to: ctx.accounts.admin_token_account.to_account_info(),
            authority: ctx.accounts.protocol_config.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, config_seeds);
        token::transfer(cpi_ctx, fees)?;

        let config = &mut ctx.accounts.protocol_config;
        config.accumulated_fees = 0;
        config.last_fee_sweep = Clock::get()?.unix_timestamp;

        emit!(FeesWithdrawnEvent {
            amount: fees,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Admin collected {} in fees", fees);
        Ok(())
    }

    /// View function for user balance
    pub fn calculate_user_balance(ctx: Context<CalculateBalance>) -> Result<u64> {
        let position = ctx.accounts.user_position.load()?;
        let (user_balance, _, _) = calculate_user_balance_internal(
            &ctx.accounts.protocol_config,
            &position,
            ctx.accounts.treasury_account.amount,
        )?;
        Ok(user_balance)
    }

    /// View: Protocol stats
    pub fn get_protocol_stats(ctx: Context<GetProtocolStats>) -> Result<(u64, u64, u64)> {
        let config = &ctx.accounts.protocol_config;
        let treasury_balance = ctx.accounts.treasury_account.amount;
        let tvl = treasury_balance + config.total_trading_deployed - config.accumulated_fees;
        Ok((tvl, config.accumulated_fees, config.total_shares))
    }

    /// View: User stats
    pub fn get_user_stats(ctx: Context<GetUserStats>) -> Result<(u64, u64, i64)> {
        let position = ctx.accounts.user_position.load()?;
        let balance = calculate_user_balance_internal(
            &ctx.accounts.protocol_config,
            &position,
            ctx.accounts.treasury_account.amount,
        )?.0;
        Ok((balance, position.lifetime_fees_paid, position.last_fee_collection))
    }

    /// View: Check fee eligibility
    pub fn check_fee_eligibility(ctx: Context<CheckFeeEligibility>) -> Result<bool> {
        let now = Clock::get()?.unix_timestamp;
        let position = ctx.accounts.user_position.load()?;
        Ok(now >= position.last_fee_collection + FEE_COLLECTION_INTERVAL)
    }


    // Emergency functions
    pub fn pause_protocol(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.protocol_config;
        require!(ctx.accounts.admin.key() == config.admin, VaultError::UnauthorizedAdmin);
        config.is_paused = true;
        Ok(())
    }

    pub fn unpause_protocol(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.protocol_config;
        require!(ctx.accounts.admin.key() == config.admin, VaultError::UnauthorizedAdmin);
        config.is_paused = false;
        Ok(())
    }
}

// ============ CONTEXTS ============

#[derive(Accounts)]
pub struct InitializeProtocol<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = ProtocolConfig::LEN,
        seeds = [b"protocol_config"],
        bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        init,
        payer = authority,
        token::mint = token_mint,
        token::authority = protocol_config,
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    pub token_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct CreateUserPosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init,
        payer = owner,
        space = UserPosition::LEN,
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    #[account(
        mut,
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        mut,
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    #[account(mut, token::authority = owner)]
    pub user_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    #[account(
        mut,
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        mut,
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    #[account(mut, token::authority = owner)]
    pub user_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DeployCapital<'info> {
    pub trading_bot: Signer<'info>,
    #[account(
        mut,
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        mut,
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    #[account(mut, token::authority = trading_bot)]
    pub trading_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ReturnCapital<'info> {
    pub trading_bot: Signer<'info>,
    #[account(
        mut,
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        mut,
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    #[account(mut, token::authority = trading_bot)]
    pub trading_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CalculateBalance<'info> {
    #[account(
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    /// CHECK: Owner pubkey for deriving PDA
    pub owner: UncheckedAccount<'info>,
    #[account(
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct CollectUserFees<'info> {
    pub caller: Signer<'info>,
    #[account(mut)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub user_position: AccountLoader<'info, UserPosition>,
    #[account(mut)]
    pub treasury_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct CollectBatchFees<'info> {
    pub caller: Signer<'info>,
    #[account(mut)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub treasury_account: Account<'info, TokenAccount>,
    // remaining_accounts: UserPosition PDAs
}

#[derive(Accounts)]
pub struct CollectFees<'info> {
    pub admin: Signer<'info>,
    #[account(mut)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub treasury_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub admin_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct GetProtocolStats<'info> {
    #[account(
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct GetUserStats<'info> {
    #[account(
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    /// CHECK: Owner pubkey
    pub owner: UncheckedAccount<'info>,
    #[account(
        seeds = [b"protocol_config"],
        bump = protocol_config.bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [b"treasury"],
        bump
    )]
    pub treasury_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct CheckFeeEligibility<'info> {
    #[account(
        seeds = [b"user_position", owner.key().as_ref()],
        bump
    )]
    pub user_position: AccountLoader<'info, UserPosition>,
    /// CHECK: Owner pubkey
    pub owner: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    pub admin: Signer<'info>,
    #[account(mut)]
    pub protocol_config: Account<'info, ProtocolConfig>,
}

// ============ STATE STRUCTS ============

#[account]
pub struct ProtocolConfig {
    pub admin: Pubkey,
    pub trading_bot: Pubkey,
    pub treasury: Pubkey,
    pub total_shares: u64,
    pub total_trading_deployed: u64,
    pub accumulated_fees: u64,
    pub performance_fee_bps: u16,
    pub is_paused: bool,
    pub bump: u8,
    pub last_fee_sweep: i64,
}

impl ProtocolConfig {
    pub const LEN: usize = 8 + 32 + 32 + 32 + 8 + 8 + 8 + 2 + 1 + 1 + 8 + 32; // Disc + fields + padding
}

#[account(zero_copy)]
#[repr(C)]
#[derive(Debug)]
pub struct UserPosition {
    pub owner: Pubkey,
    pub deposited_amount: u64,
    pub user_shares: u64,
    pub high_water_mark: u64,
    pub last_fee_collection: i64,
    pub lifetime_fees_paid: u64,
    pub is_active: u8, // 0 = false, 1 = true for zero-copy compatibility
    pub _padding: [u8; 7], // Padding for alignment
}

impl UserPosition {
    pub const LEN: usize = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 7; // Disc + fields + 7 bytes padding for alignment
}

// ============ EVENTS ============

#[event]
pub struct DepositEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub shares_minted: u64,
    pub treasury_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct WithdrawEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub shares_burned: u64,
    pub remaining_shares: u64,
    pub timestamp: i64,
}

#[event]
pub struct CapitalDeployedEvent {
    pub amount: u64,
    pub total_deployed: u64,
    pub treasury_remaining: u64,
    pub timestamp: i64,
}

#[event]
pub struct CapitalReturnedEvent {
    pub amount: u64,
    pub profit_or_loss: i64,
    pub new_treasury_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct FeeCollectedEvent {
    pub user: Pubkey,
    pub fee: u64,
    pub shares_reduced: u64,
    pub timestamp: i64,
}

#[event]
pub struct FeesWithdrawnEvent {
    pub amount: u64,
    pub timestamp: i64,
}

// ============ ERRORS ============

#[error_code]
pub enum VaultError {
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Insufficient balance")]
    InsufficientBalance,
    #[msg("Insufficient liquidity in treasury")]
    InsufficientLiquidity,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Protocol is paused")]
    ProtocolPaused,
    #[msg("Unauthorized trading bot")]
    UnauthorizedTradingBot,
    #[msg("Unauthorized admin")]
    UnauthorizedAdmin,
    #[msg("Exceeds maximum deployment allocation")]
    ExceedsMaxDeployment,
    #[msg("Fee collection too soon")]
    FeeCollectionTooSoon,
    #[msg("No fees to collect")]
    NoFeesToCollect,
    #[msg("Unauthorized caller")]
    UnauthorizedCaller,
    #[msg("Too many users in batch")]
    TooManyUsers,
    #[msg("Invalid accounts")]
    InvalidAccounts,
    #[msg("Slippage exceeded")]
    SlippageExceeded,
}