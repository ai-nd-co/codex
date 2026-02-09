; XML highlighting query (adapted from tree-sitter-xml).
;
; This version avoids capture names outside Codex TUI's HIGHLIGHT_NAMES.

; XML declaration
"xml" @keyword

[ "version" "encoding" "standalone" ] @property
(EncName) @string.special
(VersionNum) @number
[ "yes" "no" ] @boolean

; Processing instructions
(PI) @embedded
(PI (PITarget) @keyword)

; Tags
(STag (Name) @tag)
(ETag (Name) @tag)
(EmptyElemTag (Name) @tag)

; Attributes
(Attribute (Name) @property)
(Attribute (AttValue) @string)

; Entities
(EntityRef) @constant
((EntityRef) @constant.builtin
 (#any-of? @constant.builtin
   "&amp;" "&lt;" "&gt;" "&quot;" "&apos;"))
(CharRef) @constant

; Delimiters & punctuation
[
 "<?" "?>"
 "<!" "]]>"
 "<" ">"
 "</" "/>"
] @punctuation

