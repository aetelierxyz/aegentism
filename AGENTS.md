# AGENTS.md

Rules for any agent working in this repository.

## Scope

- This repository holds agentic trading programs built on the Aetelier platform. Strategy logic lives here and only here.
- Platform repositories (`aetelier-*`) stay neutral: no strategy logic goes into them.
- A capability the platform lacks is added there as a neutral feature, in its own piece of work agreed with the operator.
- A capability that exists in the platform, or belongs there, is not rebuilt here.

## Hard limits

- Never read, print, log or ask for API keys or other credentials. Programs read them from the environment at run time.
- Never start a process that trades real funds. The operator starts every live run.
- Never change account-wide exchange settings.
- Never commit, push or open a pull request. The operator does that.
- Never download data without the operator's go-ahead.

## Files

- No comment lines in code, scripts or commands.
- No mention of authorship or of the tools used to write a file.
- No planning, status or milestone text in repository files.
- A statement about a venue carries its status: observed, documented, reported, derived, assumed or unverified.

## Entry points

| Program | Definition | Instruction |
|---|---|---|
| aegent-mm v0.1.0 | `aegent-mm/v0.1.0/definition.md` | `aegent-mm/v0.1.0/instruction.md` |

- A definition is the approved strategy core. It changes only by an amendment the operator approves.
