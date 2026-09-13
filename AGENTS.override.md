# Central Documentation Entry Point

Before starting, read the [repository rules](AGENTS.md) in full and continue to follow
their commands, constraints, and directory-specific rules. This file adds workspace
navigation; it does not replace or relax the existing rules.

Read these central documents before modifying code:

- [Central index](../../../docs/README.md), [AI workflow](../../../docs/engineering/ai-workflow.md), and [documentation ownership ADR](../../../docs/architecture/decisions/ADR-0002-central-authored-documentation.md).
- [Repository map](../../../docs/architecture/repository-map.md) and [tool boundaries](../../../docs/architecture/tool-boundaries.md).
- Repository modules: [backtesting](../../../docs/modules/backtesting/README.md), [execution](../../../docs/modules/execution/README.md), and [orderbook](../../../docs/modules/orderbook/README.md). Read the relevant invariants before editing.
- [Ticket guidance](../../../docs/tracking/README.md), [shared board](../../../docs/tracking/board.md), and [ticket template](../../../docs/templates/ticket.md). Start from an existing ticket and update that same ticket with verification evidence at delivery.

Maintain authored architecture, experiment, optimization, troubleshooting, and operations
records in the central docs above. This repository retains source code, executable schemas,
tests, and existing upstream documentation. Record verification actually performed
separately from recommended verification.

Links are relative to this repository root; in the standard layout, the workspace root
is `../../..`. For a standalone clone elsewhere, first locate the workspace management
repository or use the operator-provided `WORKSPACE_DOCS_ROOT`. If central files cannot
be read, explicitly report the gap; do not invent replacement policies or claim to have
read those files.

See the [entry-point record](../../../docs/governance/repository-ai-entrypoints.md) for
maintenance and verification. Run only the repository checks needed for the current
change; publication follows the existing authorization process separately.
