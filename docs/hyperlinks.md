OnlyTerm has support for both implicit and explicit hyperlinks.

### Implicit Hyperlinks

Implicit hyperlinks are produced by running a series of rules over the output
displayed in the terminal to produce a hyperlink.  There is a default rule
to match URLs and make them clickable, but you can also specify your own rules
to make your own links.

As an example, at my place of work many of our internal tools use `T123` to
indicate task number 123 in our internal task tracking system.  It is desirable
to make this clickable, and that can be done with the following configuration
in your `~/.onlyterm.ktav`:

```
// ktav has no function calls, so there is no way to reference "the built-in
// defaults" from config; if you want the defaults plus your own rules, copy
// the default rule list (see hyperlink_rules) into your config alongside
// the extra rules you want, as done below.
hyperlink_rules: [
  // Matches: a URL in parens: (URL)
  { regex: "\\((\\w+://\\S+)\\)", format: "$1", highlight: 1 }
  // Matches: a URL in brackets: [URL]
  { regex: "\\[(\\w+://\\S+)\\]", format: "$1", highlight: 1 }
  // Matches: a URL in curly braces: [URL]
  { regex: "\\{(\\w+://\\S+)\\}", format: "$1", highlight: 1 }
  // Matches: a URL in angle brackets: <URL>
  { regex: "<(\\w+://\\S+)>", format: "$1", highlight: 1 }
  // Then handle URLs not wrapped in brackets
  { regex: "\\b\\w+://\\S+[)/a-zA-Z0-9-]+", format: "$0" }
  // implicit mailto link
  { regex: "\\b\\w+@[\\w-]+(\\.[\\w-]+)+\\b", format: "mailto:$0" }

  // make task numbers clickable
  // the first matched regex group is captured in $1.
  { regex: "\\b[tt](\\d+)\\b", format: "https://example.com/tasks/?t=$1" }

  // make username/project paths clickable. this implies paths like the following are for github.
  // ( "nvim-treesitter/nvim-treesitter" | wbthomason/packer.nvim | wezterm/wezterm | "wezterm/wezterm.git" )
  // as long as a full url hyperlink regex exists above this it should not match a full url to
  // github or gitlab / bitbucket (i.e. https://gitlab.com/user/project.git is still a whole clickable url)
  { regex: "[\"]?([\\w\\d]{1}[-\\w\\d]+)(/){1}([-\\w\\d\\.]+)[\"]?", format: "https://www.github.com/$1/$3" }
]
```

See also [hyperlink_rules](config/reference/config/hyperlink_rules.md) and
[default_hyperlink_rules](config/reference/wezterm/default_hyperlink_rules.md)
(a removed scripting function; see that page).


### Explicit Hyperlinks

OnlyTerm supports the relatively new [Hyperlinks in Terminal
Emulators](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)
specification that allows emitting text that can be clicked and resolve to a
specific URL, without the URL being part of the display text.  This allows
for a cleaner presentation.

The gist of it is that running the following bash one-liner:

```bash
printf '\e]8;;http://example.com\e\\This is a link\e]8;;\e\\\n'
```

will output the text `This is a link` that when clicked will open
`http://example.com` in your browser.
