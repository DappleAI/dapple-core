# Crosstalk

A deliberately tiny guide: one step whose task orchestrates the conduit media
commands. The director writes the conversation, a voice speaks each line, and
`av mix` overlaps the clips. The full Crosstalk guide lives in dapple-guides;
this fixture exists only to exercise the parser.

:::stage { name: "converse" }

:::step { name: "run", task: "orchestrator" }
:::

:::

:::nushell { name: "orchestrator", tags: [coder] }
# director writes it -> a voice speaks each line -> av mix overlaps the clips
let line = (({} | prompt send "director").text)
let clip = ($line | synth "voice")
{ payload: { audio: ([$clip] | av mix) }, signals: {} }
:::
