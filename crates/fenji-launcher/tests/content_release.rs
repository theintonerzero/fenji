//! `fenji-content`'s latest release, fetched from GitHub and checked as the launcher will check it
//! before each start (retranslation spec §6.3, RT6): the pack's signature against the content key
//! Fenji carries, then the pack itself, for the build Fenji pins.
use fenji_launcher::manifest::PINNED_BUILD;
use fenji_pack::{CONTENT_PUBLIC_KEY, read_pack, verify};

/// `fenji-content`'s latest release, as GitHub's API gives it.
const LATEST: &str = "https://api.github.com/repos/theintonerzero/fenji-content/releases/latest";

async fn get(client: &reqwest::Client, url: &str) -> Vec<u8> {
    let res = client.get(url).send().await.unwrap();
    assert!(res.status().is_success(), "{url}: {}", res.status());
    res.bytes().await.unwrap().to_vec()
}

/// Run by hand, with `--ignored`, after each content release (docs/development.md, "Releasing a
/// content pack").
#[tokio::test]
#[ignore = "fetches fenji-content's latest release from GitHub"]
async fn fenji_contents_latest_release_is_signed_with_the_content_key() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("fenji-launcher-tests/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap();
    let release: serde_json::Value = serde_json::from_slice(&get(&client, LATEST).await).unwrap();
    let tag = release["tag_name"].as_str().unwrap();
    let asset = |name: &str| {
        release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|asset| asset["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{tag} has no {name}"))["browser_download_url"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let file = format!("fenji-content-{tag}.tar.gz");
    let pack = get(&client, &asset(&file)).await;
    let signature = get(&client, &asset(&format!("{file}.minisig"))).await;
    let signature = String::from_utf8(signature).unwrap();
    verify(&pack, &signature, CONTENT_PUBLIC_KEY).unwrap();
    let pack = read_pack(&pack).unwrap();
    assert_eq!(pack.manifest.version, tag);
    assert_eq!(pack.manifest.build, PINNED_BUILD);
}
