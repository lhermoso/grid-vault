# Grid Vault - Performance Tracker Integration Guide

## Overview

This guide provides detailed instructions for integrating your Grid Liquidity Bot's performance tracker with the Grid Vault's new valuation reporting system. The integration enables real-time NAV (Net Asset Value) calculations by reporting deployed capital valuations to the on-chain vault program.

## Architecture Overview

```
┌─────────────────────────┐       ┌──────────────────────┐
│   Performance Tracker   │       │    Grid Vault        │
│  (TypeScript/Off-chain) │       │  (Solana Program)    │
├─────────────────────────┤       ├──────────────────────┤
│ • Orca LP Positions     │──────►│ • update_deployment_ │
│ • Drift Hedge Positions │       │   valuation()        │
│ • Price Feeds (Pyth)    │       │ • Real-time NAV      │
│ • Database Storage      │       │ • User Balances      │
└─────────────────────────┘       └──────────────────────┘
```

## Prerequisites

1. **Trading Bot Keypair**: Your bot must be authorized as the `trading_bot` in the vault's ProtocolConfig
2. **Anchor Client**: Install `@coral-xyz/anchor` for interacting with the vault program
3. **Performance Tracker Running**: Your existing tracker collecting Orca/Drift positions

## Installation

### 1. Add Vault Program IDL

First, build the vault program and export the IDL:

```bash
# In grid-vault directory
anchor build
anchor idl init -f target/idl/vault_with_treasury.json 521NYDkSEV1htFy6iAkwCfkZrAvaaw7YYDd4dhtfnXQ7
```

### 2. Install Dependencies

In your performance-tracker-ts directory:

```bash
npm install @coral-xyz/anchor @solana/web3.js
```

### 3. Add Vault Integration Module

Create a new file `src/services/vaultIntegration.ts`:

```typescript
import { AnchorProvider, Program, Wallet } from '@coral-xyz/anchor';
import { Connection, Keypair, PublicKey } from '@solana/web3.js';
import { VaultWithTreasury } from '../idl/vault_with_treasury';
import IDL from '../idl/vault_with_treasury.json';
import Decimal from 'decimal.js';

export interface DeploymentValuation {
  deploymentId: bigint;
  orcaPositionsValue: bigint;
  driftEquityValue: bigint;
  uncollectedFees: bigint;
  unrealizedPnl: bigint;
  timestamp: bigint;
}

export class VaultIntegrationService {
  private program: Program<VaultWithTreasury>;
  private protocolConfigPDA: PublicKey;
  private provider: AnchorProvider;

  constructor(
    connection: Connection,
    tradingBotKeypair: Keypair,
    programId: string = '521NYDkSEV1htFy6iAkwCfkZrAvaaw7YYDd4dhtfnXQ7'
  ) {
    // Create provider
    const wallet = new Wallet(tradingBotKeypair);
    this.provider = new AnchorProvider(connection, wallet, {
      commitment: 'confirmed',
    });

    // Initialize program
    this.program = new Program(
      IDL as VaultWithTreasury,
      new PublicKey(programId),
      this.provider
    );

    // Derive protocol config PDA
    const [configPDA] = PublicKey.findProgramAddressSync(
      [Buffer.from('protocol_config')],
      this.program.programId
    );
    this.protocolConfigPDA = configPDA;
  }

  /**
   * Report current deployed capital valuation to the vault
   */
  async reportValuation(
    orcaPositionsValue: Decimal,
    driftEquityValue: Decimal,
    uncollectedFees: Decimal,
    unrealizedPnl: Decimal,
    deploymentId: number = 1
  ): Promise<string> {
    try {
      // Convert decimal values to USDC precision (1e6)
      const USDC_PRECISION = new Decimal(1e6);
      
      const valuation: DeploymentValuation = {
        deploymentId: BigInt(deploymentId),
        orcaPositionsValue: BigInt(orcaPositionsValue.mul(USDC_PRECISION).toFixed(0)),
        driftEquityValue: BigInt(driftEquityValue.mul(USDC_PRECISION).toFixed(0)),
        uncollectedFees: BigInt(uncollectedFees.mul(USDC_PRECISION).toFixed(0)),
        unrealizedPnl: BigInt(unrealizedPnl.mul(USDC_PRECISION).toFixed(0)),
        timestamp: BigInt(Math.floor(Date.now() / 1000)),
      };

      console.log('Reporting valuation to vault:', {
        orcaValue: orcaPositionsValue.toFixed(2),
        driftValue: driftEquityValue.toFixed(2),
        fees: uncollectedFees.toFixed(2),
        pnl: unrealizedPnl.toFixed(2),
      });

      // Call the update_deployment_valuation instruction
      const tx = await this.program.methods
        .updateDeploymentValuation(valuation)
        .accounts({
          tradingBot: this.provider.wallet.publicKey,
          protocolConfig: this.protocolConfigPDA,
        })
        .rpc();

      console.log('Valuation reported successfully. Transaction:', tx);
      return tx;
    } catch (error) {
      console.error('Failed to report valuation:', error);
      throw error;
    }
  }

  /**
   * Get current protocol configuration including last valuation
   */
  async getProtocolConfig() {
    const config = await this.program.account.protocolConfig.fetch(
      this.protocolConfigPDA
    );
    
    return {
      totalDeployed: new Decimal(config.totalTradingDeployed.toString()).div(1e6),
      currentValue: new Decimal(config.deployedCurrentValue.toString()).div(1e6),
      lastValuationTime: new Date(config.lastValuationTimestamp.toNumber() * 1000),
      pendingFees: new Decimal(config.pendingUnrealizedFees.toString()).div(1e6),
    };
  }

  /**
   * Check if valuation needs update (older than specified hours)
   */
  async needsValuationUpdate(hoursThreshold: number = 6): Promise<boolean> {
    const config = await this.getProtocolConfig();
    const hoursSinceUpdate = 
      (Date.now() - config.lastValuationTime.getTime()) / (1000 * 60 * 60);
    
    return hoursSinceUpdate >= hoursThreshold;
  }
}
```

