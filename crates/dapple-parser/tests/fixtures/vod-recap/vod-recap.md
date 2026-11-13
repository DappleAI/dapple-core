# VOD Recap

A deliberately tiny guide: one step whose task chains the media-gateway VOD
commands, and one computed brief-output that renders the reel as a video. The
full gaming-VOD guide lives in dapple-guides; this fixture exists only to
exercise the parser.

:::value-slot { name: "recap", kind: "asset" }
:::

:::brief-markdown { name: "recap-intro" }
Dapple cuts the reel from the VOD: card, clip, card, clip, your own audio intact.
:::
:::brief-output { name: "recap-output", slot: "recap", source: "computed", source_path: "/recap", display: "video", title: "The recap" }
:::
:::brief { name: "recap-brief", units: ["recap-intro","recap-output"] }
:::
:::brief-group { name: "recap-group", briefs: ["recap-brief"] }
:::
:::brief-pack { name: "vod-recap-pack", groups: ["recap-group"] }
:::

:::stage { name: "cut" }

:::step { name: "assemble", task: "recap-cut", expose: ["recap"] }
:::

:::

:::nushell { name: "recap-cut", tags: [coder] }
# analyze the VOD -> the editor proposes beats -> assemble the reel
let a = ("https://twitch.tv/videos/1" | vod analyze --initial-prompt "handle game terms")
let beats = (($a | prompt send "editor").text)
{ payload: { recap: ($beats | vod assemble --source $a.url) }, signals: {} }
:::
