# Security policy

kintsu reads the command you just ran, its exit status and, when it can,
what it printed, and sends that to a model provider *you* configured, only
when *you* ask. A flaw in what it captures, how it redacts, where it sends
it, or in a suggested command reaching execution without your confirmation
is worth a quiet report, not a public issue.

## Supported versions

Only the latest release receives fixes.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting:
**Security → Report a vulnerability** on
[github.com/nathan-poncet/kintsu](https://github.com/nathan-poncet/kintsu/security/advisories/new).
Nothing you write there is visible to anyone but the maintainer until a fix
is out.

Please include what you observed, how to reproduce it, and the kintsu,
shell, terminal and OS versions. You will get an acknowledgement within
seven days, and the fix ships in the next release with credit to you unless
you prefer otherwise.

## What counts

- Captured terminal content reaching anything other than the provider or
  agent you configured.
- A secret that the default redaction should have caught leaving the
  machine.
- A suggested or agent-generated command executing without the user
  pressing Enter on it.
- Crafted command output (prompt injection) steering kintsu or the hand-off
  prompt into a dangerous action.
- The shell hook hanging, crashing or altering the exit status your prompt
  sees.

## What does not

- The provider you configured receiving the content you chose to send:
  that is the point.
- A CLI agent you handed off to doing whatever that agent does; report it
  to that agent's project.
- Custom `command = "..."` templates doing whatever the command line you
  wrote does.