## Integration with Collector

### 1. Update Collector Configuration

Modify `src/collector.ts` to include vault integration:

```typescript
import { VaultIntegrationService } from './services/vaultIntegration';
import { WalletLoader } from './services/walletLoader';

class PerformanceCollector {
  private vaultIntegration?: VaultIntegrationService;
  private tradingBotWallet?: WalletInfo;
  
  constructor() {
    // ... existing code ...
    
    // Load trading bot wallet for vault integration
    this.initializeVaultIntegration();
  }

  private async initializeVaultIntegration() {
    try {
      // Load the trading bot wallet (adjust path as needed)
      const wallets = await this.walletLoader.loadWallets();
      this.tradingBotWallet = wallets.find(w => w.name === 'trading-bot');
      
      if (this.tradingBotWallet) {
        this.vaultIntegration = new VaultIntegrationService(
          this.connection,
          this.tradingBotWallet.keypair,
          process.env.VAULT_PROGRAM_ID
        );
        console.log('Vault integration initialized');
      } else {
        console.warn('Trading bot wallet not found - vault integration disabled');
      }
    } catch (error) {
      console.error('Failed to initialize vault integration:', error);
    }
  }
```

### 2. Add Valuation Reporting

Update the collection method to report valuations:

```typescript
  /**
   * Collect and report aggregate valuation to vault
   */
  async collectAndReportValuation(): Promise<void> {
    if (!this.vaultIntegration || !this.tradingBotWallet) {
      console.log('Vault integration not available');
      return;
    }

    try {
      // Check if update is needed
      const needsUpdate = await this.vaultIntegration.needsValuationUpdate(6);
      if (!needsUpdate) {
        console.log('Valuation is still fresh, skipping update');
        return;
      }

      console.log('Collecting aggregate positions for valuation...');
      
      // Aggregate all trading wallets (excluding treasury/cold wallets)
      const tradingWallets = this.wallets.filter(w => 
        w.name.includes('trading') || w.name.includes('bot')
      );
      
      let totalOrcaValue = new Decimal(0);
      let totalOrcaFees = new Decimal(0);
      let totalDriftEquity = new Decimal(0);
      let totalDriftPnl = new Decimal(0);
      
      for (const wallet of tradingWallets) {
        // Get Orca positions
        const orcaPositions = await this.orcaService.getPositions(wallet);
        const orcaValue = this.orcaService.calculateTotalValue(orcaPositions);
        totalOrcaValue = totalOrcaValue.add(orcaValue.positionsValue);
        totalOrcaFees = totalOrcaFees.add(orcaValue.uncollectedFees);
        
        // Get Drift account
        const driftData = await this.driftService.getAccountData(wallet);
        if (driftData) {
          totalDriftEquity = totalDriftEquity.add(driftData.totalEquityUsd);
          totalDriftPnl = totalDriftPnl.add(driftData.unrealizedPnlUsd);
        }
      }
      
      // Get original deployed amount from vault
      const config = await this.vaultIntegration.getProtocolConfig();
      const originalDeployed = config.totalDeployed;
      
      // Calculate total current value and PnL
      const totalCurrentValue = totalOrcaValue
        .add(totalOrcaFees)
        .add(totalDriftEquity);
      
      const unrealizedPnl = totalCurrentValue.sub(originalDeployed);
      
      // Report to vault
      await this.vaultIntegration.reportValuation(
        totalOrcaValue,
        totalDriftEquity,
        totalOrcaFees,
        unrealizedPnl
      );
      
      console.log('Valuation reported to vault:');
      console.log(`  Original Deployed: $${originalDeployed.toFixed(2)}`);
      console.log(`  Current Value: $${totalCurrentValue.toFixed(2)}`);
      console.log(`  Unrealized PnL: $${unrealizedPnl.toFixed(2)}`);
      console.log(`  - Orca Positions: $${totalOrcaValue.toFixed(2)}`);
      console.log(`  - Orca Fees: $${totalOrcaFees.toFixed(2)}`);
      console.log(`  - Drift Equity: $${totalDriftEquity.toFixed(2)}`);
      
    } catch (error) {
      console.error('Failed to report valuation to vault:', error);
    }
  }
```

