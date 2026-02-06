; Minimal HCL / Terraform highlighting.
;
; The upstream tree-sitter-hcl grammar does not ship highlight queries, so we
; keep this conservative to avoid query drift while still giving useful color.

(comment) @comment

(bool_lit) @boolean
(numeric_lit) @number

(string_lit) @string
(quoted_template) @string
(heredoc_template) @string
(template_literal) @string

; Identifier-like nodes in common positions.
(function_call (identifier) @function)
(attribute (identifier) @property)
(block (identifier) @type)
(get_attr (identifier) @property)

; Operators / punctuation that appear in typical Terraform configs.
[
  "="
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "&&"
  "!"
  "."
  "=>"
  ":"
  "?"
  ","
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

