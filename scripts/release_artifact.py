from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import tempfile
import unicodedata
import zipfile
from pathlib import Path
from typing import Any
from xml.etree import ElementTree

from _targets import TARGETS
from _tools import ROOT, script_entrypoint


IDENTITY_SCHEMA_VERSION = 1
DEFAULT_VSIX = ROOT / "fleximark.vsix"
DEFAULT_IDENTITY = ROOT / "fleximark.vsix.identity.json"
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")
GIT_HEAD_PATTERN = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
SEMVER_PATTERN = re.compile(
    r"(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)"
    r"(?:-(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z"
)
FORBIDDEN_COMPONENTS = {"src", "test", "markdown_for_debug", "node_modules"}
ALLOWED_TOP_LEVEL = {"[Content_Types].xml", "extension.vsixmanifest", "extension"}
WINDOWS_RESERVED_BASENAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    *(f"COM{index}" for index in range(1, 10)),
    *(f"LPT{index}" for index in range(1, 10)),
}
REQUIRED_METADATA = {
    "[Content_Types].xml",
    "extension.vsixmanifest",
    "extension/package.json",
    "extension/bin/manifest.json",
}
REQUIRED_RUNTIME = {
    "extension/dist/extension.cjs",
    "extension/dist/web/preview-client/vscode-host.js",
}
MAX_VSIX_SIZE = 256 * 1024 * 1024
MAX_ENTRY_COUNT = 20_000
MAX_ENTRY_SIZE = 128 * 1024 * 1024
MAX_TOTAL_UNCOMPRESSED_SIZE = 512 * 1024 * 1024
MAX_COMPRESSION_RATIO = 200
MAX_METADATA_SIZE = 1024 * 1024
MIN_RUNTIME_SIZE = 1024


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _json_object(payload: bytes, label: str) -> dict[str, Any]:
    try:
        value = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"{label} is not valid JSON") from error
    if not isinstance(value, dict):
        raise RuntimeError(f"{label} must be a JSON object")
    return value


def _canonical_archive_path(name: str) -> tuple[tuple[str, ...], bool]:
    if not name or "\\" in name or name.startswith("/") or "\x00" in name:
        raise RuntimeError(f"VSIX contains a noncanonical path: {name!r}")
    is_directory = name.endswith("/")
    raw = name[:-1] if is_directory else name
    parts = raw.split("/")
    reserved = any(
        part.split(".", 1)[0].rstrip(" ").upper() in WINDOWS_RESERVED_BASENAMES
        for part in parts
    )
    if (
        not raw
        or any(
            not part
            or part in {".", ".."}
            or ":" in part
            or part.rstrip(" .") != part
            or any(ord(character) < 32 or ord(character) == 127 for character in part)
            for part in parts
        )
        or parts[0] not in ALLOWED_TOP_LEVEL
        or (parts[0] != "extension" and len(parts) != 1)
    ):
        raise RuntimeError(
            f"VSIX contains a noncanonical or disallowed top-level path: {name}"
        )
    if reserved:
        raise RuntimeError(f"VSIX contains a Windows reserved device path: {name}")
    return tuple(parts), is_directory


def _unix_file_type(info: zipfile.ZipInfo) -> int:
    if info.create_system != 3:
        return 0
    return stat.S_IFMT((info.external_attr >> 16) & 0xFFFF)


