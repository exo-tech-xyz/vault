# Vault Standard Suite

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Anchor](https://img.shields.io/badge/Built%20with-Anchor-blue)](https://www.anchor-lang.com/)
[![Solana](https://img.shields.io/badge/Solana-Localnet-green)](https://solana.com)

> :warning: Reference implementation only - not audited for mainnet use. Review and audit before deploying to production.

A standard factory program for tokenized vaults on Solana, inspired by [ERC-7540](https://eips.ethereum.org/EIPS/eip-7540). It standardizes the subscription (deposit) and redemption (withdrawal) flow so teams can build on a shared, audited primitive instead of deploying bespoke vault programs.

## Design Decisions

See [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) for the catalog of features requested by ecosystem teams and how each is handled — implemented in the template, spec'd as an extension, integrator-side, or intentionally excluded.

## Deployments

For demo purposes the program is deployed on devnet.

| Network | Program ID                                    |
| ------- | --------------------------------------------- |
| Devnet  | `vaLtx8Su1t5P1CZG5GFEMc94sN4K7A4AUUiciadtvUi` |

> Not deployed to mainnet-beta.

## Overview

Real World Asset (RWA) issuers and other institutions repeatedly build the same vault primitives — deposits and redemptions into a managed strategy, with role-based access control and KYC. Today every team ships its own implementation, increasing integration work and the surface area for vulnerabilities. The Vault Standard Suite provides a shared, customizable vault program so that critical Solana infrastructure can be reused safely while still allowing innovation on top.

The design follows ERC-7540 and the lessons of Token-2022, keeping the core minimal and pushing optional behavior into extensions.

## Key Features

- **Async deposit/redemption** — requests are queued and settled by a vault authority once NAV is updated; shares and assets are not distributed atomically.
- **Bring-your-own share mint** — a vault accepts a pre-configured mint as its share token rather than creating one, decoupling the program from future mint/extension combinations and reducing the need for upgrades.
- **No forced ATAs** — the program does not initialize token accounts or enforce ATAs; callers initialize accounts beforehand, preserving flexibility for non-ATA usage.
- **Extensions** — opt-in modules (fees, min subscription/redemption, pausable flows, subscription/redemption queues) that add conditional logic to core instructions.
- **Composable with [sRFC-37](https://forum.solana.com/t/srfc-37-efficient-block-allow-list-token-standard/4036)** — designed to pair with the Token Access Control List (ACL) standard, an improvement to Transfer Hooks, for KYC'd tokens without compromising composability.

## Programs

### Async Vault

The primary implementation, supporting asynchronous deposit and redemption flows where requests are queued and settled by the vault authority. Targeted at RWA issuers, teams running off-chain strategies, and any context requiring regulatory compliance.

### Closing an async vault

Closing is terminal and available to every vault. The authority can call `close_vault` once no
user is waiting to claim shares or assets, share supply is zero, and enabled queues are drained.
The instruction returns share-mint authority and account rent to the vault authority, sweeps any
unaccounted asset-token residue from the reserve and pending-vault to the authority's asset
account, then closes the reserve, pending-vault, and vault-config accounts. Installing and
pausing the optional `PausableSubscriptions` extension before settlement is recommended to stop
new deposit requests while the vault is being wound down.

Token-2022 share mints with `MintCloseAuthority` are not supported because a close authority
could remove the zero-supply share mint before `close_vault` finishes.

## Documentation

- [Design Decisions](DESIGN_DECISIONS.md) — requested features and how each is approached
- [Glossary](GLOSSARY.md) — vault terminology
- [Sequence Diagrams](programs/async_vault/docs/SEQUENCES.md) — deposit, redeem, and authority withdraw flows
- [Subscription Queue](programs/async_vault/docs/extensions/SubscriptionQueue.md) — FIFO queue extension mechanics

## Local Development

### Prerequisites

- Rust (see `rust-toolchain.toml`)
- Node.js (see `.nvmrc`)
- pnpm (see `package.json` `packageManager`)
- Solana CLI
- Anchor CLI (see `Anchor.toml`)

### Build & Test

```bash
# Install dependencies
just install

# Build IDL + clients
just build

# Run unit + integration tests
just test

# Format and lint
just fmt
```

## Tech Stack

- **[Anchor](https://www.anchor-lang.com/)** — Solana program framework
- **[Codama](https://github.com/codama-idl)** — IDL-driven Rust + TypeScript client generation
- **[LiteSVM](https://github.com/LiteSVM/litesvm)** — fast in-process testing

## Security

This program has been reviewed by [Cantina](https://cantina.xyz) (APEX). See the [audit report](audits/apex-scan-june-22-2026.pdf) and [audit status](audits/AUDIT_STATUS.md) for scope and the audited baseline commit.

This remains a **reference implementation and is not audited for mainnet use** — review and audit before deploying to production.

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## Token-2022 Considerations

The vault assumes exclusive custody of the tokens it holds. Some Token-2022 extensions break that assumption and are **not** rejected by the program — vet both the asset mint and the share mint before use.

Enforced: nonzero `TransferFeeConfig` asset mints are rejected; the share mint's mint authority moves to the vault PDA.

Vet yourself:

- **Asset `PermanentDelegate`** — can drain `reserve`/`pending_vault` directly, leaving `total_asset_balance` stale.
- **Share `MintCloseAuthority`** — closing the mint at zero supply bricks refund/claim paths.
- **Freeze authority / default-frozen** — can freeze `reserve`/`pending_vault` and block transfers.

## Notes

These programs are unoptimized and written in Anchor simply for the speed of development. Feedback is welcome and optimizations will be implemented once there is consensus that the structure of the program in question is relatively stable.

## Contributing

To suggest a feature or change, open an issue with a detailed explanation of the request and the reasoning behind it.

## Acknowledgments

Thanks to [Exo Technologies](https://www.exotechnologies.xyz/) for the initial design and implementation of this program.

---

Built and maintained by the [Solana Foundation](https://solana.org/).

Licensed under MIT. See [LICENSE](LICENSE) for details.

## Support

- [**Solana StackExchange**](https://solana.stackexchange.com/) — tag `anchor`
- [**Open an Issue**](https://github.com/solana-foundation/vault/issues/new)
