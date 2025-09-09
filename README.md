# 🔷 Grid Vault

> **Decentralized Treasury Management Protocol on Solana**  
> *Where capital meets algorithmic precision*

[![Solana](https://img.shields.io/badge/Solana-9945FF?style=for-the-badge&logo=solana&logoColor=white)](https://solana.com)
[![Anchor](https://img.shields.io/badge/Anchor-0.31.1-000000?style=for-the-badge)](https://www.anchor-lang.com/)
[![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)

## ⚡ Overview

Grid Vault is a sophisticated on-chain treasury management system built on Solana, designed to democratize algorithmic trading strategies. By pooling capital into a shared treasury, the protocol enables professional-grade grid trading while maintaining transparent profit distribution among participants.

### 🎯 Core Features

- **Unified Treasury Architecture** - Single pool design for maximum capital efficiency
- **Algorithmic Capital Deployment** - Automated grid trading with up to 90% treasury utilization
- **Proportional Profit Sharing** - Fair distribution based on contribution ratios
- **Performance Fee Structure** - 25% success fee on generated profits
- **Real-time Valuation Tracking** - On-chain NAV calculations for full transparency

## 🏗️ Architecture

```
┌─────────────────────────────────────────┐
│            Protocol Admin               │
└────────────────┬────────────────────────┘
                 │
     ┌───────────▼──────────┐
     │   ProtocolConfig     │
     │  • Admin Authority   │
     │  • Trading Bot       │
     │  • Performance Fees  │
     └───────────┬──────────┘
                 │
    ┌────────────▼────────────┐
    │      Treasury PDA       │
    │   • Pooled Capital      │
    │   • 90% Deployable      │
    │   • 10% Liquidity Buffer│
    └─────────┬───────────────┘
              │
     ┌────────▼────────┐
     │  User Positions │
     │  • Virtual Shares│
     │  • P&L Tracking  │
     └─────────────────┘
```

## 🚀 Quick Start

### Prerequisites

- Rust 1.75+
- Solana CLI 1.18+
- Anchor 0.31.1
- Node.js 18+

### Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/grid-vault.git
cd grid-vault

# Install dependencies
yarn install

# Build the program
anchor build

# Run tests
anchor test
```

### Deployment

```bash
# Deploy to devnet
anchor deploy --provider.cluster devnet

# Update program ID in lib.rs after deployment
vim programs/grid-vault/src/lib.rs
# Replace YourProgramIDHere with actual program ID
```

## 💻 Program Interface

### Core Instructions

#### `initialize_protocol`
Initializes the protocol with admin and trading bot authorities.

```rust
pub fn initialize_protocol(
    ctx: Context<InitializeProtocol>,
    admin: Pubkey,
    trading_bot: Pubkey,
    performance_fee_bps: u16
) -> Result<()>
```

#### `deposit`
Allows users to deposit SOL into the shared treasury.

```rust
pub fn deposit(
    ctx: Context<Deposit>,
    amount: u64
) -> Result<()>
```

#### `withdraw`
Enables proportional withdrawal based on current valuation.

```rust
pub fn withdraw(
    ctx: Context<Withdraw>,
    amount: u64
) -> Result<()>
```

#### `deploy_capital_for_trading`
Trading bot deploys capital for grid trading operations.

```rust
pub fn deploy_capital_for_trading(
    ctx: Context<DeployCapital>,
    amount: u64
) -> Result<()>
```

## 📊 Performance Metrics

| Metric | Value |
|--------|-------|
| Max Capital Deployment | 90% |
| Liquidity Buffer | 10% |
| Performance Fee | 25% |
| Transaction Speed | ~400ms |
| Program Size | ~150KB |

## 🔐 Security

- **Anchor Framework** - Built-in security checks and account validation
- **PDA Architecture** - Program-derived addresses for secure treasury management
- **Authority Controls** - Multi-signature requirements for critical operations
- **Overflow Protection** - Safe math operations throughout

## 🧪 Testing

```bash
# Run all tests
yarn test

# Run specific test suite
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/grid-vault.ts

# Run with coverage
yarn test:coverage
```

## 🛠️ Development

### Code Quality

```bash
# Lint code
yarn lint

# Auto-fix issues
yarn lint:fix

# Type checking
cargo check
```

### Project Structure

```
grid-vault/
├── programs/
│   └── grid-vault/
│       └── src/
│           └── lib.rs          # Main program logic
├── tests/
│   └── grid-vault.ts           # Integration tests
├── migrations/
│   └── deploy.ts               # Deployment scripts
└── Anchor.toml                 # Anchor configuration
```

## 📈 Roadmap

- [ ] Multi-asset support (USDC, USDT)
- [ ] Advanced grid strategies
- [ ] Governance token integration
- [ ] Cross-chain bridging
- [ ] Mobile SDK
- [ ] Audit completion

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit changes (`git commit -m 'Add AmazingFeature'`)
4. Push to branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## 📜 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🌟 Acknowledgments

- Solana Foundation for the incredible blockchain infrastructure
- Anchor Protocol for the robust framework
- Our community of traders and developers

---

<div align="center">

**Built with ⚡ on Solana**

[Website](https://gridvault.io) • [Documentation](https://docs.gridvault.io) • [Twitter](https://twitter.com/gridvault) • [Discord](https://discord.gg/gridvault)

</div>