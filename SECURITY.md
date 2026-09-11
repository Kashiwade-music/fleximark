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

## Workspace trust boundary

FlexiMark never executes JavaScript files from a workspace. Optional extensions
are WebAssembly components admitted through the manifest, capability, signature,
resource-limit, and trust checks documented in the architecture. Workspace
writes and local asset reads remain disabled unless the workspace is trusted.
The extension does not support virtual workspaces.
