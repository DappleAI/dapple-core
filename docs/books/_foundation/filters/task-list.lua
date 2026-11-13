-- Pandoc Lua filter for task lists and emoji shortcodes

-- Emoji shortcode to simple text/symbol mapping
local emoji_map = {
  [":chart_with_upwards_trend:"] = "[trend]",
  [":bar_chart:"] = "[chart]",
  [":wrench:"] = "[tools]",
  [":robot:"] = "[auto]",
  [":ledger:"] = "[log]",
  [":electric_plug:"] = "[plugin]",
  [":construction:"] = "[wip]",
  [":quality_first:"] = "[quality]",
  [":quality_experiment:"] = "[experiment]",
  [":cost_optimized:"] = "[optimized]",
}

function Str(elem)
  -- Convert emoji shortcodes to descriptive text
  for shortcode, text in pairs(emoji_map) do
    if elem.text == shortcode then
      return pandoc.RawInline('latex', '{\\color{dapplegreen}\\textbf{' .. text .. '}}')
    end
  end
  return elem
end

-- Handle the {[} {]} checkbox pattern that pandoc creates
function Inlines(inlines)
  local i = 1
  while i <= #inlines - 2 do
    -- Look for pattern: Str("{[}"), Space, Str("{]}")  -> unchecked
    if inlines[i].t == "Str" and inlines[i].text == "{[}" and
       inlines[i+1].t == "Space" and
       inlines[i+2].t == "Str" and inlines[i+2].text == "{]}" then
      inlines:remove(i+2)
      inlines:remove(i+1)
      inlines[i] = pandoc.RawInline('latex', '\\uncheckedbox{}')
    -- Look for pattern: Str("{[}"), Str("x"), Str("{]}")  -> checked
    elseif inlines[i].t == "Str" and inlines[i].text == "{[}" and
           inlines[i+1].t == "Str" and (inlines[i+1].text == "x" or inlines[i+1].text == "X") and
           inlines[i+2].t == "Str" and inlines[i+2].text == "{]}" then
      inlines:remove(i+2)
      inlines:remove(i+1)
      inlines[i] = pandoc.RawInline('latex', '\\checkedbox{}')
    else
      i = i + 1
    end
  end
  return inlines
end