def _validate_archive_entries(
    infos: list[zipfile.ZipInfo], *, vsix_size: int
) -> dict[str, zipfile.ZipInfo]:
    if vsix_size > MAX_VSIX_SIZE:
        raise RuntimeError("VSIX exceeds the maximum file size")
    if not infos or len(infos) > MAX_ENTRY_COUNT:
        raise RuntimeError("VSIX has an invalid archive entry count")

    seen: dict[str, str] = {}
    file_keys: set[str] = set()
    descendant_parent_keys: set[str] = set()
    by_name: dict[str, zipfile.ZipInfo] = {}
    total_size = 0
    for info in infos:
        parts, is_directory = _canonical_archive_path(info.filename)
        collision_key = unicodedata.normalize("NFC", "/".join(parts)).casefold()
        ancestor_keys = {
            unicodedata.normalize("NFC", "/".join(parts[:index])).casefold()
            for index in range(1, len(parts))
        }
        if ancestor_keys.intersection(file_keys) or (
            not is_directory and collision_key in descendant_parent_keys
        ):
            raise RuntimeError(
                f"VSIX contains a file/directory ancestor conflict: {info.filename}"
            )
        previous = seen.get(collision_key)
        if previous is not None:
            raise RuntimeError(
                f"VSIX path collision after casefold/Unicode normalization: "
                f"{previous!r}, {info.filename!r}"
            )
        seen[collision_key] = info.filename
        descendant_parent_keys.update(ancestor_keys)
        if not is_directory:
            file_keys.add(collision_key)
        by_name[info.filename] = info

        if info.flag_bits & 1:
            raise RuntimeError(f"VSIX contains an encrypted entry: {info.filename}")
        file_type = _unix_file_type(info)
        if is_directory:
            if file_type not in {0, stat.S_IFDIR} or info.file_size != 0:
                raise RuntimeError("VSIX directory entry has an invalid size or type")
            continue
        if file_type not in {0, stat.S_IFREG}:
            raise RuntimeError("VSIX entries must be regular files")
        if info.file_size < 0 or info.file_size > MAX_ENTRY_SIZE:
            raise RuntimeError("VSIX entry exceeds the maximum uncompressed size")
        total_size += info.file_size
        if total_size > MAX_TOTAL_UNCOMPRESSED_SIZE:
            raise RuntimeError("VSIX exceeds the total uncompressed size limit")
        if info.file_size and (
            info.compress_size <= 0
            or info.file_size > info.compress_size * MAX_COMPRESSION_RATIO
        ):
            raise RuntimeError("VSIX entry exceeds the compression ratio limit")

    if not REQUIRED_METADATA.union(REQUIRED_RUNTIME).issubset(by_name):
        raise RuntimeError(
            "VSIX is missing required extension metadata or runtime files"
        )
    for name in REQUIRED_METADATA:
        info = by_name[name]
        if info.is_dir() or info.file_size > MAX_METADATA_SIZE:
            raise RuntimeError(
                f"VSIX metadata entry has an invalid size or type: {name}"
            )
    for name in REQUIRED_RUNTIME:
        info = by_name[name]
        if info.is_dir() or info.file_size <= MIN_RUNTIME_SIZE:
            raise RuntimeError(
                f"VSIX required runtime file is too small or invalid: {name}"
            )
    return by_name


def _archive_version(archive: zipfile.ZipFile) -> str:
    package = _json_object(
        archive.read("extension/package.json"), "packaged extension/package.json"
    )
    version = package.get("version")
    if not isinstance(version, str) or not version:
        raise RuntimeError("packaged extension version is missing")
    if not SEMVER_PATTERN.fullmatch(version):
        raise RuntimeError("packaged extension version is not strict SemVer")
    name = package.get("name")
    publisher = package.get("publisher")
    if (
        not isinstance(name, str)
        or not name
        or not isinstance(publisher, str)
        or not publisher
    ):
        raise RuntimeError("packaged extension identity is missing")

    try:
        manifest = ElementTree.fromstring(archive.read("extension.vsixmanifest"))
    except ElementTree.ParseError as error:
        raise RuntimeError("extension.vsixmanifest is not valid XML") from error
    identities = [
        element
        for element in manifest.iter()
        if element.tag.rsplit("}", 1)[-1] == "Identity"
    ]
    if len(identities) != 1:
        raise RuntimeError("VSIX manifest must contain exactly one Identity")
    identity = identities[0]
    if (
        identity.get("Version") != version
        or identity.get("Id") != name
        or identity.get("Publisher") != publisher
    ):
        raise RuntimeError(
            "VSIX manifest version or identity does not match package.json"
        )
    return version


