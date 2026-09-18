# Security Policy

dev-prompt is a young, single-maintainer project — same spirit as
[CONTRIBUTING.md](CONTRIBUTING.md): quick and informal over process-heavy.
Security reports get one exception to that: please don't open a public issue.

## Supported versions

Only the latest release gets fixes — there's no backport policy given the
CalVer / single-maintainer setup. If you're on an older version, update first
and confirm the issue still reproduces.

## Reporting a vulnerability

- **Preferred:** [GitHub private vulnerability reporting](https://github.com/seraphx2/dev-prompt/security/advisories/new)
  — visible only to me until there's a fix.
- **Fallback:** email seraphx2@live.com with a description and repro steps.

No formal SLA (solo maintainer), but expect an initial response within a few
days. I'll credit reporters in the published advisory unless you'd rather stay
anonymous.

## Scope

This covers dev-prompt's own code — `src/`, `src-tauri/`, and the release/
packaging workflows. A vulnerability in an upstream dependency should usually
go to that project directly; if it affects dev-prompt specifically (how a
dependency is used here, not the dependency in the abstract), a report here is
still welcome.
