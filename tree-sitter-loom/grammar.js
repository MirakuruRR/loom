/**
 * loom's syntax for tree-sitter, following packages/loom/src/syntax.rs. The compiler is the authority:
 * where the two disagree, this grammar is wrong. Newlines end items and statements, as they do
 * there, and may appear freely only inside array and map literals.
 */

// Binding strength of the binary operators, as in syntax.rs: bitwise operators bind tighter than
// comparisons, so `flags & 32 != 0` reads as meant.
const PREC = {
  or: 1,
  and: 2,
  equality: 3,
  compare: 4,
  bit_or: 5,
  bit_and: 6,
  add: 7,
  multiply: 8,
  unary: 9,
  postfix: 10,
};

const commaSep = (rule) => optional(seq(rule, repeat(seq(',', rule)), optional(',')));

module.exports = grammar({
  name: 'loom',

  word: ($) => $.identifier,

  // The words that start a statement or a value, as syntax::RESERVED and the literal words are.
  // Every other keyword is one only where the grammar expects it, and a name anywhere else.
  reserved: {
    global: (_) => ['break', 'continue', 'else', 'for', 'if', 'let', 'match', 'return', 'while', 'true', 'false', 'none'],
  },

  extras: ($) => [/[ \t\r]/, $.comment],

  // Whether the braces in `if x { y }`, or in a match's arm, hold a block or a value is only
  // known at their end.
  conflicts: ($) => [[$.expression_statement, $._braced_expression]],

  rules: {
    source_file: ($) =>
      seq(repeat($._newline), choice(seq($.header, repeat(choice($._item, $._newline))), seq(choice($.enum_declaration, $.struct_declaration, $.interface_declaration), repeat($._newline)))),

    // An enum's source holds only the enum, one value on each line.
    enum_declaration: ($) =>
      seq(
        'enum',
        field('name', $.identifier),
        'at',
        field('path', $.path),
        '{',
        repeat($._newline),
        optional(seq($.enum_value, repeat(seq(repeat1($._newline), $.enum_value)), repeat($._newline))),
        '}',
      ),

    // A struct's source holds only the struct, one field on each line.
    struct_declaration: ($) =>
      seq(
        'struct',
        field('name', $.identifier),
        'at',
        field('path', $.path),
        '{',
        repeat($._newline),
        optional(seq($.struct_field, repeat(seq(repeat1($._newline), $.struct_field)), repeat($._newline))),
        '}',
      ),

    struct_field: ($) =>
      seq(field('name', $.identifier), ':', field('type', $._type), optional(seq('=', field('value', $._expression)))),

    // An interface's source holds only the interface, one function's signature on each line.
    interface_declaration: ($) =>
      seq(
        'interface',
        field('name', $.identifier),
        'at',
        field('path', $.path),
        '{',
        repeat($._newline),
        optional(seq($.interface_function, repeat(seq(repeat1($._newline), $.interface_function)), repeat($._newline))),
        '}',
      ),

    interface_function: ($) =>
      seq('fn', field('name', $.identifier), field('parameters', $.parameters), optional(seq('->', field('return_type', $._type)))),

    // A value, and what the editor shows for it when that is not its name.
    enum_value: ($) => seq(field('name', $.identifier), optional(field('label', $.string))),

    header: ($) =>
      seq(optional('editor'), 'blueprint', field('name', $.identifier), ':', field('parent', $._type), 'at', field('path', $.path), $._newline),

    _item: ($) =>
      seq(
        choice(
          $.use_declaration,
          $.implements_declaration,
          $.variable_declaration,
          $.dispatcher_declaration,
          $.component_declaration,
          $.default_declaration,
          $.function_declaration,
          $.custom_event_declaration,
          $.category_declaration,
          $.widget_declaration,
          $.event_handler,
        ),
        $._newline,
      ),

    // A folder the source takes the types it names by their short names from.
    use_declaration: ($) => seq('use', field('folder', $.path)),

    // An interface the Blueprint implements, whose functions its handlers may implement.
    implements_declaration: ($) => seq('implements', field('interface', $._type)),

    // A private variable is read and set only by this Blueprint, and is never editable.
    variable_declaration: ($) =>
      seq(optional(choice('editable', field('access', 'private'))), 'var', field('name', $.identifier), ':', field('type', $._type), optional(seq('=', field('value', $._expression)))),

    function_declaration: ($) =>
      seq(
        // Who may call it, then whether it is pure, in that order.
        optional(field('access', choice('private', 'protected'))),
        optional(seq('pure', optional('mut'))),
        'fn',
        field('name', $.identifier),
        field('parameters', $.parameters),
        optional(seq('->', field('return_type', $._type))),
        field('body', $.block),
      ),

    // A custom event: called as a function is, it gives nothing back and may wait.
    custom_event_declaration: ($) =>
      seq('event', field('name', $.identifier), optional(field('parameters', $.parameters)), field('body', $.block)),

    dispatcher_declaration: ($) => seq('dispatcher', field('name', $.identifier), field('parameters', $.parameters)),

    component_declaration: ($) =>
      seq(
        'component',
        field('name', $.identifier),
        ':',
        field('class', $._type),
        optional($.field_initializers),
      ),

    category_declaration: ($) => seq('category', field('name', $.string)),

    // A property the parent declares, or a field inside one, given this Blueprint's default.
    default_declaration: ($) =>
      seq(
        'default',
        field('name', $.identifier),
        repeat(seq('.', field('field', $.identifier))),
        '=',
        field('value', $._expression),
      ),

    // Placed on the root canvas with `at x y size w h`, or without, the tree's root. `slot(...)`
    // sets the fields of the slot its parent holds it in, and its block holds its children.
    widget_declaration: ($) =>
      seq(
        'widget',
        field('name', $.identifier),
        ':',
        field('class', $._type),
        optional($.field_initializers),
        optional(
          seq(
            'at',
            field('x', $.integer),
            field('y', $.integer),
            'size',
            field('width', $.integer),
            field('height', $.integer),
          ),
        ),
        optional(seq('slot', field('slot', $.field_initializers))),
        optional(field('children', $.widget_block)),
      ),

    // A widget's children, each ended by a newline, or by the closing brace.
    widget_block: ($) =>
      seq(
        '{',
        repeat($._newline),
        optional(seq($.widget_declaration, repeat(seq(repeat1($._newline), $.widget_declaration)), repeat($._newline))),
        '}',
      ),

    // `(field: value, ...)`, which may span lines.
    field_initializers: ($) =>
      seq(
        '(',
        repeat($._newline),
        optional(
          seq(
            $.field_initializer,
            repeat(seq(repeat($._newline), ',', repeat($._newline), $.field_initializer)),
            repeat($._newline),
            optional(seq(',', repeat($._newline))),
          ),
        ),
        ')',
      ),

    event_handler: ($) =>
      seq(
        'on',
        optional(seq(field('component', $.identifier), '.')),
        field('event', $.identifier),
        optional(field('parameters', $.parameters)),
        optional(seq('->', field('return_type', $._type))),
        field('body', $.block),
      ),

    parameters: ($) => seq('(', commaSep($.parameter), ')'),

    // `out` before the name makes the parameter an output of the function, which a call does not
    // pass.
    // `out` before a name makes an output, and `out` before the colon is the name.
    parameter: ($) =>
      seq(
        optional('out'),
        field('name', choice($.identifier, alias('out', $.identifier))),
        ':',
        optional('ref'),
        field('type', $._type),
        optional(seq('=', field('default', $._expression))),
      ),

    _type: ($) => choice($.type_identifier, $.path, $.array_type, $.set_type, $.map_type, $.class_type, $.soft_type),

    type_identifier: ($) => $.identifier,

    array_type: ($) => seq('array', '<', field('element', $._type), '>'),

    set_type: ($) => seq('set', '<', field('element', $._type), '>'),

    map_type: ($) => seq('map', '<', field('key', $._type), ',', field('value', $._type), '>'),

    // `class<Actor>`: a class, this one or a subclass.
    class_type: ($) => seq('class', '<', field('class', $._type), '>'),

    // `soft<Texture2D>` or `soft<class<Actor>>`: a reference by path, loaded when asked.
    soft_type: ($) => seq('soft', '<', field('target', $._type), '>'),

    // A block's statements, each ended by a newline, or by the closing brace.
    block: ($) =>
      seq(
        '{',
        repeat($._newline),
        optional(seq($._statement, repeat(seq(repeat1($._newline), $._statement)), repeat($._newline))),
        '}',
      ),

    _statement: ($) =>
      choice(
        $.let_statement,
        $.while_statement,
        $.for_statement,
        $.labelled_statement,
        $.if_statement,
        $.match_statement,
        $.return_statement,
        $.break_statement,
        $.continue_statement,
        $.assignment,
        $.expression_statement,
      ),

    let_statement: ($) =>
      seq('let', field('name', $.identifier), optional(seq(':', field('type', $._type))), '=', field('value', $._expression)),

    while_statement: ($) => seq('while', field('condition', $._expression), field('body', $.block)),

    // `rows: for ...` or `rows: while ...`, which a break or a continue inside may name.
    labelled_statement: ($) =>
      seq(field('label', $.identifier), ':', field('body', choice($.for_statement, $.while_statement))),

    // `for item in array { }`, `for item, i in array { }` or `for key, value in map { }`.
    for_statement: ($) =>
      seq(
        'for',
        field('name', $.identifier),
        optional(seq(',', field('name', $.identifier))),
        'in',
        field('items', $._expression),
        // A count, from the number before `..` to the one after, both included.
        optional(seq('..', field('last', $._expression))),
        field('body', $.block),
      ),

    if_statement: ($) =>
      seq(
        'if',
        field('condition', $._expression),
        field('consequence', $.block),
        optional(seq('else', field('alternative', choice($.if_statement, $.block)))),
      ),

    // `match value { A, B { } else { } }`.
    match_statement: ($) =>
      seq(
        'match',
        field('value', $._expression),
        '{',
        repeat($._newline),
        repeat(seq($.match_arm, repeat($._newline))),
        optional(seq(alias($.match_else, $.match_arm), repeat($._newline))),
        '}',
      ),

    match_arm: ($) => seq(field('value', $._expression), repeat(seq(',', field('value', $._expression))), field('body', $.block)),

    match_else: ($) => seq('else', field('body', $.block)),

    return_statement: ($) => prec.right(seq('return', optional(field('value', $._expression)))),

    // Without a label, the innermost loop.
    break_statement: ($) => seq('break', optional(field('label', $.identifier))),

    continue_statement: ($) => seq('continue', optional(field('label', $.identifier))),

    assignment: ($) =>
      seq(field('target', $._expression), field('operator', choice('=', '+=', '-=', '*=', '/=', '%=', '&=', '|=')), field('value', $._expression)),

    expression_statement: ($) => $._expression,

    _expression: ($) =>
      choice(
        $.integer,
        $.float,
        $.string,
        $.boolean,
        $.none,
        $.identifier,
        $.path,
        $.array,
        $.map,
        $.parenthesized_expression,
        $.unary_expression,
        $.binary_expression,
        $.call,
        $.make,
        $.member_expression,
        $.method_call,
        $.index_expression,
        $.if_expression,
        $.match_expression,
      ),

    // `if ok { a } else { b }`: a value, where an if_statement is a statement.
    if_expression: ($) =>
      seq(
        'if',
        field('condition', $._expression),
        field('consequence', $._braced_expression),
        'else',
        field('alternative', choice($.if_expression, $._braced_expression)),
      ),

    _braced_expression: ($) => seq('{', repeat($._newline), $._expression, repeat($._newline), '}'),

    // `match value { A { a } else { b } }`: a value, where a match_statement is a statement.
    match_expression: ($) =>
      seq(
        'match',
        field('value', $._expression),
        '{',
        repeat($._newline),
        repeat(seq(alias($.match_expression_arm, $.match_arm), repeat($._newline))),
        alias($.match_expression_else, $.match_arm),
        repeat($._newline),
        '}',
      ),

    match_expression_arm: ($) =>
      seq(field('value', $._expression), repeat(seq(',', field('value', $._expression))), field('body', $._braced_expression)),

    match_expression_else: ($) => seq('else', field('body', $._braced_expression)),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    unary_expression: ($) => prec(PREC.unary, seq(field('operator', choice('-', '!')), field('operand', $._expression))),

    binary_expression: ($) => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.equality, choice('==', '!=')],
        [PREC.compare, choice('<', '<=', '>', '>=')],
        [PREC.bit_or, '|'],
        [PREC.bit_and, '&'],
        [PREC.add, choice('+', '-')],
        [PREC.multiply, choice('*', '/', '%')],
      ];
      return choice(
        ...table.map(([p, op]) =>
          prec.left(p, seq(field('left', $._expression), field('operator', op), field('right', $._expression))),
        ),
      );
    },

    // `Name(args)`: a call to a function of this Blueprint, its parent, or a builtin.
    call: ($) => seq(field('function', $.identifier), field('arguments', $.arguments)),

    arguments: ($) => seq('(', commaSep($._expression), ')'),

    // `Struct(field: value, ...)`: a struct made from the fields given.
    // At least one field: `Name()` is a call.
    make: ($) =>
      seq(
        field('struct', $.identifier),
        '(',
        repeat($._newline),
        $.field_initializer,
        repeat(seq(repeat($._newline), ',', repeat($._newline), $.field_initializer)),
        repeat($._newline),
        optional(seq(',', repeat($._newline))),
        ')',
      ),

    field_initializer: ($) => seq(field('name', $.identifier), ':', field('value', $._expression)),

    member_expression: ($) =>
      prec(PREC.postfix, seq(field('object', $._expression), '.', field('member', $.identifier))),

    method_call: ($) =>
      prec(
        PREC.postfix,
        seq(field('object', $._expression), '.', field('method', $.identifier), field('arguments', $.arguments)),
      ),

    index_expression: ($) =>
      prec(PREC.postfix, seq(field('array', $._expression), '[', field('index', $._expression), ']')),

    // `[]` or `[a, b, c]`, which may span lines.
    array: ($) =>
      seq(
        '[',
        repeat($._newline),
        optional(
          seq(
            $._expression,
            repeat(seq(repeat($._newline), ',', repeat($._newline), $._expression)),
            repeat($._newline),
            optional(seq(',', repeat($._newline))),
          ),
        ),
        ']',
      ),

    // `{}` or `{key: value, ...}`, which may span lines.
    map: ($) =>
      seq(
        '{',
        repeat($._newline),
        optional(
          seq(
            $.map_entry,
            repeat(seq(repeat($._newline), ',', repeat($._newline), $.map_entry)),
            repeat($._newline),
            optional(seq(',', repeat($._newline))),
          ),
        ),
        '}',
      ),

    map_entry: ($) => seq(field('key', $._expression), ':', field('value', $._expression)),

    boolean: (_) => choice('true', 'false'),

    none: (_) => 'none',

    // A name, or any name in backticks, such as `if` or `Scale Sample Count`.
    identifier: (_) => token(choice(/[A-Za-z_][A-Za-z0-9_]*/, /`[^`\n\r]+`/)),

    // An asset or class path: a slash and a capital, such as /Script/Engine.Actor or /Game/Things/X.
    path: (_) => token(seq('/', /[A-Z]/, /[A-Za-z0-9_\/.\-]*/)),

    integer: (_) => token(choice(/0[xX][0-9a-fA-F]+/, /[0-9]+/)),

    float: (_) => token(/[0-9]+\.[0-9]+/),

    string: (_) => token(seq('"', repeat(choice(/[^"\\\n]/, /\\["\\n]/)), '"')),

    comment: (_) => token(seq('//', /[^\n]*/)),

    _newline: (_) => /\n/,
  },
});
