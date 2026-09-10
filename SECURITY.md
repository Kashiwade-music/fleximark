# Security Policy

## Supported versions

FlexiMark is a Preview extension. Security fixes are provided only for the
latest release.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's **Report a
vulnerability** form in the repository's Security tab. Do not open a public
issue for an unpatched vulnerability.

Include the affected version, reproduction steps, impact, and any suggested
mitigation. You should receive an acknowledgement within seven days. Please
allow time for a fix and coordinated disclosure before publishing details.

## Workspace code execution

FlexiMark can execute `.fleximark/parserPlugin.js` from trusted local
workspaces. Open only workspaces whose contents you trust. The extension
declares that untrusted and virtual workspaces are unsupported.