def _validate_daemon_manifest(
    archive: zipfile.ZipFile, by_name: dict[str, zipfile.ZipInfo]
) -> None:
    manifest = _json_object(
        archive.read("extension/bin/manifest.json"), "packaged daemon manifest"
    )
    if set(manifest) != {"schemaVersion", "protocolVersion", "artifacts"}:
        raise RuntimeError("packaged daemon manifest has unexpected fields")
    if (
        type(manifest.get("schemaVersion")) is not int
        or manifest["schemaVersion"] != 1
        or type(manifest.get("protocolVersion")) is not int
        or manifest["protocolVersion"] != 1
    ):
        raise RuntimeError("packaged daemon manifest has unsupported versions")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or len(artifacts) != len(TARGETS):
        raise RuntimeError("packaged daemon manifest must contain all six targets")

    for artifact, target in zip(artifacts, TARGETS, strict=True):
        if not isinstance(artifact, dict):
            raise RuntimeError("packaged daemon manifest artifact must be an object")
        if set(artifact) != {"platform", "arch", "path", "sha256"}:
            raise RuntimeError(
                "packaged daemon manifest artifact has unexpected fields"
            )
        expected_path = target.bin_relative_path.as_posix()
        if (
            artifact.get("platform") != target.platform
            or artifact.get("arch") != target.arch
            or artifact.get("path") != expected_path
        ):
            raise RuntimeError(
                "packaged daemon manifest target order or path is invalid"
            )
        expected_hash = artifact.get("sha256")
        if not isinstance(expected_hash, str) or not SHA256_PATTERN.fullmatch(
            expected_hash
        ):
            raise RuntimeError("packaged daemon manifest has an invalid SHA-256")
        info = by_name[f"extension/{expected_path}"]
        if info.file_size <= 0:
            raise RuntimeError("packaged daemon must not be empty")
        actual_hash = hashlib.sha256(
            archive.read(f"extension/{expected_path}")
        ).hexdigest()
        if actual_hash != expected_hash:
            raise RuntimeError("packaged daemon checksum does not match the manifest")
        if _unix_file_type(info) != stat.S_IFREG:
            raise RuntimeError("packaged daemons must be regular Unix ZIP entries")
        mode = (info.external_attr >> 16) & 0xFFFF
        if target.platform != "win32" and mode & 0o111 == 0:
            raise RuntimeError("packaged Unix daemon is not executable")


def validate_prebuilt_inputs(root: Path = ROOT) -> None:
    for relative in (
        Path("dist/extension.cjs"),
        Path("dist/web/preview-client/vscode-host.js"),
    ):
        path = root / relative
        try:
            metadata = path.lstat()
        except FileNotFoundError as error:
            raise RuntimeError(
                f"release prebuilt input is missing: {relative}"
            ) from error
        if not stat.S_ISREG(metadata.st_mode):
            raise RuntimeError(
                f"release prebuilt input is not a regular file: {relative}"
            )
        if metadata.st_size <= MIN_RUNTIME_SIZE:
            raise RuntimeError(f"release prebuilt input is too small: {relative}")
        if metadata.st_size > MAX_ENTRY_SIZE:
            raise RuntimeError(f"release prebuilt input is too large: {relative}")

    manifest_path = root / "bin" / "manifest.json"
    try:
        metadata = manifest_path.lstat()
    except FileNotFoundError as error:
        raise RuntimeError("release prebuilt manifest is missing") from error
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_METADATA_SIZE:
        raise RuntimeError("release prebuilt manifest has an invalid size or type")
    manifest = _json_object(manifest_path.read_bytes(), "release prebuilt manifest")
    if set(manifest) != {"schemaVersion", "protocolVersion", "artifacts"}:
        raise RuntimeError("release prebuilt manifest has unexpected fields")
    if (
        type(manifest.get("schemaVersion")) is not int
        or manifest["schemaVersion"] != 1
        or type(manifest.get("protocolVersion")) is not int
        or manifest["protocolVersion"] != 1
    ):
        raise RuntimeError("release prebuilt manifest has unsupported versions")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or len(artifacts) != len(TARGETS):
        raise RuntimeError("release prebuilt manifest must contain all six targets")

    for artifact, target in zip(artifacts, TARGETS, strict=True):
        expected_path = target.bin_relative_path.as_posix()
        if not isinstance(artifact, dict) or set(artifact) != {
            "platform",
            "arch",
            "path",
            "sha256",
        }:
            raise RuntimeError("release prebuilt manifest artifact is invalid")
        expected_hash = artifact.get("sha256")
        if (
            artifact.get("platform") != target.platform
            or artifact.get("arch") != target.arch
            or artifact.get("path") != expected_path
            or not isinstance(expected_hash, str)
            or not SHA256_PATTERN.fullmatch(expected_hash)
        ):
            raise RuntimeError("release prebuilt manifest target or hash is invalid")
        daemon = root / target.bin_relative_path
        try:
            daemon_metadata = daemon.lstat()
        except FileNotFoundError as error:
            raise RuntimeError(
                f"release prebuilt daemon is missing: {expected_path}"
            ) from error
        if not stat.S_ISREG(daemon_metadata.st_mode):
            raise RuntimeError(
                f"release prebuilt daemon is not regular: {expected_path}"
            )
        if daemon_metadata.st_size <= 0:
            raise RuntimeError(f"release prebuilt daemon is empty: {expected_path}")
        if daemon_metadata.st_size > MAX_ENTRY_SIZE:
            raise RuntimeError(f"release prebuilt daemon is too large: {expected_path}")
        if (
            os.name != "nt"
            and target.platform != "win32"
            and daemon_metadata.st_mode & 0o111 == 0
        ):
            raise RuntimeError(
                f"release prebuilt daemon is not executable: {expected_path}"
            )
        if sha256_file(daemon) != expected_hash:
            raise RuntimeError(
                f"release prebuilt daemon hash mismatch: {expected_path}"
            )


