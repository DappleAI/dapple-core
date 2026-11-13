Prompt Templates (examples)

## Resume data extractor (template) - file: prompts/resume-data-extractor/template.txt

```
SYSTEM: You are a structured data extractor. Extract name, email, and work_history from user text.
USER: "{{user_text}}"
ASSISTANT: Return valid JSON with keys: name (string), email (string), work_history (array of objects with fields: role,company,years).
```

Example expected output - file: prompts/resume-data-extractor/expected_output.json

```json
{
  "name": "Jane Doe",
  "email": "jane@example.com",
  "work_history": [
    {"role": "Software Engineer", "company": "ACME", "years": "3 years"}
  ]
}
```

## Resume formatter (LLM-assisted) - file: prompts/resume-formatter/template.txt

```
SYSTEM: You are a resume formatter. Given structured JSON input, produce a Markdown resume following this style guide:
- Top-level heading with the candidate name
- Contact line with email
- Sections for Work History (bulleted)

USER: {{input_json}}
ASSISTANT: Produce Markdown output only.
```

Prompt harness usage (quick)

- Use the `prompt-harness.sh` script (described in the Prompt Authoring chapter) to substitute `{{user_text}}` or `{{input_json}}` into the templates for manual testing.
