---
tags:
  - hyperlink
---
# `hyperlink_rules`

Defines rules to match text from the terminal output and generate
clickable links.

The value is a list of rule entries. Each entry has the following fields:

* `regex` - the regular expression to match (see supported [Regex syntax](https://docs.rs/regex/latest/regex/#syntax))
* `format` - Controls which parts of the regex match will be used to form the link.
  Must have a `prefix:` signaling the protocol type (e.g., `https:`/`mailto:`),
  which can either come from the regex match or needs to be explicitly added.
  The format string can use placeholders like `$0`, `$1`, `$2` etc. that will be replaced
  with that numbered capture group.  So, `$0` will take the entire
  region of text matched by the whole regex, while `$1` matches out
  the first capture group.  In the example below, `mailto:$0` is
  used to prefix a protocol to the text to make it into an URL.

{{since('20230320-124340-559cb7b0', outline=True)}}
    * `highlight` - specifies the range of the matched text that should be
      highlighted/underlined when the mouse hovers over the link.  The value is
      a number that corresponds to a capture group in the regex.  The default
      is `0`, highlighting the entire region of text matched by the regex.  `1`
      would be the first capture group, and so on.

{{since('20230408-112425-69ae8472', outline=True)}}
    The regex syntax now supports backreferences and look around assertions.
    See [Fancy Regex Syntax](https://docs.rs/fancy-regex/latest/fancy_regex/#syntax)
    for the extended syntax, which builds atop the underlying
    [Regex syntax](https://docs.rs/regex/latest/regex/#syntax).
    In prior versions, only the base
    [Regex syntax](https://docs.rs/regex/latest/regex/#syntax) was supported.

Assigning `hyperlink_rules` overrides the built-in default rules.

The default value for `hyperlink_rules` can be retrieved using
[onlyterm.default_hyperlink_rules()](../onlyterm/default_hyperlink_rules.md),
and is shown below:

```
hyperlink_rules: [
  ## Matches: a URL in parens: (URL)
  {
    regex: \\((\\w+://\\S+)\\)
    format: $1
    highlight: 1
  }
  ## Matches: a URL in brackets: [URL]
  {
    regex: \\[(\\w+://\\S+)\\]
    format: $1
    highlight: 1
  }
  ## Matches: a URL in curly braces: [URL]
  {
    regex: \\{(\\w+://\\S+)\\}
    format: $1
    highlight: 1
  }
  ## Matches: a URL in angle brackets: <URL>
  {
    regex: <(\\w+://\\S+)>
    format: $1
    highlight: 1
  }
  ## Then handle URLs not wrapped in brackets
  {
    regex: \\b\\w+://\\S+[)/a-zA-Z0-9-]+
    format: $0
  }
  ## implicit mailto link
  {
    regex: \\b\\w+@[\\w-]+(\\.[\\w-]+)+\\b
    format: mailto:$0
  }
]
```

!!! note
    ktav has no string quoting syntax, but it does recognize a small, fixed
    set of backslash escapes in any value: `\\`, `\,`, `\}`, `\]`, `\{`,
    `\[`, `\n`, `\r`, `\.` and `\:` (see the
    [migration guide](../../../migration-to-ktav.md)). A literal backslash
    must always be written as `\\`, so a regex like `\b[tT](\d+)\b` becomes
    `\\b[tT](\\d+)\\b` when written as a ktav value, with no surrounding
    quotes. Also note that a `regex` value containing raw `[`, `]`, `{` or
    `}` characters (as several of the rules above do) must have each field
    on its own line inside the rule's `{ ... }` object — packing multiple
    fields onto one line alongside a regex containing those characters can
    confuse ktav's line-based bracket matching.

!!! danger "No longer possible: extending the defaults from config"

    Earlier versions of this page showed calling the scripting function
    `onlyterm.default_hyperlink_rules()` to get the built-in rule list and
    then `.push(...)`-ing extra rules onto it (for example, to make task
    numbers or bare `owner/repo` paths clickable in addition to the
    defaults). `default_hyperlink_rules()` and `.push(...)` both required
    the scripting engine, which has been removed — see the
    [changelog](../../../changelog.md#continuousnightly). ktav has no
    function calls and no way to reference "the built-in defaults" from
    within the config file, so `hyperlink_rules` can now only be set to a
    complete, literal list: if you want the defaults *plus* your own rules,
    you must copy the default rules shown above into your config file
    alongside the ones you want to add.
