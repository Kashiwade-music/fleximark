//! Role: Verify real WASM engine pipeline integration and rollback.
use ed25519_dalek::{Signer, SigningKey};
use fleximark_engine::{DocumentEngine, EngineError, RenderConfig};
use fleximark_model::{DocumentUri, PositionEncoding, SourceProvenance, TransformId};
use fleximark_plugin_api::{CancellationToken, Hook, PluginFailureKind};
use fleximark_plugin_host::{ExecutionLimits, HostPolicy, PluginHost, VerifiedPluginPackage};
use fleximark_plugin_sdk::PluginCapabilities;
use fleximark_render_html::RenderContext;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Command,
    sync::{Arc, OnceLock},
};
fn content_hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn content_hash(source: &str) -> String {
    content_hash_bytes(source.as_bytes())
}
fn fixture_component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let fixture_manifest = manifest_dir
            .join("../../tests/fixtures/plugin-component/Cargo.toml")
            .canonicalize()
            .unwrap();
        let fixture_target = manifest_dir.join("../../target/fixture-wasm");
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--quiet",
                "--manifest-path",
                fixture_manifest.to_str().unwrap(),
                "--target-dir",
                fixture_target.to_str().unwrap(),
                "--target",
                "wasm32-wasip2",
                "--locked",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "fixture component build failed");
        std::fs::read(fixture_target.join("wasm32-wasip2/debug/fleximark_fixture_plugin.wasm"))
            .unwrap()
    })
}

fn fixture_host(required: bool) -> Arc<PluginHost> {
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"engine-fixture\"\napi_version = 1\nrequired = {required}\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        content_hash_bytes(wasm)
    );
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let public_key = signing_key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: true,
            workspace_root: ".".to_owned(),
        },
        content_hash("engine-config"),
        1,
    )
    .unwrap();
    host.register_verified(VerifiedPluginPackage {
        configured_id: "engine-fixture",
        config_order: 0,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &content_hash(&manifest),
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    })
    .unwrap();
    Arc::new(host)
}

