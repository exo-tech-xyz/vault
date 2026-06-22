# Design Decisions

This catalogs the feature requests and requirements gathered from ecosystem teams, and for each, how the template handles it — either already implemented in the repo, or the intended approach a team would follow to build it.

The vault is an open-source **base async-vault template**: teams fork it and add the functionality their own use case needs. It's shipped as a template rather than deployed because every team's requirements differ. The base covers the common ground, with optional extensions for frequently shared needs.

_Both atomic and async vault models were explored. Async was the model teams actually needed, so the atomic track was dropped — this template and catalog cover the async vault only._

**Each item is marked:**

- 🟢 **Implemented in template**
- 🟡 **How it would be built** — a spec'd-but-not-yet-implemented extension, an integrator-side pattern, or an intentional exclusion with the reasoning.

---

## Design baseline (how the template is built)

How the core is approached, before any requested features:

- 🟢 **Shares minted/burned, never transferred** — at each step shares move via mint/burn on the user's account, so transfer-fee-style extensions don't corrupt accounting.
- 🟢 **Single authority** — one authority signs permissioned actions; transferred in two steps (`InviteNewAuthority` → `AcceptAuthorityInvitation`). Teams needing richer roles point the authority at a PDA from their own RBAC program.
- 🟢 **Async request lifecycle** — `RequestDeposit`/`RequestRedeem` move funds to a shared `pending_vault` → authority `ApproveRequest` (snapshots NAV into the request) / `RejectRequest` → user/operator `Claim`; `CancelRequest` while pending. Each request is a unique keypair.

## Deposit / redemption mechanics

- 🟢 **Let someone act on a user's behalf** — `SetOperator`; the operator can claim/cancel for the user.
- 🟡 **Auto-pairing of async deposits and redemptions** — an authority instruction that matches an open deposit against an open redemption so they settle against each other at NAV, instead of routing assets through an external strategy. Ecosystem teams flagged that 1:1 netting may have regulatory implications in some jurisdictions; those teams suggested netting against aggregated groups of requests as an alternative.
- 🟡 **Slippage protection on requests** — intentionally **not** supported: it doesn't fit the async + NAV model, and subscribers are expected to understand the fund. (No integrator workaround intended.)
- 🟡 **Multi-asset deposits** — let a vault accept more than one deposit asset; possible future extension (current template is single-asset).
- 🟡 **Multi-asset holdings** — unlikely to be needed: in the async model the vault doesn't hold deployed assets — the authority withdraws and allocates them, so holdings live outside the vault.

## Configurable extensions (opt-in TLV modules)

The most common shared needs, built as toggleable extensions.

| Requested feature                    | Use case                                                       | Approach                                                                   |
| ------------------------------------ | -------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Deposit / Withdraw fees              | Charge fixed or % (bps) fees                                   | 🟢 Fee extension                                                           |
| Performance / management fees        | Fee on gains, or accruing over time                            | 🟡 A fee extension beyond the flat deposit/withdraw fees                   |
| Pausable subscriptions / redemptions | Manual subscription/redemption windows (RWA)                   | 🟢 Pausable extensions                                                     |
| FIFO subscription / redemption queue | Fair, ordered processing                                       | 🟢 Queue extensions (see fairness note below)                              |
| Minimum subscription / redemption    | Floor per request                                              | 🟢 Min extensions                                                          |
| Instant share minting                | Small deposits skip approval, mint instantly under a threshold | 🟡 Spec'd extension; not in template                                       |
| Subscription lock-up period          | Cooldown after approval before shares are claimable            | 🟡 Spec'd extension; not in template                                       |
| Partial subscriptions / redemptions  | Authority partially fills, down to a user-set minimum          | 🟡 Spec'd extension (`min_partial_fill` / `partial_fill`); not in template |
| Vault asset cap                      | Hard cap on total assets (`deposit + balance ≤ cap`)           | 🟡 Spec'd extension; not in template                                       |

- **FIFO fairness** — pure FIFO is unfair when limits exist and a whale consumes the limit first. A team needing fairness would generalize the queue (granular control), or combine authority partial fills with a max-redemption + withdraw cooldown.

## Vault lifecycle

- 🟢 **Terminal vault closure** — core `CloseVault` is available to every vault once all requests
  are settled, share supply is zero, and queues are drained. Residual asset tokens are swept to
  the authority before vault accounts are closed.
- 🟢 **Subscription pause** — `PausableSubscriptions` is optional but recommended when winding
  down a vault, since it stops new deposit requests while existing obligations are settled.

## Limits & caps (often regulatory)

- 🟢 **Per-request minimums** — `MinSubscription` / `MinRedemption`.
- 🟡 **Max depositors limit** (regulatory) — an extension tracking a depositor count and rejecting new entrants past the cap.
- 🟡 **Per-investor subscription/redemption caps** — track per-owner totals (likely a per-user account) and enforce a ceiling at request time.
- 🟡 **Withdraw cooldown** — enforce a wait before a user can request redemption; overlaps with the lock-up extension approach.

