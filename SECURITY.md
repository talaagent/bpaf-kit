# Security policy

Security fixes target the latest published versions. Report vulnerabilities
privately through [GitHub private vulnerability reporting](https://github.com/talaagent/bpaf-kit/security/advisories/new).
If that form is unavailable, email **github@igm.pub**. Please do not disclose
an unpatched vulnerability in a public issue.

Include the affected crate and version, enabled features, OS, impact, and a
minimal reproduction. Relevant issues include argument quoting in the echoed
command line that misrepresents what runs, environment or working-directory
leaks into child processes, hangs or unbounded memory from a child's
output, a version file or environment variable that makes a tool misbehave
beyond what its contents say, and panics on any of these. Remove secrets
from command lines, environment dumps and logs before reporting.

Problems in bpaf itself should be reported upstream at
[pacak/bpaf](https://github.com/pacak/bpaf).
