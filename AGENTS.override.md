# Central documentation entry point

Before starting, read the [original repository rules](AGENTS.md) in full. Continue to follow
their commands, constraints, and directory-specific rules. This file adds workspace navigation;
it does not replace or relax the original rules.

Read these central documents before modifying code:

- [Entry point](../../../docs/README.md), [AI workflow](../../../docs/engineering/ai-workflow.md), and [documentation ownership ADR](../../../docs/architecture/decisions/ADR-0002-central-authored-documentation.md).
- [Repository map](../../../docs/architecture/repository-map.md) and [tool boundaries](../../../docs/architecture/tool-boundaries.md).
- Repository modules: [backtesting](../../../docs/modules/backtesting/README.md), [execution](../../../docs/modules/execution/README.md), and [orderbook](../../../docs/modules/orderbook/README.md). Read the relevant invariants before making changes.
- [Ticket guide](../../../docs/tracking/README.md), [shared board](../../../docs/tracking/board.md), and [ticket template](../../../docs/templates/ticket.md). Start from an existing ticket and update that same ticket with validation evidence at delivery.

Maintain authored architecture, experiment, optimization, troubleshooting, and operations records
in the central docs above. This repository retains source, executable schemas, tests, and original
upstream documentation. Record completed validation separately from recommended validation.

Links are relative to this file's repository root; in the standard layout the workspace root is
`../../..`. For a standalone clone elsewhere, first locate the workspace management repository or
read the operator-provided `WORKSPACE_DOCS_ROOT`. If central files are unavailable, explicitly
report the gap; do not invent replacement rules or claim to have read them.

See the [integration record](../../../docs/governance/repository-ai-entrypoints.md) for entry-point
maintenance and verification. Run only repository checks needed for the current change; publishing
follows the existing authorization process separately.