## Running strategies on vault assets & NAV

- 🟢 **Deploy vault assets into a strategy** (e.g. lend/borrow, an off-chain RWA position) — the common "run custom logic / route assets into a downstream protocol" request. In the async model the authority pulls assets out with `WithdrawAssets`, deploys them wherever the strategy lives, and reflects the result by updating NAV. Strategy execution sits outside the vault by design; a virtual `total_asset_balance` keeps accounting correct while assets are deployed.
- 🟢 **NAV is authority-set** — `UpdateNav` (bumps `nav_version`); `ApproveRequest` snapshots NAV into each request, fixing the conversion rate at approval. Approval requires NAV ≠ 0; the program does not enforce NAV freshness — updating NAV before approving is authority discipline.
- 🟡 **NAV anomaly / update threshold** — a guard against bad NAV updates; would be a configurable extension (the core can't know what an anomaly is), percentage- or fixed-based.
- 🟡 **Lend/borrow looping, oracle verifiability** — integrator-side: built into whatever strategy the authority runs with the withdrawn assets, not in the template.

## Compliance / KYC / transfer control

Mostly integrator-side or composed with other standards.

- 🟡 **KYC'd tokens without bespoke programs** — compose with **[sRFC 37 (Token ACL)](https://forum.solana.com/t/srfc-37-efficient-block-allow-list-token-standard/4036)**, an efficient block/allow-list standard (improvement over transfer hooks).
- 🟡 **Allow/block-list at transfer time** — some teams found transfer-time lists insufficient for their needs and freeze/unfreeze instead; enshrining all verification on-chain doesn't scale (CU limits, off-chain data). An approach raised: a mint-level **"token movements verifiers"** extension — a configurable m-of-n signer list that must sign a transfer for it to be accepted, so verification happens off-chain and only the approval is recorded on-chain.
- 🟡 **Investor tiering** (retail/accredited/entity/individual) — one approach is on-chain user groups with per-group config, but persona-based tiers (e.g. regulator-defined categories) aren't a global standard. A team would either keep tiering off-chain or define generic on-chain user groups the vault reads.
- 🟡 **KYC/KYB** — off-chain today, keyed to on-chain groups. On-chain credentials (identity NFTs, ZK proofs) explored but not adopted — teams reported these did not meet their regulatory requirements. Long-term aim: a shared on-chain policy system so users aren't onboarded twice.
- 🟡 **Engineering direction for transfer/KYC/KYB tooling** — discussed with teams: modularize into low-granularity instructions; ship basic primitives, keep the rest off-chain.
- 🟡 **Transfer allowlist for assets leaving the vault** — an extension restricting which destinations the authority may withdraw assets to.
- 🟡 **On-chain metadata / prospectus-disclosure standard** (APY, liquidity, instrument data) — an acknowledged gap; would be a separate standard.

## Access control & admin

- 🟢 **Authority handoff** — two-step invite/accept.
- 🟡 **Complex role-based access** — out of the vault's scope; point the authority at a PDA from an external RBAC program. (A reusable RBAC program has early designs only.)
- 🟡 **Admin timelocks** — generally out of scope; handled by the external RBAC tooling above. A timelock on authority change was proposed as a low-lift spec'd extension — caveat: a compromised authority can act instantly via `WithdrawAssets`/`UpdateNav` anyway, so a multisig authority remains the real mitigation.

## Build & performance

- 🟢 **Anchor** for development speed; intentionally unoptimized. Codama (clients), LiteSVM (tests).
- 🟡 **Perf path** — the likely approaches are switching to Pinocchio or moving to zerocopy for program-owned accounts (and catching Anchor 2 gains).

## Explicitly out of scope

- 🟡 **Referrals** — excluded; adds significant complexity and is better served by existing providers.
- 🟡 **Swap-and-deposit in one transaction** — excluded.

---

## Disclaimer

The content herein is provided for educational, informational, and entertainment purposes only, and does not constitute an offer to sell or a solicitation of an offer to buy any securities, options, futures, or other derivatives related to securities in any jurisdiction, nor should not be relied upon as advice to buy, sell or hold any of the foregoing. This content is intended to be general in nature and is not specific to you, the user or anyone else. You should not make any decision, financial, investment, trading or otherwise, based on any of the information presented without undertaking independent due diligence and consultation with a professional advisor. Solana Foundation Foundation and its agents, advisors, council members, officers and employees (the "Foundation Parties") make no representation or warranties, expressed or implied, as to the accuracy of the information herein and expressly disclaims any and all liability that may be based on such information or any errors or omissions therein. The Foundation Parties shall have no liability whatsoever, under contract, tort, trust or otherwise, to any person arising from or related to the content or any use of the information contained herein by you or any of your representatives. All opinions expressed herein are the speakers' own personal opinions and do not reflect the opinions of any entities.