### 3. Schedule Valuation Updates

Add a cron job for regular valuation updates:

```typescript
  /**
   * Start the performance tracking service
   */
  async start(): Promise<void> {
    await this.initialize();
    
    // Initial collection
    await this.collectAllWallets();
    
    // Report initial valuation
    await this.collectAndReportValuation();
    
    // Schedule regular data collection (every hour)
    cron.schedule('0 * * * *', async () => {
      console.log('Starting scheduled data collection...');
      await this.collectAllWallets();
    });
    
    // Schedule valuation reporting (every 6 hours)
    cron.schedule('0 */6 * * *', async () => {
      console.log('Starting scheduled valuation report...');
      await this.collectAndReportValuation();
    });
    
    console.log('Performance tracker started with valuation reporting');
    console.log('Data collection: Every hour');
    console.log('Valuation reports: Every 6 hours');
  }
```

## Environment Configuration

Add these variables to your `.env` file:

```bash
# Existing configuration
DATABASE_URL=postgresql://grid_admin:tracker_password@localhost:5432/grid_performance
RPC_ENDPOINT=https://api.mainnet-beta.solana.com
COLLECTION_INTERVAL=3600
DRIFT_ENV=mainnet-beta

# New vault integration settings
VAULT_PROGRAM_ID=521NYDkSEV1htFy6iAkwCfkZrAvaaw7YYDd4dhtfnXQ7
VALUATION_UPDATE_INTERVAL=21600  # 6 hours in seconds
WALLETS_PATH=/app/wallets

# Trading bot wallet should be in the wallets directory
# Named as 'trading-bot.json' or configured in wallet loader
```

## Testing the Integration

### 1. Manual Test

Create a test script `test-valuation.ts`:

```typescript
import { Connection, Keypair } from '@solana/web3.js';
import { VaultIntegrationService } from './src/services/vaultIntegration';
import Decimal from 'decimal.js';
import * as fs from 'fs';

async function testValuation() {
  // Load trading bot keypair
  const secretKey = JSON.parse(
    fs.readFileSync('/path/to/trading-bot.json', 'utf-8')
  );
  const tradingBot = Keypair.fromSecretKey(new Uint8Array(secretKey));
  
  // Connect to RPC
  const connection = new Connection(
    process.env.RPC_ENDPOINT || 'https://api.devnet.solana.com',
    'confirmed'
  );
  
  // Initialize vault integration
  const vaultService = new VaultIntegrationService(
    connection,
    tradingBot
  );
  
  // Test values (in USD)
  const testValuation = {
    orcaPositions: new Decimal(75000),    // $75k in LP positions
    driftEquity: new Decimal(15000),      // $15k in Drift
    uncollectedFees: new Decimal(500),    // $500 in fees
    unrealizedPnl: new Decimal(500),      // $500 profit
  };
  
  // Report valuation
  const tx = await vaultService.reportValuation(
    testValuation.orcaPositions,
    testValuation.driftEquity,
    testValuation.uncollectedFees,
    testValuation.unrealizedPnl
  );
  
  console.log('Test valuation reported:', tx);
  
  // Check updated config
  const config = await vaultService.getProtocolConfig();
  console.log('Updated vault config:', config);
}

testValuation().catch(console.error);
```

