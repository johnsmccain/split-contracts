# Contributing to split-contracts

Thank you for your interest in contributing to StellarSplit! This repo is part of the [Drips Wave Program](https://drips.network/wave) — a monthly open-source bounty program run by the Stellar Development Foundation.

## Before You Start

**Do not begin coding until you have been assigned to an issue by a maintainer.**

1. Browse [open issues](../../issues) and find one labelled `good first issue` or matching your skill level.
2. Comment on the issue: "I'd like to work on this."
3. Wait for a maintainer to assign you. Only then should you fork and start coding.

## Workflow

### 1. Fork & Clone

```bash
git clone https://github.com/<your-username>/split-contracts.git
cd split-contracts
```

### 2. Create a Branch

Branch names must follow this pattern:

```
fix/issue-NUMBER-short-description
feat/issue-NUMBER-short-description
```

Examples:
- `fix/issue-3-refund-edge-case`
- `feat/issue-7-add-partial-release`

```bash
git checkout -b fix/issue-42-short-description
```

### 3. Make Your Changes

- Write clean, well-commented Rust code.
- Add or update tests in `contracts/split/src/test.rs`.
- Run `cargo test --workspace` and ensure all tests pass.
- Run `cargo clippy` and fix any warnings.
- Run `cargo fmt` to format your code.

### 4. Commit

Use conventional commits:

```
fix: correct refund logic when deadline is exact ledger timestamp (#42)
feat: add partial release function (#7)
```

### 5. Open a Pull Request

- Title: concise, under 70 characters.
- Description: what changed, why, and how you tested it.
- Reference the issue: `Closes #42`
- Do not open a PR without a linked issue.

## Code Standards

- All public functions must have Rust doc comments (`///`).
- No `unwrap()` in production code paths — use `expect("descriptive message")` or proper error handling.
- Keep functions small and focused.

## Questions?

Open a [Discussion](../../discussions) or ask in the issue thread.
