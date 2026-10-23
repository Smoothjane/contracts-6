# Security Policy

These contracts move real money. Please treat vulnerabilities seriously and
report them privately.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/StellarPOS-App/contracts/security/advisories/new).
Do **not** open a public issue, pull request, or discussion.

Please include:

- The affected contract and function
- A description of the impact (loss of funds, frozen funds, auth bypass, ...)
- A reproduction, ideally a failing test using `soroban_sdk::testutils`
- Any suggested fix

We aim to acknowledge reports within 3 business days and to agree a
disclosure timeline with you before any details become public.

## Scope

In scope: everything under `contracts/`, and `scripts/deploy.sh` where it
affects the security of a deployment.

Out of scope: issues in `soroban-sdk`, the Stellar network, or the Stellar
CLI itself. Report those upstream to the
[Stellar Development Foundation](https://stellar.org/grants-and-funding/bug-bounty).

## Supported versions

Only the latest release on `main` receives security fixes. The contracts
have **not** been externally audited yet. Do not deploy to mainnet with
significant value before an audit.

## Security properties we rely on

Report any way to break these:

1. The payment processor never holds customer funds after a transaction.
2. A `(merchant, order_ref)` pair can be paid at most once.
3. Only a payment's settlement account can authorize its refund, and total
   refunds never exceed the gross amount.
4. No treasury withdrawal executes without `threshold` approvals from current
   signers.
5. Escrowed funds always leave through release, claim, refund, or a dispute
   ruling. No path strands them.
