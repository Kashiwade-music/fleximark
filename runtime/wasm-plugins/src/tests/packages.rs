//! Role: Verify packages plugin execution invariants.
use super::*;
#[test]
fn capabilities_are_deny_by_default_even_when_requested() {
    let all = PluginCapabilities {
        read_workspace: true,
        write_workspace: true,
        environment: true,
        unsafe_html_output: true,
    };
    let sandbox = observed_sandbox(all.clone(), PluginCapabilities::default(), BTreeMap::new());
    assert!(sandbox.read_roots.is_empty() && sandbox.write_roots.is_empty());
    assert!(sandbox.environment.is_empty());
    assert!(!sandbox.unsafe_html_output);

    let requested = PluginCapabilities {
        read_workspace: true,
        write_workspace: true,
        ..PluginCapabilities::default()
    };
    let sandbox = observed_sandbox(
        requested,
        PluginCapabilities {
            read_workspace: true,
            ..PluginCapabilities::default()
        },
        BTreeMap::new(),
    );
    assert_eq!(sandbox.read_roots, vec!["C:/workspace"]);
    assert!(sandbox.write_roots.is_empty());

    let requested = PluginCapabilities {
        write_workspace: true,
        environment: true,
        ..PluginCapabilities::default()
    };
    let environment = BTreeMap::from([("FLEXIMARK_MODE".to_owned(), "test".to_owned())]);
    let sandbox = observed_sandbox(requested.clone(), requested, environment.clone());
    assert!(sandbox.read_roots.is_empty());
    assert_eq!(sandbox.write_roots, vec!["C:/workspace"]);
    assert_eq!(sandbox.environment, environment);

    let sandbox = observed_sandbox(
        PluginCapabilities::default(),
        all.clone(),
        BTreeMap::from([("SECRET".to_owned(), "not-exposed".to_owned())]),
    );
    assert!(sandbox.read_roots.is_empty() && sandbox.write_roots.is_empty());
    assert!(sandbox.environment.is_empty());
    assert!(!sandbox.unsafe_html_output);

    let ran = Arc::new(AtomicBool::new(false));
    let ran_by_runtime = Arc::clone(&ran);
    let untrusted_runtime = runtime(move |request, _, _| {
        ran_by_runtime.store(true, Ordering::Release);
        let HookRequest::TransformDocument { document } = request else {
            panic!("unexpected test path")
        };
        document_response(&document)
    });
    let mut requested = manifest("untrusted-fully-granted", false);
    requested.capabilities = all.clone();
    let mut host = PluginHost::new(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: false,
            workspace_root: "C:/workspace".to_owned(),
        },
    );
    host.register(requested, all, untrusted_runtime).unwrap();
    let result = host
        .transform_document("hello\n", &document(), &CancellationToken::default())
        .unwrap();
    assert_eq!(result.value, document());
    assert_eq!(
        result.diagnostics,
        vec![PluginDiagnostic {
            plugin_id: "untrusted-fully-granted".to_owned(),
            hook: Hook::TransformDocument,
            kind: PluginFailureKind::PolicyViolation,
            message: "workspace trust is required before plugin execution".to_owned(),
        }]
    );
    assert!(!ran.load(Ordering::Acquire));
}

#[test]
fn verified_registration_binds_config_manifest_signature_and_wasm() {
    assert_verified_package_rejections_are_retryable();
}

