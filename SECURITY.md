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

## Release integrity

FlexiMark publishes one VSIX containing native daemons for Windows, Linux, and
macOS on x64 and arm64. The GitHub artifact attestation for that complete VSIX is
the release trust root. The SHA-256 values inside `bin/manifest.json` are used
after installation to detect daemon corruption and platform artifact mix-ups;
they are not independent signatures. The extension verifies the selected
bundled daemon against that manifest immediately before every launch.

The `fleximark.daemonPath` setting explicitly selects a user-supplied executable,
so binaries selected through that setting are outside the bundled release trust
model.
