# Introduction

A minimal flow that runs one nushell task and reports a known constant. Used as the fixture for external-api happy-path tests.

:::stage { name: "stub-stage" }

:::step { name: "stub-step", task: "stub-task" }
:::

:::

:::nushell { name: "stub-task", tags: [coder] }
# Predictable stub task for the external-api happy-path test.
# Returns a fixed payload + a `done` signal so the dispatch runner marks
# the task `ok`. The test asserts payload.constant == 42 against the
# attempt's persisted context_data.
{
    payload: {
        echo: "stub-flow",
        constant: 42
    },
    signals: {
        done: true
    }
}
:::
