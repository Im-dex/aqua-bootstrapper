use std::path::Path;

use serde_json::Value;
use sigstore_verification::{Attestation, AttestationClient, FetchParams};

use crate::aqua::{OWNER, REPO};
use crate::error::{Error, Result};
use crate::util::progress::{self, Progress};

const GITHUB_ACTIONS_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const SLSA_PROVENANCE_V1: &str = "https://slsa.dev/provenance/v1";

pub async fn verify_aqua_release_asset(
    artifact_path: &Path,
    version: &str,
    sha256_hex: &str,
) -> Result<()> {
    if !version.starts_with('v') {
        return Err(Error::Attestation(format!(
            "Aqua version must be a release tag starting with 'v': {version}"
        )));
    }

    verify_slsa_policy(artifact_path, version, sha256_hex).await
}

async fn verify_slsa_policy(artifact_path: &Path, version: &str, sha256_hex: &str) -> Result<()> {
    let expected_ref = format!("refs/tags/{version}");
    let expected_repo_short = format!("github.com/{OWNER}/{REPO}");
    let expected_repo_https = format!("https://github.com/{OWNER}/{REPO}");
    let expected_repo_git = format!("git+https://github.com/{OWNER}/{REPO}");

    if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Attestation(format!(
            "invalid SHA-256 digest produced for downloaded Aqua asset: {sha256_hex}"
        )));
    }

    progress::step(format!(
        "Fetching Aqua GitHub attestations for sha256:{}...",
        short_digest(sha256_hex)
    ));
    let attestations = fetch_aqua_attestations(sha256_hex).await?;
    if attestations.is_empty() {
        return Err(Error::Attestation(format!(
            "no GitHub attestations found for sha256:{sha256_hex}"
        )));
    }
    progress::step(format!(
        "Fetched {} Aqua GitHub attestation(s)",
        attestations.len()
    ));

    let mut failures = Vec::new();
    let mut progress = Progress::new(
        "Checking Aqua GitHub attestations",
        Some(attestations.len() as u64),
    );

    for (index, attestation) in attestations.iter().enumerate() {
        let statement = match slsa_statement_from_attestation(attestation) {
            Ok(statement) => statement,
            Err(error) => {
                failures.push(format!("attestation #{index}: {error}"));
                progress.advance(1);
                continue;
            }
        };

        if let Err(error) = inspect_slsa_statement(
            &statement,
            sha256_hex,
            &expected_ref,
            [
                expected_repo_short.as_str(),
                expected_repo_https.as_str(),
                expected_repo_git.as_str(),
            ],
        ) {
            failures.push(format!("attestation #{index}: {error}"));
            progress.advance(1);
            continue;
        }

        match sigstore_verification::verify_attestations(
            std::slice::from_ref(attestation),
            artifact_path,
            None,
        )
        .await
        {
            Ok(()) => {
                progress.advance(1);
                progress.finish("Verified Aqua SLSA provenance attestation");
                tracing::debug!(
                    repository = %format!("{OWNER}/{REPO}"),
                    oidc_issuer = GITHUB_ACTIONS_OIDC_ISSUER,
                    git_ref = %expected_ref,
                    subject_digest = %format!("sha256:{sha256_hex}"),
                    "Aqua SLSA provenance policy accepted"
                );
                return Ok(());
            }
            Err(error) => {
                failures.push(format!("attestation #{index}: {error}"));
                progress.advance(1);
            }
        }
    }

    Err(Error::Attestation(format!(
        "no verified SLSA provenance matched Aqua policy; failures: {}",
        failures.join("; ")
    )))
}

fn short_digest(sha256_hex: &str) -> &str {
    sha256_hex.get(..12).unwrap_or(sha256_hex)
}

