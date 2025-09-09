# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Solana program (smart contract) built with Anchor framework that implements a vault with treasury management system. The program allows users to deposit funds into a shared treasury, enables a trading bot to deploy capital for trading, and distributes profits/losses proportionally among users.

## Key Commands

### Build and Development
```bash
# Build the Solana program
anchor build

# Run tests
anchor test

# Deploy to localnet
anchor deploy

# Run TypeScript tests
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts
```

### Code Quality
```bash
# Run linter
yarn lint

# Fix linting issues
yarn lint:fix
```

## Architecture

### Core Program Structure
- **Main Program**: `programs/grid-vault/src/lib.rs` - Contains all smart contract logic
- **Program ID**: Currently placeholder `YourProgramIDHere` at line 4 - needs to be updated with actual program ID after deployment
- **Module Name**: `vault_with_treasury`

### Key Components

1. **Protocol Configuration (`ProtocolConfig`)**
   - Manages global protocol state including admin, trading bot, treasury addresses
   - Tracks total deposits and deployed capital
   - Controls performance fees (25% default)

2. **User Positions (`UserPosition`)**
   - Tracks individual user deposits and virtual balances
   - Maintains user's proportional share of the vault

3. **Treasury System**
   - Central treasury holds all user deposits (PDA: `[b"treasury"]`)
   - 90% of treasury can be deployed for trading
   - 10% maintained as liquidity buffer

### Main Functions

- `initialize_protocol`: Sets up the protocol with admin and trading bot authorities
- `create_user_position`: Creates a user's account in the vault
- `deposit`: Users deposit funds into the shared treasury
- `withdraw`: Users withdraw their proportional share
- `deploy_capital_for_trading`: Trading bot deploys capital from treasury (max 90%)
- `return_capital_from_trading`: Trading bot returns capital with profits/losses
- `calculate_user_balance`: Calculates user's current balance including profits/losses

### Testing
- Test files located in `tests/` directory
- Currently has basic initialization test that needs expansion
- Tests use TypeScript with Anchor's testing framework

## Development Notes

- The program uses Anchor 0.31.1
- All user funds are pooled in a single treasury account, not individual vaults
- Profit distribution is handled proportionally based on user's virtual balance relative to total deposits
- Performance fee of 25% is taken from profits before distribution
- The program includes comprehensive error handling and event emission for tracking