def validate_vsix(vsix: Path, *, expected_version: str | None = None) -> str:
    if not vsix.is_file():
        raise RuntimeError(f"VSIX does not exist: {vsix}")
    try:
        with zipfile.ZipFile(vsix) as archive:
            infos = archive.infolist()
            by_name = _validate_archive_entries(infos, vsix_size=vsix.stat().st_size)
            for name in by_name:
                parts, _ = _canonical_archive_path(name)
                if (
                    FORBIDDEN_COMPONENTS.intersection(parts)
                    or parts[-1] == "parserPlugin.js"
                ):
                    raise RuntimeError(
                        f"VSIX contains an unsafe or forbidden path: {name}"
                    )
            corrupt = archive.testzip()
            if corrupt is not None:
                raise RuntimeError(f"VSIX archive CRC failed for: {corrupt}")
            version = _archive_version(archive)
            if expected_version is not None and version != expected_version:
                raise RuntimeError(
                    f"VSIX version {version} does not match expected {expected_version}"
                )
            _validate_daemon_manifest(archive, by_name)
            return version
    except zipfile.BadZipFile as error:
        raise RuntimeError("VSIX is not a valid ZIP archive") from error
    except KeyError as error:
        raise RuntimeError(f"VSIX is missing required file: {error.args[0]}") from error


def create_identity(
    vsix: Path,
    identity_path: Path,
    *,
    expected_version: str,
    git_tag: str,
    source_git_head: str,
) -> dict[str, Any]:
    if not vsix.is_file():
        raise RuntimeError(f"VSIX does not exist: {vsix}")
    hash_before_validation = sha256_file(vsix)
    version = validate_vsix(vsix, expected_version=expected_version)
    hash_after_validation = sha256_file(vsix)
    if hash_before_validation != hash_after_validation:
        raise RuntimeError("VSIX changed while it was being validated")
    identity = {
        "schemaVersion": IDENTITY_SCHEMA_VERSION,
        "file": vsix.name,
        "version": version,
        "sha256": hash_after_validation,
        "gitTag": git_tag,
        "sourceGitHead": source_git_head,
    }
    _validate_git_identity(identity)
    identity_path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=f".{identity_path.name}.",
            suffix=".tmp",
            dir=identity_path.parent,
            delete=False,
        ) as stream:
            temporary_path = Path(stream.name)
            stream.write(json.dumps(identity, indent=2) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary_path, identity_path)
    finally:
        if temporary_path is not None and temporary_path.exists():
            temporary_path.unlink()
    return identity


def _validate_git_identity(identity: dict[str, Any]) -> None:
    git_tag = identity.get("gitTag")
    if (
        not isinstance(git_tag, str)
        or not git_tag
        or any(ord(character) < 32 or ord(character) == 127 for character in git_tag)
    ):
        raise RuntimeError("release identity git tag is invalid")
    source_git_head = identity.get("sourceGitHead")
    if not isinstance(source_git_head, str) or not GIT_HEAD_PATTERN.fullmatch(
        source_git_head
    ):
        raise RuntimeError("release identity source git head is invalid")
    if git_tag != f"v{identity.get('version')}":
        raise RuntimeError("release identity git tag does not match its version")