async fn fetch_aqua_attestations(sha256_hex: &str) -> Result<Vec<Attestation>> {
    let client = AttestationClient::builder()
        .build()
        .map_err(|error| Error::Attestation(error.to_string()))?;
    let params = FetchParams {
        owner: OWNER.to_string(),
        repo: Some(format!("{OWNER}/{REPO}")),
        digest: format!("sha256:{sha256_hex}"),
        limit: 30,
        predicate_type: None,
    };

    client
        .fetch_attestations(params)
        .await
        .map_err(|error| Error::Attestation(error.to_string()))
}

fn slsa_statement_from_attestation(
    attestation: &Attestation,
) -> std::result::Result<Value, String> {
    let bundle = sigstore_verification::bundle::parse_bundle(attestation)
        .map_err(|error| error.to_string())?;

    if bundle.payload.is_empty() {
        return Err("attestation bundle has no DSSE payload".to_string());
    }

    serde_json::from_slice(&bundle.payload)
        .map_err(|error| format!("attestation payload is not valid JSON: {error}"))
}

fn inspect_slsa_statement(
    statement: &Value,
    sha256_hex: &str,
    expected_ref: &str,
    expected_repositories: [&str; 3],
) -> std::result::Result<(), String> {
    match statement.get("predicateType").and_then(Value::as_str) {
        Some(SLSA_PROVENANCE_V1) => {}
        Some(_) => return Err("attestation predicate is not SLSA provenance v1".to_string()),
        None => return Err("attestation predicateType is missing".to_string()),
    }

    if !subject_digest_matches(statement, sha256_hex) {
        return Err(format!("subject digest sha256:{sha256_hex} was not found"));
    }

    if !json_contains_string(statement, expected_ref) {
        return Err(format!("release ref {expected_ref} was not found"));
    }

    if !expected_repositories
        .iter()
        .any(|repository| json_contains_string(statement, repository))
    {
        return Err(format!(
            "repository {OWNER}/{REPO} was not found in SLSA provenance"
        ));
    }

    tracing::debug!(
        repository = %format!("{OWNER}/{REPO}"),
        oidc_issuer = GITHUB_ACTIONS_OIDC_ISSUER,
        git_ref = %expected_ref,
        subject_digest = %format!("sha256:{sha256_hex}"),
        "Aqua attestation policy invariants"
    );

    Ok(())
}

fn subject_digest_matches(statement: &Value, sha256_hex: &str) -> bool {
    statement
        .get("subject")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|subject| {
            subject
                .get("digest")
                .and_then(|digest| digest.get("sha256"))
                .and_then(Value::as_str)
                .is_some_and(|digest| digest.eq_ignore_ascii_case(sha256_hex))
        })
}

fn json_contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(actual) => actual == expected,
        Value::Array(items) => items
            .iter()
            .any(|item| json_contains_string(item, expected)),
        Value::Object(object) => object
            .values()
            .any(|child| json_contains_string(child, expected)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::json;
    use sigstore_verification::api::{Attestation, DsseEnvelope, Signature, SigstoreBundle};

    use super::*;

    #[test]
    fn extracts_slsa_statement_from_attestation_bundle() {
        let statement = json!({
            "predicateType": SLSA_PROVENANCE_V1,
            "subject": [
                {
                    "digest": {
                        "sha256": "abc123"
                    }
                }
            ]
        });
        let payload = STANDARD.encode(serde_json::to_vec(&statement).unwrap());
        let attestation = Attestation {
            bundle: Some(SigstoreBundle {
                media_type: "application/vnd.dev.sigstore.bundle+json;version=0.3".to_string(),
                dsse_envelope: Some(DsseEnvelope {
                    payload,
                    payload_type: "application/vnd.in-toto+json".to_string(),
                    signatures: vec![Signature {
                        sig: "signature".to_string(),
                        keyid: None,
                    }],
                }),
                verification_material: None,
                message_signature: None,
            }),
            bundle_url: None,
        };

        assert_eq!(
            slsa_statement_from_attestation(&attestation).unwrap(),
            statement
        );
    }
}
