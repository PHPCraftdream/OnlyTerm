---
tags:
  - exit_behavior
---
## `exit_behavior_messaging = "Verbose"`

{{since('20230712-072601-f4abf8fd')}}

Controls how onlyterm indicates the exit status of the spawned process
in a pane when it terminates.

If [exit_behavior](exit_behavior.md) is set to keep the pane open after
the process has completed, onlyterm will display a message to let you
know that it has finished.

This option controls that message.  It can have one of the following
values:

* `"Verbose"` - Shows 2-3 lines of explanation, including the process name, its exit status and a link to the [exit_behavior](exit_behavior.md) documentation.
* `"Brief"` - Like `"Verbose"`, but the link to the documentation is not included.
* `"Terse"` - A very short indication of the exit status is shown in square brackets.
* `"None"` - No message is shown.

In earlier versions of onlyterm, this was not configurable and behaved equivalently
to the `"Verbose"` setting.

## Example of a failing process with Verbose messaging

`default_prog=[:: false]` overrides `default_prog` to spawn the literal
string `false` (the Unix `/bin/false` command, which always exits with a
non-zero status) as a single-element argv array. The `::` before `false` is
required here: without it, the bareword `false` would parse as the ktav
boolean `false` rather than the program-name string `"false"`.

```console
$ onlyterm -n --config 'default_prog=[:: false]' \
    --config exit_behavior=Hold \
    --config exit_behavior_messaging=Verbose
```

Produces:

```
⚠️  Process "false" in domain "local" didn't exit cleanly
Exited with code 1
This message is shown because exit_behavior="Hold"
```

## Example of a failing process with Brief messaging

```console
$ onlyterm -n --config 'default_prog=[:: false]' \
     --config exit_behavior=Hold \
     --config exit_behavior_messaging=Brief
```

Produces:

```
⚠️  Process "false" in domain "local" didn't exit cleanly
Exited with code 1
```

## Example of a failing process with Terse messaging

```console
$ onlyterm -n --config 'default_prog=[:: false]' \
     --config exit_behavior=Hold \
     --config exit_behavior_messaging=Terse
```

Produces:

```
[Exited with code 1]
```

## Example of a successful process with Verbose messaging

```console
$ onlyterm -n --config 'default_prog=[:: true]' \
     --config exit_behavior=Hold \
     --config exit_behavior_messaging=Verbose
```

Produces:

```
👍 Process "true" in domain "local" completed.
This message is shown because exit_behavior="Hold"
```

## Example of a successful process with Brief messaging

```console
$ onlyterm -n --config 'default_prog=[:: true]' \
     --config exit_behavior=Hold \
     --config exit_behavior_messaging=Brief
```

Produces:

```
👍 Process "true" in domain "local" completed.
```

## Example of a successful process with Terse messaging

```console
$ onlyterm -n --config 'default_prog=[:: true]' \
     --config exit_behavior=Hold \
     --config exit_behavior_messaging=Terse
```

Produces:

```
[done]
```