### 2. Run Test

```bash
npx ts-node test-valuation.ts
```

## Monitoring & Alerts

### 1. Add Logging

Create detailed logs for audit trail:

```typescript
class ValuationLogger {
  private logPath = './logs/valuations.jsonl';
  
  async logValuation(data: any) {
    const entry = {
      timestamp: new Date().toISOString(),
      ...data,
    };
    
    fs.appendFileSync(
      this.logPath,
      JSON.stringify(entry) + '\n'
    );
  }
}
```

### 2. Health Checks

Monitor valuation freshness:

```typescript
async function checkValuationHealth() {
  const config = await vaultService.getProtocolConfig();
  const hoursSinceUpdate = 
    (Date.now() - config.lastValuationTime.getTime()) / (1000 * 60 * 60);
  
  if (hoursSinceUpdate > 24) {
    console.error('CRITICAL: Valuation is stale!');
    // Send alert to Discord/Telegram
  }
}
```

## Error Handling

### Common Errors and Solutions

1. **`UnauthorizedTradingBot`**
   - Ensure your bot's public key matches the `trading_bot` in ProtocolConfig
   - Check that you're signing with the correct keypair

2. **`InvalidValuation`**
   - Timestamp must be within 5 minutes of current time
   - Ensure your system clock is synchronized

3. **`StaleValuation`** (in balance calculations)
   - Valuations older than 24 hours are considered stale
   - Increase update frequency if needed

4. **Transaction Failed**
   - Check RPC endpoint is responsive
   - Ensure bot has enough SOL for transaction fees
   - Verify program ID matches deployed program

## Best Practices

1. **Update Frequency**
   - Report valuations every 6-12 hours for normal operations
   - Increase frequency during high volatility
   - Always update before large capital returns

2. **Deployment Tracking**
   - Use unique `deployment_id` for different strategies
   - Track partial returns accurately
   - Clear valuations when fully returning capital

3. **Price Accuracy**
   - Use multiple price sources (Pyth + Jupiter)
   - Handle price feed failures gracefully
   - Log price discrepancies for review

4. **Security**
   - Keep trading bot keypair secure
   - Use environment variables for sensitive data
   - Implement rate limiting on updates

## Troubleshooting

### Debug Mode

Enable detailed logging:

```typescript
// Set in environment
process.env.DEBUG = 'vault:*';

// Or in code
import Debug from 'debug';
const debug = Debug('vault:integration');

debug('Reporting valuation:', valuation);
```

### Common Issues

1. **Positions not updating**: Check Orca/Drift service connections
2. **Wrong values**: Verify decimal precision conversions
3. **Missing fees**: Ensure uncollected fees are included
4. **PnL mismatch**: Compare original deployed vs current total

## Support

For issues or questions:
1. Check program logs: `solana logs 521NYDkSEV1htFy6iAkwCfkZrAvaaw7YYDd4dhtfnXQ7`
2. Review vault events for ValuationUpdateEvent
3. Verify tracker database for historical data

## Appendix: TypeScript Types

```typescript
// Complete type definitions for vault program
export interface ProtocolConfig {
  admin: PublicKey;
  tradingBot: PublicKey;
  treasury: PublicKey;
  totalShares: bigint;
  totalTradingDeployed: bigint;
  accumulatedFees: bigint;
  performanceFeeBps: number;
  isPaused: boolean;
  bump: number;
  lastFeeSweep: bigint;
  deployedCurrentValue: bigint;
  lastValuationTimestamp: bigint;
  pendingUnrealizedFees: bigint;
}

export interface ValuationUpdateEvent {
  totalDeployedOriginal: bigint;
  totalDeployedCurrent: bigint;
  orcaValue: bigint;
  driftValue: bigint;
  uncollectedFees: bigint;
  unrealizedPnl: bigint;
  pendingFees: bigint;
  timestamp: bigint;
}
```

---

*Last Updated: 2025-01-09*
*Version: 1.0.0*