#[test]
fn configured_open_change_and_resync_preserve_pipeline_state_and_diagnostic_order() {
    let optional_host = fixture_host(false);
    let config = RenderConfig::for_plugins(RenderContext::default(), None, optional_host.as_ref());
    let mut configured = DocumentEngine::open_configured(
        DocumentUri("file:///configured-pipeline.md".into()),
        1,
        "# one\n".to_owned(),
        PositionEncoding::Utf16,
        config,
        optional_host.clone(),
        &CancellationToken::default(),
    )
    .unwrap();
    let mut plain = DocumentEngine::open(
        DocumentUri("file:///configured-pipeline.md".into()),
        1,
        "# one\n".to_owned(),
        PositionEncoding::Utf16,
    )
    .unwrap();
    assert_eq!(
        configured.document().document_version,
        plain.document().document_version
    );
    assert_eq!(configured.document().uri, plain.document().uri);
    assert_eq!(
        configured.document().blocks[0].kind,
        plain.document().blocks[0].kind
    );
    let SourceProvenance::Original {
        ranges: plain_ranges,
        ..
    } = &plain.document().blocks[0].provenance
    else {
        panic!("plain parsing must retain original provenance")
    };
    let SourceProvenance::Derived {
        ranges, transform, ..
    } = &configured.document().blocks[0].provenance
    else {
        panic!("plugin preprocessing must remap provenance before identity reconciliation")
    };
    assert_eq!(ranges, plain_ranges);
    assert_eq!(transform, &TransformId("plugin-preprocess-v1".to_owned()));
    assert_ne!(
        configured.document().blocks[0].id,
        plain.document().blocks[0].id
    );
    assert!(configured.plugin_diagnostics().is_empty());

    let cancelled = CancellationToken::default();
    cancelled.cancel();
    configured
        .change_full_text_with_cancellation(2, "# two\n".to_owned(), &cancelled)
        .unwrap();
    plain
        .change_full_text_with_cancellation(2, "# two\n".to_owned(), &CancellationToken::default())
        .unwrap();
    assert_eq!(configured.source(), "# two\n");
    assert_eq!(configured.document().document_version, 2);
    assert_eq!(configured.content_hash(), content_hash("# two\n"));
    assert_eq!(configured.source(), plain.source());
    assert_eq!(configured.content_hash(), plain.content_hash());
    assert_eq!(
        configured.document().blocks[0].kind,
        plain.document().blocks[0].kind
    );
    assert_eq!(
        configured.document().blocks[0].provenance,
        plain.document().blocks[0].provenance
    );
    assert!(!configured.is_out_of_sync());
    assert_eq!(
        configured
            .plugin_diagnostics()
            .iter()
            .map(|diagnostic| (&diagnostic.hook, &diagnostic.kind))
            .collect::<Vec<_>>(),
        [
            (&Hook::PreprocessSource, &PluginFailureKind::Cancelled),
            (&Hook::TransformBlock, &PluginFailureKind::Cancelled),
            (&Hook::TransformDocument, &PluginFailureKind::Cancelled),
        ]
    );

    assert!(matches!(
        configured.change_full_text_with_cancellation(
            2,
            "stale\n".to_owned(),
            &CancellationToken::default(),
        ),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(matches!(
        plain.change_full_text_with_cancellation(
            2,
            "stale\n".to_owned(),
            &CancellationToken::default(),
        ),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(configured.is_out_of_sync());
    assert!(plain.is_out_of_sync());
    configured
        .resynchronize_with_cancellation(2, "# resynced\n".to_owned(), &cancelled)
        .unwrap();
    plain
        .resynchronize_with_cancellation(
            2,
            "# resynced\n".to_owned(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(configured.source(), "# resynced\n");
    assert_eq!(configured.document().document_version, 2);
    assert!(!configured.is_out_of_sync());
    assert_eq!(configured.source(), plain.source());
    assert_eq!(configured.content_hash(), plain.content_hash());
    assert_eq!(
        configured.document().blocks[0].provenance,
        plain.document().blocks[0].provenance
    );
    assert_eq!(
        configured
            .plugin_diagnostics()
            .iter()
            .map(|diagnostic| &diagnostic.hook)
            .collect::<Vec<_>>(),
        [
            &Hook::PreprocessSource,
            &Hook::TransformBlock,
            &Hook::TransformDocument
        ]
    );
}

#[test]
fn required_plugin_cancellation_rolls_back_authoritative_state_and_render_snapshot() {
    let required_host = fixture_host(true);
    let config = RenderConfig::for_plugins(RenderContext::default(), None, required_host.as_ref());
    let mut session = DocumentEngine::open_configured(
        DocumentUri("file:///required-rollback.md".into()),
        1,
        "before\n".to_owned(),
        PositionEncoding::Utf8,
        config,
        required_host,
        &CancellationToken::default(),
    )
    .unwrap();
    let initial = session
        .render_configured(&CancellationToken::default())
        .unwrap()
        .snapshot;
    let original_document = session.document().clone();
    let original_hash = session.content_hash();
    let original_diagnostics = session.plugin_diagnostics().to_vec();
    let cancelled = CancellationToken::default();
    cancelled.cancel();

    let error = session
        .change_full_text_with_cancellation(2, "after\n".to_owned(), &cancelled)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "plugin pipeline failed: plugin engine-fixture is required and failed (Cancelled): document version was cancelled"
    );
    assert_eq!(session.source(), "before\n");
    assert_eq!(session.content_hash(), original_hash);
    assert_eq!(session.document(), &original_document);
    assert_eq!(session.plugin_diagnostics(), original_diagnostics);
    assert!(!session.is_out_of_sync());

    let unchanged = session
        .render_configured(&CancellationToken::default())
        .unwrap()
        .snapshot;
    assert_eq!(unchanged.blocks, initial.blocks);

    assert_eq!(
        session
            .render_configured(&cancelled)
            .unwrap_err()
            .to_string(),
        "plugin pipeline failed: operation cancelled"
    );
    let after_cancelled_render = session
        .render_configured(&CancellationToken::default())
        .unwrap()
        .snapshot;
    assert_eq!(after_cancelled_render.blocks, initial.blocks);
}
