(function_declaration
  "fn" @context
  name: (identifier) @name) @item

(event_handler
  "on" @context
  event: (identifier) @name) @item

(variable_declaration
  "var" @context
  name: (identifier) @name) @item

(widget_declaration
  "widget" @context
  name: (identifier) @name) @item

(dispatcher_declaration
  "dispatcher" @context
  name: (identifier) @name) @item

(custom_event_declaration
  "event" @context
  name: (identifier) @name) @item
