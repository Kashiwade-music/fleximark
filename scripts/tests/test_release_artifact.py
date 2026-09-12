from __future__ import annotations

import hashlib
import json
import os
import stat
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from typing import Any, Callable
from unittest.mock import patch


SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import _targets
import release_artifact


class ReleaseArtifactTests(unittest.TestCase):
    def write_prebuilt_inputs(self, root: Path) -> list[Path]:
        runtime = [
            root / "dist" / "extension.cjs",
            root / "dist" / "web" / "preview-client" / "vscode-host.js",
        ]
        for path in runtime:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"r" * 1025)
        artifacts = []
        daemons = []
        for target in _targets.TARGETS:
            path = root / target.bin_relative_path
            path.parent.mkdir(parents=True, exist_ok=True)
            payload = f"{target.platform}-{target.arch}".encode()
            path.write_bytes(payload)
            if target.platform != "win32":
                path.chmod(0o755)
            daemons.append(path)
            artifacts.append(
                {
                    "platform": target.platform,
                    "arch": target.arch,
                    "path": target.bin_relative_path.as_posix(),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                }
            )
        (root / "bin" / "manifest.json").write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "protocolVersion": 1,
                    "artifacts": artifacts,
                }
            ),
            encoding="utf-8",
        )
        return daemons

    def write_vsix(
        self,
        path: Path,
        *,
        version: str = "1.2.3",
        manifest_version: str | None = None,
        manifest_id: str = "fleximark",
        manifest_publisher: str = "Kashiwade",
        daemon_hash_override: str | None = None,
        artifact_limit: int | None = None,
        extra_name: str | None = None,
        unix_daemon_mode: int = 0o755,
        extension_runtime: bytes = b"e" * 1025,
        empty_daemon: bool = False,
        omit_name: str | None = None,
        package_mutator: Callable[[dict[str, Any]], None] | None = None,
    ) -> None:
        artifacts: list[dict[str, str]] = []
        daemon_payloads: dict[str, bytes] = {}
        for index, target in enumerate(_targets.TARGETS):
            relative = target.bin_relative_path.as_posix()
            payload = (
                b""
                if empty_daemon and index == 0
                else f"{target.platform}-{target.arch}".encode()
            )
            daemon_payloads[relative] = payload
            artifacts.append(
                {
                    "platform": target.platform,
                    "arch": target.arch,
                    "path": relative,
                    "sha256": hashlib.sha256(payload).hexdigest(),
                }
            )
        if daemon_hash_override is not None:
            artifacts[0]["sha256"] = daemon_hash_override
        if artifact_limit is not None:
            artifacts = artifacts[:artifact_limit]

        with zipfile.ZipFile(path, "w") as archive:

            def write(name: str, payload: str | bytes, *, mode: int = 0o644) -> None:
                if name == omit_name:
                    return
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | mode) << 16
                archive.writestr(info, payload)

            write("[Content_Types].xml", "<Types />")
            write(
                "extension.vsixmanifest",
                (
                    '<PackageManifest xmlns="http://schemas.microsoft.com/'
                    'developer/vsx-schema/2011"><Metadata><Identity '
                    f'Id="{manifest_id}" Publisher="{manifest_publisher}" '
                    f'Version="{manifest_version or version}" /></Metadata>'
                    "</PackageManifest>"
                ),
            )
            package = json.loads(
                (SCRIPTS.parent / "package.json").read_text(encoding="utf-8")
            )
            package["version"] = version
            if package_mutator is not None:
                package_mutator(package)
            write("extension/package.json", json.dumps(package))
            write(
                "extension/bin/manifest.json",
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "protocolVersion": 1,
                        "artifacts": artifacts,
                    }
                ),
            )
            write("extension/dist/extension.cjs", extension_runtime)
            write("extension/dist/web/preview-client/vscode-host.js", b"p" * 1025)
            write("extension/dist/web/preview-client/browser-host.js", b"b" * 1025)
            declared_paths = {
                package["icon"],
                *(
                    contribution["path"]
                    for key in ("grammars", "snippets")
                    for contribution in package["contributes"][key]
                ),
                *(
                    language["configuration"]
                    for language in package["contributes"]["languages"]
                    if "configuration" in language
                ),
            }
            for declared in sorted(declared_paths):
                relative = declared.removeprefix("./")
                if relative in {
                    "dist/extension.cjs",
                    "dist/web/preview-client/browser-host.js",
                    "dist/web/preview-client/vscode-host.js",
                }:
                    continue
                write(f"extension/{relative}", b"{}")
            for relative, payload in daemon_payloads.items():
                platform_name = relative.split("/", 2)[1].split("-", 1)[0]
                write(
                    f"extension/{relative}",
                    payload,
                    mode=0o644 if platform_name == "win32" else unix_daemon_mode,
                )
            if extra_name is not None:
                write(extra_name, b"forbidden")

    def test_create_and_verify_identity_for_complete_release_vsix(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            identity_path = root / "fleximark.vsix.identity.json"
            self.write_vsix(vsix)

            created = release_artifact.create_identity(
                vsix,
                identity_path,
                expected_version="1.2.3",
                git_tag="v1.2.3",
                source_git_head="b" * 40,
            )
            self.assertEqual(
                created,
                {
                    "schemaVersion": 1,
                    "file": "fleximark.vsix",
                    "version": "1.2.3",
                    "sha256": hashlib.sha256(vsix.read_bytes()).hexdigest(),
                    "gitTag": "v1.2.3",
                    "sourceGitHead": "b" * 40,
                },
            )
            self.assertTrue(identity_path.read_bytes().endswith(b"\n"))
            self.assertEqual(
                release_artifact.verify_identity(
                    vsix,
                    identity_path,
                    expected_sha256=created["sha256"],
                    expected_git_tag="v1.2.3",
                    expected_source_git_head="b" * 40,
                ),
                created,
            )

    def test_prebuilt_gate_checks_runtime_manifest_and_daemon_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            daemons = self.write_prebuilt_inputs(root)

            release_artifact.validate_prebuilt_inputs(root)

            daemons[0].write_bytes(b"tampered")
            with self.assertRaisesRegex(RuntimeError, "hash mismatch"):
                release_artifact.validate_prebuilt_inputs(root)

    def test_prebuilt_gate_requires_the_vscode_preview_host_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_prebuilt_inputs(root)
            (root / "dist" / "web" / "preview-client" / "vscode-host.js").unlink()

            with self.assertRaisesRegex(RuntimeError, "prebuilt input is missing"):
                release_artifact.validate_prebuilt_inputs(root)

    def test_identity_creation_detects_toctou_and_replaces_atomically(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            identity_path = root / "fleximark.vsix.identity.json"
            self.write_vsix(vsix)

            with patch.object(
                release_artifact,
                "sha256_file",
                side_effect=["a" * 64, "b" * 64],
            ):
                with self.assertRaisesRegex(RuntimeError, "changed"):
                    release_artifact.create_identity(
                        vsix,
                        identity_path,
                        expected_version="1.2.3",
                        git_tag="v1.2.3",
                        source_git_head="b" * 40,
                    )
            self.assertFalse(identity_path.exists())

            with patch.object(
                release_artifact.os,
                "replace",
                side_effect=OSError("replace failed"),
            ):
                with self.assertRaisesRegex(OSError, "replace failed"):
                    release_artifact.create_identity(
                        vsix,
                        identity_path,
                        expected_version="1.2.3",
                        git_tag="v1.2.3",
                        source_git_head="b" * 40,
                    )
            self.assertFalse(identity_path.exists())
            self.assertEqual(list(root.glob(".*.tmp")), [])

    def test_rejects_internal_version_disagreement(self) -> None:
        for options in (
            {"manifest_version": "1.2.4"},
            {"manifest_id": "other"},
            {"manifest_publisher": "Other"},
        ):
            with (
                self.subTest(options=options),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, **options)

                with self.assertRaisesRegex(
                    RuntimeError, "manifest version or identity"
                ):
                    release_artifact.validate_vsix(vsix, expected_version="1.2.3")

    def test_requires_strict_semver_and_exact_v_prefixed_tag(self) -> None:
        invalid_versions = (
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-01",
            "1.2",
            "v1.2.3",
            "1.2.3;echo-injected",
        )
        for version in invalid_versions:
            with (
                self.subTest(version=version),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, version=version)
                with self.assertRaisesRegex(RuntimeError, "strict SemVer"):
                    release_artifact.validate_vsix(vsix)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            self.write_vsix(vsix, version="1.2.3-alpha.1+build.5")
            release_artifact.create_identity(
                vsix,
                root / "fleximark.vsix.identity.json",
                expected_version="1.2.3-alpha.1+build.5",
                git_tag="v1.2.3-alpha.1+build.5",
                source_git_head="b" * 40,
            )

    def test_cli_reads_fixed_verify_expectations_from_environment(self) -> None:
        expected = {
            "FLEXIMARK_EXPECTED_SHA256": "a" * 64,
            "FLEXIMARK_EXPECTED_GIT_TAG": "v1.2.3",
            "FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD": "b" * 40,
        }
        with (
            patch.dict(os.environ, expected, clear=False),
            patch.object(sys, "argv", ["release_artifact.py", "verify"]),
            patch.object(
                release_artifact,
                "verify_identity",
                return_value={"sha256": "a" * 64},
            ) as verify,
        ):
            release_artifact.main()
        self.assertEqual(verify.call_args.kwargs["expected_sha256"], "a" * 64)
        self.assertEqual(verify.call_args.kwargs["expected_git_tag"], "v1.2.3")
        self.assertEqual(verify.call_args.kwargs["expected_source_git_head"], "b" * 40)

    def test_rejects_incomplete_malformed_or_mismatched_daemon_manifest(
        self,
    ) -> None:
        cases = (
            ({"artifact_limit": 5}, "all six targets"),
            ({"daemon_hash_override": "not-a-hash"}, "invalid SHA-256"),
            ({"daemon_hash_override": "0" * 64}, "daemon checksum"),
        )
        for options, expected in cases:
            with (
                self.subTest(expected=expected),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, **options)

                with self.assertRaisesRegex(RuntimeError, expected):
                    release_artifact.validate_vsix(vsix)

    def test_rejects_nonexecutable_unix_daemon_entries(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            vsix = Path(temporary) / "fleximark.vsix"
            self.write_vsix(vsix, unix_daemon_mode=0o644)

            with self.assertRaisesRegex(RuntimeError, "Unix daemon is not executable"):
                release_artifact.validate_vsix(vsix)

    def test_rejects_forbidden_development_content(self) -> None:
        for forbidden in (
            "extension/src/private.mjs",
            "extension/test/fixture.json",
            "extension/node_modules/dependency/index.js",
            "extension/parserPlugin.js",
            "extension/adapters/vscode/src/extension.mjs",
            "extension/web/preview-client/index.mjs",
            "extension/crates/fleximarkd/src/main.rs",
            "extension/capabilities/legacy.json",
            "extension/schemas/config.schema.json",
            "extension/.ruff_cache/cache-entry",
            "extension/.future_cache/cache-entry",
            "extension/unknown-cache-v2/cache-entry",
            "extension/nested/tsconfig.unit.json",
            "extension/out/types/unit.tsbuildinfo",
            "extension/.git/config",
            "extension/.env",
            "extension/.vscode/settings.json",
            "extension/coverage/lcov.info",
            "extension/assets/private.pem",
            "extension/unexpected-runtime.js",
        ):
            with (
                self.subTest(forbidden=forbidden),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, extra_name=forbidden)

                with self.assertRaisesRegex(RuntimeError, "forbidden path"):
                    release_artifact.validate_vsix(vsix)

    def test_requires_both_preview_hosts_and_every_declared_package_path(self) -> None:
        for missing, expected in (
            (
                "extension/dist/web/preview-client/browser-host.js",
                "missing required extension metadata or runtime files",
            ),
            (
                "extension/dist/extension.cjs",
                "missing required extension metadata or runtime files",
            ),
            ("extension/assets/logo_icon.png", "missing declared package path"),
            (
                "extension/syntaxes/markdown.tmLanguage.json",
                "missing declared package path",
            ),
            ("extension/snippets/tabs.json", "missing declared package path"),
            (
                "extension/language-support/abc/language-configuration.json",
                "missing declared package path",
            ),
        ):
            with (
                self.subTest(missing=missing),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, omit_name=missing)
                with self.assertRaisesRegex(RuntimeError, expected):
                    release_artifact.validate_vsix(vsix)

    def test_allows_packaged_runtime_and_contribution_classes(self) -> None:
        for allowed in (
            "extension/README.md",
            "extension/LICENSE.txt",
            "extension/package.nls.ja.json",
            "extension/l10n/bundle.l10n.ja.json",
            "extension/assets/additional.webp",
        ):
            with (
                self.subTest(allowed=allowed),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, extra_name=allowed)
                self.assertEqual(release_artifact.validate_vsix(vsix), "1.2.3")

    def test_rejects_redirected_main_and_contribution_paths(self) -> None:
        def redirect_main(package: dict[str, Any]) -> None:
            package["main"] = "./dist/web/preview-client/browser-host.js"

        def redirect_grammar(package: dict[str, Any]) -> None:
            package["contributes"]["grammars"][0]["path"] = package["contributes"][
                "snippets"
            ][0]["path"]

        def swap_grammar_order(package: dict[str, Any]) -> None:
            grammars = package["contributes"]["grammars"]
            grammars[0], grammars[1] = grammars[1], grammars[0]

        def change_snippet_metadata(package: dict[str, Any]) -> None:
            package["contributes"]["snippets"][0]["language"] = "abc"

        def change_language_metadata(package: dict[str, Any]) -> None:
            package["contributes"]["languages"][0]["aliases"] = ["ABC"]

        def change_command_metadata(package: dict[str, Any]) -> None:
            package["contributes"]["commands"][0]["command"] = "evil.command"

        def change_menu_metadata(package: dict[str, Any]) -> None:
            package["contributes"]["menus"]["editor/title"][0]["command"] = (
                "evil.command"
            )

        def change_configuration_metadata(package: dict[str, Any]) -> None:
            package["contributes"]["configuration"]["properties"][
                "fleximark.previewTarget"
            ]["default"] = "evilTarget"

        def change_boolean_to_integer(package: dict[str, Any]) -> None:
            package["contributes"]["configuration"]["properties"][
                "fleximark.autoOpenPreview"
            ]["default"] = 0

        for label, mutate in (
            ("main", redirect_main),
            ("grammar", redirect_grammar),
            ("grammar order", swap_grammar_order),
            ("snippet metadata", change_snippet_metadata),
            ("language metadata", change_language_metadata),
            ("command metadata", change_command_metadata),
            ("menu metadata", change_menu_metadata),
            ("configuration metadata", change_configuration_metadata),
            ("boolean as integer", change_boolean_to_integer),
        ):
            with (
                self.subTest(label=label),
                tempfile.TemporaryDirectory() as temporary,
            ):
                vsix = Path(temporary) / "fleximark.vsix"
                self.write_vsix(vsix, package_mutator=mutate)
                with self.assertRaisesRegex(
                    RuntimeError, "declarations do not match trusted package.json"
                ):
                    release_artifact.validate_vsix(vsix)

    def test_rejects_integer_to_float_in_contribution_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            trusted_package = json.loads(
                (SCRIPTS.parent / "package.json").read_text(encoding="utf-8")
            )
            trusted_package["contributes"]["configuration"][
                "x-fleximark-contract-version"
            ] = 1
            (root / "package.json").write_text(
                json.dumps(trusted_package), encoding="utf-8"
            )

            def change_integer_to_float(package: dict[str, Any]) -> None:
                package["contributes"]["configuration"][
                    "x-fleximark-contract-version"
                ] = 1.0

            vsix = root / "fleximark.vsix"
            self.write_vsix(vsix, package_mutator=change_integer_to_float)
            with (
                patch.object(release_artifact, "ROOT", root),
                self.assertRaisesRegex(
                    RuntimeError, "declarations do not match trusted package.json"
                ),
            ):
                release_artifact.validate_vsix(vsix)

    def test_validate_cli_uses_the_full_archive_contract(self) -> None:
        vsix = Path("contract.vsix")
        with (
            patch.object(
                sys,
                "argv",
                [
                    "release_artifact.py",
                    "validate",
                    "--vsix",
                    str(vsix),
                    "--expected-version",
                    "1.2.3",
                ],
            ),
            patch.object(release_artifact, "validate_vsix") as validate_vsix,
        ):
            release_artifact.main()

        validate_vsix.assert_called_once_with(vsix, expected_version="1.2.3")

    def test_validate_cli_rejects_identity_and_provenance_options(self) -> None:
        invalid_options = (
            ("--identity", "identity.json"),
            ("--expected-sha256", "a" * 64),
            ("--git-tag", "v1.2.3"),
            ("--source-git-head", "b" * 40),
            ("--expected-git-tag", "v1.2.3"),
            ("--expected-source-git-head", "b" * 40),
            ("--print-sha256",),
        )
        for invalid in invalid_options:
            with (
                self.subTest(option=invalid[0]),
                patch.object(
                    sys,
                    "argv",
                    ["release_artifact.py", "validate", *invalid],
                ),
                patch.object(sys, "stderr"),
                patch.object(release_artifact, "validate_vsix") as validate_vsix,
            ):
                with self.assertRaises(SystemExit) as raised:
                    release_artifact.main()
                self.assertEqual(raised.exception.code, 2)
                validate_vsix.assert_not_called()

    def test_create_cli_preserves_release_identity_arguments(self) -> None:
        vsix = Path("release.vsix")
        identity = Path("release.identity.json")
        digest = "a" * 64
        with (
            patch.object(
                sys,
                "argv",
                [
                    "release_artifact.py",
                    "create",
                    "--vsix",
                    str(vsix),
                    "--identity",
                    str(identity),
                    "--expected-version",
                    "1.2.3",
                    "--git-tag",
                    "v1.2.3",
                    "--source-git-head",
                    "b" * 40,
                    "--print-sha256",
                ],
            ),
            patch.object(
                release_artifact,
                "create_identity",
                return_value={"sha256": digest},
            ) as create_identity,
            patch("builtins.print") as print_digest,
        ):
            release_artifact.main()

        create_identity.assert_called_once_with(
            vsix,
            identity,
            expected_version="1.2.3",
            git_tag="v1.2.3",
            source_git_head="b" * 40,
        )
        print_digest.assert_called_once_with(digest)

    def test_rejects_missing_malformed_and_mismatched_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            identity_path = root / "fleximark.vsix.identity.json"
            self.write_vsix(vsix)

            with self.assertRaisesRegex(RuntimeError, "does not exist"):
                release_artifact.verify_identity(vsix, identity_path)

            identity_path.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "file": "fleximark.vsix",
                        "version": "1.2.3",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "unexpected fields"):
                release_artifact.verify_identity(vsix, identity_path)

            identity_path.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "file": "fleximark.vsix",
                        "version": "1.2.3",
                        "sha256": "not-a-hash",
                        "gitTag": "v1.2.3",
                        "sourceGitHead": "b" * 40,
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "SHA-256 is invalid"):
                release_artifact.verify_identity(vsix, identity_path)

            identity = release_artifact.create_identity(
                vsix,
                identity_path,
                expected_version="1.2.3",
                git_tag="v1.2.3",
                source_git_head="b" * 40,
            )
            with self.assertRaisesRegex(RuntimeError, "expected SHA-256"):
                release_artifact.verify_identity(
                    vsix, identity_path, expected_sha256="f" * 64
                )
            with self.assertRaisesRegex(RuntimeError, "expected git tag"):
                release_artifact.verify_identity(
                    vsix, identity_path, expected_git_tag="v9.9.9"
                )
            with self.assertRaisesRegex(RuntimeError, "expected source git head"):
                release_artifact.verify_identity(
                    vsix, identity_path, expected_source_git_head="c" * 40
                )
            for untrusted_tag in ("v1.2.4", "v1.2.3;echo-injected"):
                with self.subTest(untrusted_tag=untrusted_tag):
                    mismatched_tag = {**identity, "gitTag": untrusted_tag}
                    identity_path.write_text(
                        json.dumps(mismatched_tag), encoding="utf-8"
                    )
                    with self.assertRaisesRegex(
                        RuntimeError, "does not match its version"
                    ):
                        release_artifact.verify_identity(vsix, identity_path)
            identity_path.write_text(json.dumps(identity), encoding="utf-8")
            vsix.write_bytes(vsix.read_bytes() + b"changed")
            with self.assertRaisesRegex(RuntimeError, "does not match"):
                release_artifact.verify_identity(
                    vsix, identity_path, expected_sha256=identity["sha256"]
                )

    def test_identity_sidecar_must_be_regular_nonempty_and_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            identity = root / "fleximark.vsix.identity.json"
            self.write_vsix(vsix)

            identity.write_bytes(b"")
            with self.assertRaisesRegex(RuntimeError, "size or type"):
                release_artifact.read_identity(identity)
            identity.unlink()
            identity.mkdir()
            with self.assertRaisesRegex(RuntimeError, "size or type"):
                release_artifact.read_identity(identity)
            identity.rmdir()
            identity.write_bytes(b"{}")
            with patch.object(release_artifact, "MAX_METADATA_SIZE", 1):
                with self.assertRaisesRegex(RuntimeError, "size or type"):
                    release_artifact.read_identity(identity)

    def archive_info(
        self,
        name: str,
        *,
        mode: int = stat.S_IFREG | 0o644,
        file_size: int = 1,
        compress_size: int = 1,
        encrypted: bool = False,
    ) -> zipfile.ZipInfo:
        info = zipfile.ZipInfo(name)
        info.create_system = 3
        info.external_attr = mode << 16
        info.file_size = file_size
        info.compress_size = compress_size
        if encrypted:
            info.flag_bits |= 1
        return info

    def test_rejects_unsafe_noncanonical_and_colliding_archive_paths(self) -> None:
        cases = (
            ([self.archive_info("")], "noncanonical"),
            ([self.archive_info("../escape")], "noncanonical"),
            ([self.archive_info("/absolute")], "noncanonical"),
            ([self.archive_info("C:/escape")], "noncanonical"),
            ([self.archive_info("extension//file")], "noncanonical"),
            ([self.archive_info("extension/./file")], "noncanonical"),
            ([self.archive_info("extension/file.")], "noncanonical"),
            ([self.archive_info("extension/file ")], "noncanonical"),
            ([self.archive_info("unexpected/file")], "top-level"),
            ([self.archive_info("extension/CON")], "reserved"),
            ([self.archive_info("extension/assets/com1.log")], "reserved"),
            ([self.archive_info("extension/assets/CON .txt")], "reserved"),
            ([self.archive_info("extension/CONIN$")], "reserved"),
            ([self.archive_info("extension/conout$.log")], "reserved"),
            ([self.archive_info("extension/LPT9.json")], "reserved"),
            ([self.archive_info("extension/assets/COM¹.png")], "reserved"),
            ([self.archive_info("extension/assets/com².log")], "reserved"),
            ([self.archive_info("extension/assets/CoM³.webp")], "reserved"),
            ([self.archive_info("extension/assets/LPT¹.png")], "reserved"),
            ([self.archive_info("extension/assets/lpt².log")], "reserved"),
            ([self.archive_info("extension/assets/LpT³.webp")], "reserved"),
            (
                [
                    self.archive_info("extension/assets/Readme"),
                    self.archive_info("extension/assets/README"),
                ],
                "collision",
            ),
            (
                [
                    self.archive_info("extension/assets/é"),
                    self.archive_info("extension/assets/e\u0301"),
                ],
                "collision",
            ),
        )
        for infos, expected in cases:
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(RuntimeError, expected):
                    release_artifact._validate_archive_entries(infos, vsix_size=1)
        with self.assertRaisesRegex(RuntimeError, "noncanonical"):
            release_artifact._canonical_archive_path("extension\\file")

    def test_rejects_file_directory_ancestor_conflicts(self) -> None:
        cases = (
            [
                self.archive_info("extension/assets"),
                self.archive_info("extension/assets/icon.svg"),
            ],
            [
                self.archive_info("extension/assets/icon.svg"),
                self.archive_info("extension/assets"),
            ],
        )
        for infos in cases:
            with self.subTest(names=[info.filename for info in infos]):
                with self.assertRaisesRegex(RuntimeError, "ancestor conflict"):
                    release_artifact._validate_archive_entries(infos, vsix_size=1)

    def test_rejects_tiny_packaged_and_prebuilt_runtime_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_prebuilt_inputs(root)
            (root / "dist" / "extension.cjs").write_bytes(b"x")
            with self.assertRaisesRegex(RuntimeError, "too small"):
                release_artifact.validate_prebuilt_inputs(root)

        with tempfile.TemporaryDirectory() as temporary:
            vsix = Path(temporary) / "fleximark.vsix"
            self.write_vsix(vsix, extension_runtime=b"x")
            with self.assertRaisesRegex(RuntimeError, "too small"):
                release_artifact.validate_vsix(vsix)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            daemons = self.write_prebuilt_inputs(root)
            daemons[0].write_bytes(b"")
            with self.assertRaisesRegex(RuntimeError, "empty"):
                release_artifact.validate_prebuilt_inputs(root)

        with tempfile.TemporaryDirectory() as temporary:
            vsix = Path(temporary) / "fleximark.vsix"
            self.write_vsix(vsix, empty_daemon=True)
            with self.assertRaisesRegex(RuntimeError, "empty"):
                release_artifact.validate_vsix(vsix)

    def test_rejects_special_encrypted_or_oversized_archive_entries(self) -> None:
        cases = (
            (
                [self.archive_info("extension/link", mode=stat.S_IFLNK | 0o777)],
                "regular",
            ),
            (
                [self.archive_info("extension/fifo", mode=stat.S_IFIFO | 0o644)],
                "regular",
            ),
            (
                [self.archive_info("extension/device", mode=stat.S_IFCHR | 0o644)],
                "regular",
            ),
            (
                [
                    self.archive_info(
                        "extension/assets/", mode=stat.S_IFDIR | 0o755, file_size=1
                    )
                ],
                "directory entry",
            ),
            ([self.archive_info("extension/file", encrypted=True)], "encrypted"),
            (
                [
                    self.archive_info(
                        "extension/file", file_size=10_000, compress_size=1
                    )
                ],
                "compression ratio",
            ),
        )
        for infos, expected in cases:
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(RuntimeError, expected):
                    release_artifact._validate_archive_entries(infos, vsix_size=1)

    def test_rejects_each_archive_resource_limit_before_payload_reads(self) -> None:
        info = self.archive_info("extension/file")
        cases = (
            ("MAX_VSIX_SIZE", 0, [info], 1, "maximum file size"),
            ("MAX_ENTRY_COUNT", 0, [info], 0, "entry count"),
            ("MAX_ENTRY_SIZE", 0, [info], 0, "maximum uncompressed"),
            (
                "MAX_TOTAL_UNCOMPRESSED_SIZE",
                1,
                [info, self.archive_info("extension/second")],
                0,
                "total uncompressed",
            ),
        )
        for constant, limit, infos, vsix_size, expected in cases:
            with (
                self.subTest(constant=constant),
                patch.object(release_artifact, constant, limit),
            ):
                with self.assertRaisesRegex(RuntimeError, expected):
                    release_artifact._validate_archive_entries(
                        infos, vsix_size=vsix_size
                    )

        with tempfile.TemporaryDirectory() as temporary:
            vsix = Path(temporary) / "fleximark.vsix"
            self.write_vsix(vsix)
            with patch.object(release_artifact, "MAX_METADATA_SIZE", 0):
                with self.assertRaisesRegex(RuntimeError, "metadata entry"):
                    release_artifact.validate_vsix(vsix)


if __name__ == "__main__":
    unittest.main()
