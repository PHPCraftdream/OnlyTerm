# Privacy Policy for OnlyTerm

No data about your device(s) or OnlyTerm usage leave your device.

## Data Maintained by OnlyTerm

OnlyTerm maintains some historical data, such as recent searches or action
usage, in some of its overlays such as the debug overlay and character
selector, in order to make your usage more convenient. It is used only
by the local process, and care is taken to limit access for the associated
files on disk to only your local user identity.

OnlyTerm tracks the output from the commands that you have executed in
a scrollback buffer.  At the time of writing, that scrollback buffer
is an in-memory structure that is not visible to other users of the machine.
In the future, if OnlyTerm expands to offload scrollback information to
your local disk, it will do so in such a way that other users on the
same system will not be able to inspect it.

## Update Checking

By default, once every 24 hours, OnlyTerm makes an HTTP request to GitHub's
release API in order to determine if a newer version is available and to
notify you if that is the case.

The content of that request is private between your machine and GitHub.  The
contributors to OnlyTerm cannot see inside that request and therefore cannot
infer any information from it.

If you wish, you can disable update checking. See
[check_for_updates](docs/config/reference/config/check_for_updates.md) for
more information on that.

## Third-Party Builds

The above is true of the OnlyTerm source code and the binaries produced by
OnlyTerm's CI.

This project is a fork of [wezterm/wezterm](https://github.com/wezterm/wezterm);
the upstream project's own binaries are made available from https://wezterm.org/
and https://github.com/wezterm/wezterm/, and are covered by upstream's own
privacy policy.

If you obtained a pre-built OnlyTerm binary from some other source be aware that
the person(s) building those versions may have modified them to behave
differently from the source version.
