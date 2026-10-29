# Scanner rule sources

The Semgrep CE 1.182.0 prototype expects a separate clean checkout of
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
compatibility in CI; actual core findings need separate qualification.

The Gitleaks 8.30.1 snapshot scan uses the default rules built into that pinned
binary via `gitleaks.toml`. It does not scan Git history.