def read_identity(identity_path: Path) -> dict[str, Any]:
    try:
        metadata = identity_path.lstat()
    except FileNotFoundError as error:
        raise RuntimeError(
            f"release identity does not exist: {identity_path}"
        ) from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_size <= 0
        or metadata.st_size > MAX_METADATA_SIZE
    ):
        raise RuntimeError("release identity has an invalid size or type")
    if not identity_path.is_file():
        raise RuntimeError(f"release identity does not exist: {identity_path}")
    identity = _json_object(identity_path.read_bytes(), "release identity")
    if set(identity) != {
        "schemaVersion",
        "file",
        "version",
        "sha256",
        "gitTag",
        "sourceGitHead",
    }:
        raise RuntimeError("release identity has unexpected fields")
    if (
        type(identity.get("schemaVersion")) is not int
        or identity["schemaVersion"] != IDENTITY_SCHEMA_VERSION
    ):
        raise RuntimeError("release identity has an unsupported schema version")
    if not isinstance(identity.get("file"), str) or not identity["file"]:
        raise RuntimeError("release identity file is invalid")
    if not isinstance(identity.get("version"), str) or not identity["version"]:
        raise RuntimeError("release identity version is invalid")
    if not SEMVER_PATTERN.fullmatch(identity["version"]):
        raise RuntimeError("release identity version is not strict SemVer")
    digest = identity.get("sha256")
    if not isinstance(digest, str) or not SHA256_PATTERN.fullmatch(digest):
        raise RuntimeError("release identity SHA-256 is invalid")
    _validate_git_identity(identity)
    return identity


def verify_identity(
    vsix: Path,
    identity_path: Path,
    *,
    expected_sha256: str | None = None,
    expected_git_tag: str | None = None,
    expected_source_git_head: str | None = None,
) -> dict[str, Any]:
    identity = read_identity(identity_path)
    if not vsix.is_file():
        raise RuntimeError(f"VSIX does not exist: {vsix}")
    if identity["file"] != vsix.name:
        raise RuntimeError("release identity names a different VSIX")
    if expected_sha256 is not None:
        if not SHA256_PATTERN.fullmatch(expected_sha256):
            raise RuntimeError("expected VSIX SHA-256 is invalid")
        if identity["sha256"] != expected_sha256:
            raise RuntimeError("release identity does not match the expected SHA-256")
    if expected_git_tag is not None and identity["gitTag"] != expected_git_tag:
        raise RuntimeError("release identity does not match the expected git tag")
    if (
        expected_source_git_head is not None
        and identity["sourceGitHead"] != expected_source_git_head
    ):
        raise RuntimeError(
            "release identity does not match the expected source git head"
        )
    hash_before_validation = sha256_file(vsix)
    if hash_before_validation != identity["sha256"]:
        raise RuntimeError("VSIX SHA-256 does not match the release identity")
    validate_vsix(vsix, expected_version=identity["version"])
    hash_after_validation = sha256_file(vsix)
    if hash_before_validation != hash_after_validation:
        raise RuntimeError("VSIX changed while it was being validated")
    return identity


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Create or verify an exact FlexiMark release artifact identity"
    )
    parser.add_argument("operation", choices=("create", "verify"))
    parser.add_argument("--vsix", type=Path, default=DEFAULT_VSIX)
    parser.add_argument("--identity", type=Path, default=DEFAULT_IDENTITY)
    parser.add_argument("--expected-version")
    parser.add_argument(
        "--expected-sha256", default=os.environ.get("FLEXIMARK_EXPECTED_SHA256")
    )
    parser.add_argument("--git-tag")
    parser.add_argument("--source-git-head")
    parser.add_argument(
        "--expected-git-tag", default=os.environ.get("FLEXIMARK_EXPECTED_GIT_TAG")
    )
    parser.add_argument(
        "--expected-source-git-head",
        default=os.environ.get("FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD"),
    )
    parser.add_argument("--print-sha256", action="store_true")
    args = parser.parse_args()

    if args.operation == "create":
        if (
            args.expected_sha256 is not None
            or args.expected_git_tag is not None
            or args.expected_source_git_head is not None
        ):
            raise RuntimeError("create does not accept verification expectations")
        if not args.expected_version or not args.git_tag or not args.source_git_head:
            raise RuntimeError("create requires version, git tag, and source git head")
        identity = create_identity(
            args.vsix,
            args.identity,
            expected_version=args.expected_version,
            git_tag=args.git_tag,
            source_git_head=args.source_git_head,
        )
    else:
        if (
            args.expected_version is not None
            or args.git_tag is not None
            or args.source_git_head is not None
        ):
            raise RuntimeError("verify reads release metadata from the identity")
        identity = verify_identity(
            args.vsix,
            args.identity,
            expected_sha256=args.expected_sha256,
            expected_git_tag=args.expected_git_tag,
            expected_source_git_head=args.expected_source_git_head,
        )
    if args.print_sha256:
        print(identity["sha256"])


if __name__ == "__main__":
    script_entrypoint(main)
