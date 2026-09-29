; Keywords

[
  "editor"
  "blueprint"
  "enum"
  "struct"
  "interface"
  "at"
  "use"
  "implements"
  "editable"
  "var"
  "dispatcher"
  "component"
  "default"
  "fn"
  "event"
  "category"
  "widget"
  "size"
  "slot"
  "on"
  "ref"
  "out"
  "private"
  "protected"
  "pure"
  "mut"
] @keyword

[
  "let"
  "while"
  "for"
  "in"
  "if"
  "match"
  "else"
  "return"
  "break"
  "continue"
] @keyword

[
  "array"
  "set"
  "map"
  "class"
  "soft"
] @type.builtin

; Literals

(string) @string
(integer) @number
(float) @number
(boolean) @boolean
(none) @constant.builtin
(path) @string.special
(comment) @comment

((identifier) @variable.builtin
  (#any-of? @variable.builtin "self" "super"))

; Declarations

(header
  name: (identifier) @type)

(enum_declaration
  name: (identifier) @type)

(enum_value
  name: (identifier) @constant)

(struct_declaration
  name: (identifier) @type)

(struct_field
  name: (identifier) @property)

(interface_declaration
  name: (identifier) @type)

(interface_function
  name: (identifier) @function)

(function_declaration
  name: (identifier) @function)

(event_handler
  event: (identifier) @function)

(custom_event_declaration
  name: (identifier) @function)

(labelled_statement
  label: (identifier) @label)

(break_statement
  label: (identifier) @label)

(continue_statement
  label: (identifier) @label)

(parameter
  name: (identifier) @variable.parameter)

(variable_declaration
  name: (identifier) @property)

(widget_declaration
  name: (identifier) @property)

(dispatcher_declaration
  name: (identifier) @function)

(component_declaration
  name: (identifier) @property)

(event_handler
  component: (identifier) @property)

(category_declaration
  name: (string) @label)

; Types

(type_identifier) @type

; Calls and members

(call
  function: (identifier) @function.call)

(method_call
  method: (identifier) @function.method)

(make
  struct: (identifier) @type)

(field_initializer
  name: (identifier) @property)

(member_expression
  member: (identifier) @property)

; A capitalised name where an object would go is a class, as in
; GameplayStatics.GetActorOfClass(Actor).
(member_expression
  object: (identifier) @type
  (#match? @type "^[A-Z]"))

(method_call
  object: (identifier) @type
  (#match? @type "^[A-Z]"))

(arguments
  (identifier) @type
  (#match? @type "^[A-Z]"))

; Operators and punctuation

[
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "&="
  "|="
  "->"
  ".."
  "||"
  "&&"
  "=="
  "!="
  "<="
  ">="
  "|"
  "&"
  "+"
  "-"
  "*"
  "/"
  "%"
  "!"
] @operator

; `<` and `>` compare in an expression, and bracket the element type of an array type.
(binary_expression
  operator: ["<" ">"] @operator)

(array_type
  ["<" ">"] @punctuation.bracket)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  "."
  ":"
] @punctuation.delimiter