fn assert_verified_package_rejections_are_retryable() {
    let cases = [
        (
            VerifiedPackageCorruption::ManifestHash,
            "manifest hash mismatch",
        ),
        (
            VerifiedPackageCorruption::ManifestAfterSigning,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::Signature,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::PublicKey,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::ConfiguredId,
            "configured and manifested plugin ids differ",
        ),
        (VerifiedPackageCorruption::Wasm, "WASM hash mismatch"),
    ];
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"verified\"\napi_version = 1\nrequired = false\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        sha256(wasm)
    );
    let modified_manifest = manifest.replace("required = false", "required = true");
    let manifest_hash = sha256(manifest.as_bytes());
    let modified_manifest_hash = sha256(modified_manifest.as_bytes());
    let wrong_manifest_hash = sha256(b"different manifest bytes");
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let mut modified_signature = signature;
    modified_signature[0] ^= 1;
    let public_key = signing_public_key(&signing_key);
    let wrong_public_key = signing_public_key(&SigningKey::from_bytes(&[8; 32]));
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: true,
            workspace_root: "C:/workspace".to_owned(),
        },
        sha256(b"config"),
        1,
    )
    .unwrap();
    let empty_hash = host.plugin_set_hash();

    for (corruption, expected_message) in cases {
        let (configured_id, manifest_bytes, expected_hash, wasm_bytes, signature_bytes, signer_key) =
            match corruption {
                VerifiedPackageCorruption::ManifestHash => (
                    "verified",
                    manifest.as_bytes(),
                    wrong_manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::ManifestAfterSigning => (
                    "verified",
                    modified_manifest.as_bytes(),
                    modified_manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::Signature => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    modified_signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::PublicKey => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    wrong_public_key.as_str(),
                ),
                VerifiedPackageCorruption::ConfiguredId => (
                    "another-id",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::Wasm => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    b"tampered".as_slice(),
                    signature.as_slice(),
                    public_key.as_str(),
                ),
            };
        let error = host
            .register_verified(VerifiedPluginPackage {
                configured_id,
                config_order: 0,
                manifest_bytes,
                expected_manifest_sha256: expected_hash,
                wasm_bytes,
                signature_bytes,
                signer_public_key: signer_key,
                grants: PluginCapabilities::default(),
                environment: BTreeMap::new(),
            })
            .unwrap_err();
        match error {
            HostError::Integrity(message) => assert_eq!(message, expected_message),
            other => panic!("{corruption:?} returned the wrong error variant: {other}"),
        }
        assert_eq!(host.plugin_set_hash(), empty_hash);
    }

    // Every rejected package left config_order 0 available for the same valid package.
    host.register_verified(VerifiedPluginPackage {
        configured_id: "verified",
        config_order: 0,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &manifest_hash,
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    })
    .unwrap();
    assert_ne!(host.plugin_set_hash(), empty_hash);
}

#[derive(Clone, Copy, Debug)]
enum VerifiedPackageCorruption {
    ManifestHash,
    ManifestAfterSigning,
    Signature,
    PublicKey,
    ConfiguredId,
    Wasm,
}

#[test]
fn verified_registration_rejects_noncanonical_config_order_without_mutating_the_host() {
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"ordered\"\napi_version = 1\nrequired = false\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        sha256(wasm)
    );
    let signing_key = SigningKey::from_bytes(&[19; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let manifest_hash = sha256(manifest.as_bytes());
    let public_key = signing_public_key(&signing_key);
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: true,
            workspace_root: "C:/workspace".to_owned(),
        },
        sha256(b"ordered-config"),
        1,
    )
    .unwrap();
    let empty_hash = host.plugin_set_hash();
    let package = |config_order| VerifiedPluginPackage {
        configured_id: "ordered",
        config_order,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &manifest_hash,
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    };

    assert!(matches!(
        host.register_verified(package(1)),
        Err(HostError::Integrity(message))
            if message == "plugins must be registered once in canonical config order"
    ));
    assert_eq!(host.plugin_set_hash(), empty_hash);

    host.register_verified(package(0)).unwrap();
    let registered_hash = host.plugin_set_hash();
    assert_ne!(registered_hash, empty_hash);
    assert!(matches!(
        host.register_verified(package(0)),
        Err(HostError::Integrity(message))
            if message == "plugins must be registered once in canonical config order"
    ));
    assert_eq!(host.plugin_set_hash(), registered_hash);
}
