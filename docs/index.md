# Rusty-Soar

Rusty-Soar is a Rust port of [Soar](https://soar.eecs.umich.edu/), the cognitive architecture
from the University of Michigan. It has two goals:

- **Behave exactly like Soar.** Rusty-Soar follows the C++ Soar kernel algorithm for algorithm,
  so results from research done with Soar carry over.
- **Be provable.** The kernel is written in safe, `no_std` Rust with fixed-size memory, so that
  tools like [Kani](https://model-checking.github.io/kani/) can prove properties about agents.

<div class="grid cards" markdown>

-   **New here?**

    ---

    Start with [Getting Started](getting-started/index.md). In about half an hour you will
    install Rusty-Soar, run an agent, and connect one to your own program.

-   **New to Soar?**

    ---

    Read [Soar in Ten Minutes](guides/soar-basics.md) for the ideas behind working memory,
    operators and impasses.

-   **Moving agents from C++ Soar?**

    ---

    See [What's Ported](reference/status.md) and
    [Differences from C++ Soar](reference/differences.md) first.

-   **Looking something up?**

    ---

    The [Reference](reference/cli.md) section lists every command, setting and function.

</div>

!!! warning "Rusty-Soar is in active development"

    The core decision cycle is ported, along with reinforcement learning. Some parts of Soar are
    not yet ported (chunking, semantic and episodic memory, among others), and the formal
    proofs are still in progress. [What's Ported](reference/status.md) has the details.
