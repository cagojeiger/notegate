# Scanner rule sources

The Semgrep CE 1.182.0 scanner uses a separate clean checkout of
`https://github.com/semgrep/semgrep-rules` at commit
`16b9e6d45f10649f6a531ec4838ed9924372c3bc`. It uses only the
`python/lang/security/audit`, `javascript/lang/security/audit`, and
`typescript/lang/security/audit` directories. These rules are subject to the
[Semgrep Rules License v1.0](https://semgrep.dev/legal/rules-license/), which
restricts use and redistribution; the rules are not vendored here. The Semgrep
engine has a separate LGPL 2.1 license. This limited rule selection is not
equivalent to CodeQL. The selected bundle does not analyze GitHub Actions or Rust.
Semgrep's default exclusions still omit some paths such as tests, vendor and dist;
this is not a full tracked-file scan. Synthetic clean/finding cases validate engine
compatibility in CI.

The Gitleaks 8.30.1 snapshot scan uses the default rules built into that pinned
binary via `gitleaks.toml`. It does not scan Git history.

`known-fixtures.json` exempts only reviewed synthetic secrets with matching paths,
rules, line ranges and raw block hashes. `reviewed-audits.json` bounds approved
audit warnings by path, rule, full-file hash and maximum count. Changed content,
additional findings and scanner or parsing errors still fail the check; low
confidence alone does not exempt a finding. Raw reports are not published.
