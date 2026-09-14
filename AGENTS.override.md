# Central documentation entry point

Read the complete [repository rules](AGENTS.md) before starting. Continue to
follow their commands, constraints, and any rules in deeper directories. This
file only adds workspace navigation; it does not replace or relax those rules.

Read these central documents before changing code:

- [Documentation index](../../../docs/README.md), [AI workflow](../../../docs/engineering/ai-workflow.md), and [documentation ownership ADR](../../../docs/architecture/decisions/ADR-0002-central-authored-documentation.md).
- [Repository map](../../../docs/architecture/repository-map.md) and [tool boundaries](../../../docs/architecture/tool-boundaries.md).
- Repository modules: [backtesting](../../../docs/modules/backtesting/README.md), [execution](../../../docs/modules/execution/README.md), and [orderbook](../../../docs/modules/orderbook/README.md). Read the relevant invariants before making changes.
- [Ticket guide](../../../docs/tracking/README.md), [board](../../../docs/tracking/board.md), and [ticket template](../../../docs/templates/ticket.md). Start from an existing ticket and update the same ticket with delivery and validation evidence.

Maintain authored architecture, experiment, optimization, debugging, and
operations records in the central docs above. Keep source code, executable
schemas, tests, and original upstream documentation in this repository. Record
checks actually run separately from checks that are only recommended.

Links are relative to this repository root. In the standard layout, the
workspace root is `../../..`. For a standalone clone, locate the workspace
management repository or use the operator-provided `WORKSPACE_DOCS_ROOT`. If
the central files cannot be read, report the gap; do not create a duplicate set
of rules or claim that they were read.

See the [entry-point record](../../../docs/governance/repository-ai-entrypoints.md)
for maintenance and validation. Run only repository checks required by the
current change; publication follows the separately authorized workflow.
