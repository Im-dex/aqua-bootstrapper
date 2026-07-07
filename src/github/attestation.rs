use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use reqwest::Client;
use serde_json::Value;

use crate::aqua::{OWNER, REPO};
use crate::error::{Error, Result};

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

    let verified =
        sigstore_verification::verify_github_attestation(artifact_path, OWNER, REPO, None, None)
            .await
            .map_err(|error| Error::Attestation(error.to_string()))?;

    if !verified {
        return Err(Error::Attestation(
            "sigstore-verification returned a negative verification result".to_string(),
        ));
    }

    verify_slsa_policy(version, sha256_hex).await
}

async fn verify_slsa_policy(version: &str, sha256_hex: &str) -> Result<()> {
    let expected_ref = format!("refs/tags/{version}");
    let expected_repo_short = format!("github.com/{OWNER}/{REPO}");
    let expected_repo_https = format!("https://github.com/{OWNER}/{REPO}");
    let expected_repo_git = format!("git+https://github.com/{OWNER}/{REPO}");

    if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Attestation(format!(
            "invalid SHA-256 digest produced for downloaded Aqua asset: {sha256_hex}"
        )));
    }

    let statements = fetch_slsa_statements(sha256_hex).await?;
    let mut failures = Vec::new();

    for statement in &statements {
        match inspect_slsa_statement(
            statement,
            sha256_hex,
            &expected_ref,
            [
                expected_repo_short.as_str(),
                expected_repo_https.as_str(),
                expected_repo_git.as_str(),
            ],
        ) {
            Ok(()) => {
                tracing::debug!(
                    repository = %format!("{OWNER}/{REPO}"),
                    oidc_issuer = GITHUB_ACTIONS_OIDC_ISSUER,
                    git_ref = %expected_ref,
                    subject_digest = %format!("sha256:{sha256_hex}"),
                    "Aqua SLSA provenance policy accepted"
                );
                return Ok(());
            }
            Err(error) => failures.push(error),
        }
    }

    Err(Error::Attestation(format!(
        "no verified SLSA provenance matched Aqua policy; failures: {}",
        failures.join("; ")
    )))
}

async fn fetch_slsa_statements(sha256_hex: &str) -> Result<Vec<Value>> {
    let url = format!(
        "https://api.github.com/orgs/{OWNER}/attestations/sha256:{sha256_hex}?per_page=30&predicate_type=provenance"
    );

    let client = Client::new();
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .header("User-Agent", "aqua-bootstrapper")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::Attestation(format!(
            "GitHub attestations API returned {}",
            response.status()
        )));
    }

    let response: Value = response.json().await?;
    let mut statements = Vec::new();
    collect_dsse_payloads(&response, &mut statements);

    for bundle_url in collect_bundle_urls(&response) {
        let bundle = fetch_attestation_bundle(&client, &bundle_url).await?;
        collect_dsse_payloads(&bundle, &mut statements);
    }

    if statements.is_empty() {
        return Err(Error::Attestation(format!(
            "no SLSA payloads found for sha256:{sha256_hex}"
        )));
    }

    Ok(statements)
}

async fn fetch_attestation_bundle(client: &Client, bundle_url: &str) -> Result<Value> {
    let url = reqwest::Url::parse(bundle_url).map_err(|error| {
        Error::Attestation(format!(
            "invalid GitHub attestation bundle_url {bundle_url}: {error}"
        ))
    })?;

    if url.scheme() != "https" {
        return Err(Error::Attestation(format!(
            "GitHub attestation bundle_url must use https: {bundle_url}"
        )));
    }

    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "aqua-bootstrapper")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::Attestation(format!(
            "GitHub attestation bundle_url returned {}",
            response.status()
        )));
    }

    Ok(response.json().await?)
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

fn collect_bundle_urls(value: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_bundle_urls_inner(value, &mut urls);
    urls
}

fn collect_bundle_urls_inner(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(bundle_url) = object.get("bundle_url").and_then(Value::as_str)
                && !urls.iter().any(|url| url == bundle_url)
            {
                urls.push(bundle_url.to_string());
            }

            for child in object.values() {
                collect_bundle_urls_inner(child, urls);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_bundle_urls_inner(item, urls);
            }
        }
        _ => {}
    }
}

fn collect_dsse_payloads(value: &Value, statements: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            if let Some(payload) = object.get("payload").and_then(Value::as_str)
                && let Some(statement) = decode_statement_payload(payload)
            {
                statements.push(statement);
            }

            for child in object.values() {
                collect_dsse_payloads(child, statements);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_dsse_payloads(item, statements);
            }
        }
        _ => {}
    }
}

fn decode_statement_payload(payload: &str) -> Option<Value> {
    let bytes = STANDARD
        .decode(payload)
        .or_else(|_| STANDARD_NO_PAD.decode(payload))
        .ok()?;

    serde_json::from_slice(&bytes).ok()
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
    use serde_json::json;

    use super::*;

    #[test]
    fn collect_bundle_urls_finds_nested_urls_once() {
        let response = json!({
            "attestations": [
                { "bundle_url": "https://example.test/bundle-1.json" },
                {
                    "nested": {
                        "bundle_url": "https://example.test/bundle-2.json"
                    }
                },
                { "bundle_url": "https://example.test/bundle-1.json" }
            ]
        });

        assert_eq!(
            collect_bundle_urls(&response),
            vec![
                "https://example.test/bundle-1.json".to_string(),
                "https://example.test/bundle-2.json".to_string(),
            ]
        );
    }
}
