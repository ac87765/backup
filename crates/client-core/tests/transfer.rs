//! Docker 集成测试：真实 PostgreSQL + MinIO + 服务端。
//!
//! 这些测试由审查者在 WSL 中运行（本机沙箱不执行 Docker）：
//! `cargo test -p client-core --test transfer`。
//! 覆盖 Web ↔ 客户端互通、篡改中止、文件夹合并/下载、回收站与版本往返。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use client_core::{Client, ConflictPolicy, MemorySecretStore, NodeSort, NodeView, VersionView};
use common::{TestServer, TestUser, create_user, environment, start_server};
use crypto_core::{
    ContentHasher, ContentId, MasterKey, NodeId, NodeKey, NodeMetadata, VersionId, VersionMetadata,
    chunk_count_for_size, compute_name_token, derive_content_key, derive_name_key, encrypt_chunk,
    encrypt_node_metadata, encrypt_version_metadata, unwrap_node_key, wrap_node_key,
};
use reqwest::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use reqwest::{Client as HttpClient, Method};
use serde_json::{Value, json};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn from_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    let (pairs, _) = value.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("hex digit")
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = ContentHasher::new();
    hasher.update(bytes);
    hasher.finish_hex()
}

async fn json_response(response: reqwest::Response) -> Value {
    let bytes = response.bytes().await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

struct WebSession {
    http: HttpClient,
    base_url: String,
    cookie: String,
}

impl WebSession {
    async fn login(server: &TestServer, user: &TestUser) -> Self {
        let http = HttpClient::new();
        let prelogin: Value = json_response(
            http.post(format!("{}/api/v1/auth/prelogin", server.base_url))
                .header("x-requested-with", "backup-web")
                .header(CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(&json!({ "email": user.email })).unwrap())
                .send()
                .await
                .unwrap(),
        )
        .await;
        let params_json = serde_json::to_string(&prelogin["kdf_params"]).unwrap();
        let params = crypto_core::parse_kdf_params(&params_json).unwrap();
        let salt: [u8; 16] = from_hex(prelogin["kdf_salt"].as_str().unwrap())
            .try_into()
            .unwrap();
        let keys = crypto_core::derive_keys_from_password(&user.password, &salt, &params).unwrap();
        let response = http
            .post(format!("{}/api/v1/auth/login", server.base_url))
            .header("x-requested-with", "backup-web")
            .header(CONTENT_TYPE, "application/json")
            .body(
                serde_json::to_vec(&json!({
                    "email": user.email,
                    "auth_key": hex(keys.auth_key.expose_secret()),
                }))
                .unwrap(),
            )
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "Web 登录失败");
        let cookie = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| {
                value
                    .strip_prefix("__Host-session=")
                    .and_then(|rest| rest.split(';').next())
                    .map(str::to_owned)
            })
            .expect("缺少会话 Cookie");
        Self {
            http,
            base_url: server.base_url.clone(),
            cookie,
        }
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base_url, path))
            .header("x-requested-with", "backup-web")
            .header(COOKIE, format!("__Host-session={}", self.cookie))
    }

    async fn get_json(&self, path: &str) -> Value {
        let response = self.request(Method::GET, path).send().await.unwrap();
        assert!(response.status().is_success(), "GET {path} 失败");
        json_response(response).await
    }

    async fn post_json(&self, path: &str, body: &Value) -> Value {
        let response = self
            .request(Method::POST, path)
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "POST {path} 失败：{}",
            response.status()
        );
        json_response(response).await
    }
}

struct WebUpload {
    node_id: Uuid,
    version_id: Uuid,
}

/// 按 Web Worker 的流程构造并上传一个文件（可篡改一个块，确认时仍使用原始哈希）。
async fn web_upload(
    session: &WebSession,
    master_key: &MasterKey,
    parent_id: Uuid,
    name: &str,
    data: &[u8],
    tamper_first_chunk: bool,
) -> WebUpload {
    let node_id = Uuid::new_v4();
    let content_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let node_typed = NodeId::parse(&node_id.to_string()).unwrap();
    let content_typed = ContentId::parse(&content_id.to_string()).unwrap();
    let version_typed = VersionId::parse(&version_id.to_string()).unwrap();
    let parent_typed = NodeId::parse(&parent_id.to_string()).unwrap();
    let node_key = NodeKey::generate().unwrap();
    let wrapped = wrap_node_key(master_key, &node_typed, &node_key).unwrap();
    let meta = encrypt_node_metadata(
        &node_key,
        &node_typed,
        &NodeMetadata {
            name: name.to_owned(),
        },
    )
    .unwrap();
    let name_key = derive_name_key(master_key).unwrap();
    let token = compute_name_token(&name_key, &parent_typed, name).unwrap();
    let count = chunk_count_for_size(data.len() as u64).unwrap();
    let declared_size = data.len() as u64 + u64::from(count) * 41;
    let version_meta = encrypt_version_metadata(
        &node_key,
        &version_typed,
        &VersionMetadata {
            content_id: content_typed.clone(),
            size: data.len() as u64,
            mtime: 1_700_000_000,
            mime: "application/octet-stream".to_owned(),
            content_sha256: Some(sha256(data)),
        },
    )
    .unwrap();

    let created = session
        .post_json(
            "/api/v1/upload-sessions",
            &json!({
                "content_id": content_id,
                "kind": "new_file",
                "node_id": node_id,
                "parent_id": parent_id,
                "wrapped_key": hex(&wrapped),
                "encrypted_meta": hex(&meta),
                "name_token": token.to_hex(),
                "version_id": version_id,
                "version_meta": hex(&version_meta),
                "chunk_count": count,
                "declared_size": declared_size,
            }),
        )
        .await;
    let session_id = created["id"].as_str().unwrap();
    let indices: Vec<u32> = (0..count).collect();
    let presigned = session
        .post_json(
            &format!("/api/v1/upload-sessions/{session_id}/chunks/presign"),
            &json!({ "indices": indices }),
        )
        .await;
    let content_key = derive_content_key(&node_key, &content_typed).unwrap();
    let mut confirmed = Vec::new();
    for index in 0..count {
        let item = presigned["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["idx"].as_u64() == Some(u64::from(index)))
            .unwrap();
        let url = item["url"].as_str().unwrap();
        let start = index as usize * crypto_core::CHUNK_PLAINTEXT_SIZE;
        let end = ((index + 1) as usize * crypto_core::CHUNK_PLAINTEXT_SIZE).min(data.len());
        let plaintext = &data[start..end];
        let mut ciphertext = encrypt_chunk(
            &content_key,
            &content_typed,
            index,
            count,
            index + 1 == count,
            plaintext,
        )
        .unwrap();
        let ciphertext_sha = sha256(&ciphertext);
        if tamper_first_chunk && index == 0 {
            let last = ciphertext.len() - 1;
            ciphertext[last] ^= 0x01;
        }
        let response = HttpClient::new()
            .put(url)
            .body(ciphertext)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "PUT 块失败");
        confirmed.push(json!({
            "idx": index,
            "declared_size": item["expected_size"].as_i64().unwrap(),
            "sha256": ciphertext_sha,
        }));
    }
    session
        .post_json(
            &format!("/api/v1/upload-sessions/{session_id}/chunks/confirm"),
            &json!({ "chunks": confirmed }),
        )
        .await;
    session
        .post_json(
            &format!("/api/v1/upload-sessions/{session_id}/commit"),
            &json!({}),
        )
        .await;
    WebUpload {
        node_id,
        version_id,
    }
}

/// 用 crypto-core 独立解密密文（不经过 client-core 的下载实现）。
async fn web_decrypt(
    session: &WebSession,
    master_key: &MasterKey,
    node_id: Uuid,
    version_id: Uuid,
) -> Vec<u8> {
    let manifest = session
        .get_json(&format!(
            "/api/v1/nodes/{node_id}/versions/{version_id}/download"
        ))
        .await;
    let wrapped = from_hex(manifest["wrapped_key"].as_str().unwrap());
    let node_typed = NodeId::parse(&node_id.to_string()).unwrap();
    let node_key = unwrap_node_key(master_key, &node_typed, &wrapped).unwrap();
    let content_id = ContentId::parse(manifest["content_id"].as_str().unwrap()).unwrap();
    let version_typed = VersionId::parse(&version_id.to_string()).unwrap();
    let metadata = crypto_core::decrypt_version_metadata_for_content(
        &node_key,
        &version_typed,
        &content_id,
        &from_hex(manifest["encrypted_meta"].as_str().unwrap()),
    )
    .unwrap();
    let content_key = derive_content_key(&node_key, &content_id).unwrap();
    let count = manifest["chunk_count"].as_u64().unwrap() as u32;
    let mut output = Vec::new();
    for chunk in manifest["chunks"].as_array().unwrap() {
        let idx = chunk["idx"].as_u64().unwrap() as u32;
        let response = HttpClient::new()
            .get(chunk["url"].as_str().unwrap())
            .send()
            .await
            .unwrap();
        let ciphertext = response.bytes().await.unwrap();
        let plaintext = crypto_core::decrypt_chunk(
            &content_key,
            &content_id,
            idx,
            count,
            idx + 1 == count,
            &ciphertext,
        )
        .unwrap();
        output.extend_from_slice(&plaintext);
    }
    assert_eq!(output.len() as u64, metadata.size);
    output
}

async fn client_for(server: &TestServer, user: &TestUser, dir: &Path) -> Client {
    let client = Client::builder(&server.base_url)
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(dir)
        .build()
        .unwrap();
    client.login(&user.email, &user.password).await.unwrap();
    client
}

fn find_node(nodes: &[NodeView], name: &str) -> NodeView {
    nodes
        .iter()
        .find(|node| node.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("找不到节点 {name}"))
}

fn current_version(node: &NodeView) -> Uuid {
    node.current_version_id.expect("缺少当前版本")
}

async fn write_download(client: &Client, node: &NodeView, dir: &Path) -> PathBuf {
    let task = client
        .download_file(
            node.id,
            current_version(node),
            &node.name,
            dir,
            ConflictPolicy::KeepBoth,
        )
        .unwrap();
    task.run().await.unwrap().path
}

#[tokio::test]
async fn client_and_web_interoperate_in_both_directions() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let state = tempfile::tempdir().unwrap();
    let client = client_for(&server, &user, state.path()).await;
    let web = WebSession::login(&server, &user).await;

    let uploads = tempfile::tempdir().unwrap();
    let client_file = uploads.path().join("客户端上传.bin");
    let client_data: Vec<u8> = (0..1024 * 1024 + 37)
        .map(|index| (index % 239) as u8)
        .collect();
    std::fs::write(&client_file, &client_data).unwrap();
    let uploaded = client
        .upload_file(user.root_node_id, &client_file)
        .unwrap()
        .run()
        .await
        .unwrap();
    let decrypted = web_decrypt(
        &web,
        &user.master_key,
        uploaded.node_id,
        uploaded.version_id,
    )
    .await;
    assert_eq!(decrypted, client_data, "Web 流程必须能独立解密客户端上传");

    // folder-transfer spec：明文 SHA-256 与文件名只能以密文形式到达服务端。
    let version_meta: Vec<u8> = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT encrypted_meta FROM file_versions WHERE id = $1 AND user_id = $2",
    )
    .bind(uploaded.version_id)
    .bind(user.user_id)
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    let node_meta: Vec<u8> = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT encrypted_meta FROM nodes WHERE id = $1 AND user_id = $2",
    )
    .bind(uploaded.node_id)
    .bind(user.user_id)
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    assert!(
        !hex(&version_meta).contains(&sha256(&client_data)),
        "服务端不得出现明文 content_sha256"
    );
    assert!(
        !String::from_utf8_lossy(&node_meta).contains("客户端上传"),
        "服务端不得出现明文文件名"
    );

    let web_data = b"web upload -> native download".to_vec();
    let web_uploaded = web_upload(
        &web,
        &user.master_key,
        user.root_node_id,
        "web.txt",
        &web_data,
        false,
    )
    .await;
    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let node = find_node(&nodes, "web.txt");
    assert_eq!(node.id, web_uploaded.node_id);
    assert_eq!(node.current_version_id, Some(web_uploaded.version_id));
    let downloads = tempfile::tempdir().unwrap();
    let path = write_download(&client, &node, downloads.path()).await;
    assert_eq!(std::fs::read(path).unwrap(), web_data);
}

#[tokio::test]
async fn tampered_chunk_fails_download_without_leaving_a_file() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let state = tempfile::tempdir().unwrap();
    let client = client_for(&server, &user, state.path()).await;
    let web = WebSession::login(&server, &user).await;
    web_upload(
        &web,
        &user.master_key,
        user.root_node_id,
        "tampered.bin",
        b"tampered chunk test",
        true,
    )
    .await;

    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let node = find_node(&nodes, "tampered.bin");
    let downloads = tempfile::tempdir().unwrap();
    let task = client
        .download_file(
            node.id,
            current_version(&node),
            &node.name,
            downloads.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap();
    let error = task.run().await.unwrap_err();
    assert!(
        error.to_string().contains("完整性") || error.to_string().contains("SHA"),
        "必须报告完整性错误：{error:?}"
    );
    let remaining: Vec<PathBuf> = std::fs::read_dir(downloads.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        remaining.is_empty(),
        "失败后不得留下目标或临时文件：{remaining:?}"
    );
}

#[tokio::test]
async fn folder_upload_merges_and_download_preserves_structure() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let state = tempfile::tempdir().unwrap();
    let client = client_for(&server, &user, state.path()).await;

    let local = tempfile::tempdir().unwrap();
    let photos = local.path().join("photos");
    std::fs::create_dir_all(photos.join("2025")).unwrap();
    std::fs::create_dir_all(photos.join("2026")).unwrap();
    std::fs::create_dir_all(photos.join("empty")).unwrap();
    std::fs::write(photos.join("2025/a.jpg"), b"a-v1").unwrap();
    std::fs::write(photos.join("2025/b.jpg"), b"b-v1").unwrap();
    std::fs::write(photos.join("2026/c.jpg"), b"c-v1").unwrap();

    let first = client
        .upload_folder(user.root_node_id, &photos)
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(first.counts.added, 3);
    assert_eq!(first.counts.failed, 0);

    std::fs::write(photos.join("2025/a.jpg"), b"a-v2!").unwrap();
    std::fs::write(photos.join("2026/d.jpg"), b"d-v1").unwrap();
    let second = client
        .upload_folder(user.root_node_id, &photos)
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(second.counts.added, 1);
    assert_eq!(second.counts.updated, 1);
    assert_eq!(second.counts.unchanged, 2);

    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let cloud = find_node(&nodes, "photos");
    let downloads = tempfile::tempdir().unwrap();
    let outcome = client
        .download_folder(
            cloud.id,
            "photos",
            downloads.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(outcome.status, client_core::FolderJobStatus::Done);
    let root = downloads.path().join("photos");
    assert!(root.join("empty").is_dir(), "空文件夹必须保留");
    assert_eq!(std::fs::read(root.join("2025/a.jpg")).unwrap(), b"a-v2!");
    assert_eq!(std::fs::read(root.join("2025/b.jpg")).unwrap(), b"b-v1");
    assert_eq!(std::fs::read(root.join("2026/c.jpg")).unwrap(), b"c-v1");
    assert_eq!(std::fs::read(root.join("2026/d.jpg")).unwrap(), b"d-v1");
}

#[tokio::test]
async fn trash_restore_and_version_restore_roundtrip() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let state = tempfile::tempdir().unwrap();
    let client = client_for(&server, &user, state.path()).await;

    let local = tempfile::tempdir().unwrap();
    let path = local.path().join("report.txt");
    std::fs::write(&path, b"version-1").unwrap();
    let first = client
        .upload_file(user.root_node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();
    std::fs::write(&path, b"version-2").unwrap();
    let second = client
        .upload_new_version(first.node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();

    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let node = find_node(&nodes, "report.txt");
    assert_eq!(node.current_version_id, Some(second.version_id));

    let versions: Vec<VersionView> = client.list_versions(node.id).await.unwrap();
    assert_eq!(versions.len(), 2);
    let old_version = versions
        .iter()
        .find(|version| version.id == first.version_id)
        .expect("旧版本必须保留")
        .id;

    let downloads = tempfile::tempdir().unwrap();
    let path_current = write_download(&client, &node, downloads.path()).await;
    assert_eq!(std::fs::read(path_current).unwrap(), b"version-2");

    let node_id = node.id;
    client.delete_to_trash(node_id).await.unwrap();
    let trash = client.list_trash().await.unwrap();
    assert!(trash.iter().any(|item| item.node.id == node_id));
    assert!(
        client
            .list_folder(user.root_node_id, NodeSort::NAME_ASC)
            .await
            .unwrap()
            .iter()
            .all(|node| node.id != node_id)
    );
    let restored = client.restore_from_trash(node_id, None).await.unwrap();
    assert_eq!(restored.id, node.id);
    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let restored_node = find_node(&nodes, "report.txt");

    let downloads = tempfile::tempdir().unwrap();
    let path_restored = write_download(&client, &restored_node, downloads.path()).await;
    assert_eq!(std::fs::read(path_restored).unwrap(), b"version-2");

    client.restore_version(node.id, old_version).await.unwrap();
    let nodes = client
        .list_folder(user.root_node_id, NodeSort::NAME_ASC)
        .await
        .unwrap();
    let latest = find_node(&nodes, "report.txt");
    let downloads = tempfile::tempdir().unwrap();
    let path_old = write_download(&client, &latest, downloads.path()).await;
    assert_eq!(std::fs::read(path_old).unwrap(), b"version-1");
}

#[tokio::test]
async fn rename_and_move_work_without_prior_listing() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let state_a = tempfile::tempdir().unwrap();
    let uploader = client_for(&server, &user, state_a.path()).await;

    let local = tempfile::tempdir().unwrap();
    let path = local.path().join("before.txt");
    std::fs::write(&path, b"rename/move without list").unwrap();
    let uploaded = uploader
        .upload_file(user.root_node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(uploaded.node_id.to_string().len(), 36);

    // 新 Client，未 list 任何目录；rename_node 必须自己列出当前父目录加载节点密钥。
    let state_b = tempfile::tempdir().unwrap();
    let renamer = client_for(&server, &user, state_b.path()).await;
    let renamed = renamer
        .rename_node(uploaded.node_id, user.root_node_id, "renamed.txt", 1)
        .await
        .unwrap();
    assert_eq!(renamed.name, "renamed.txt");
    assert_eq!(renamed.row_version, 2);

    let destination = renamer
        .create_folder(user.root_node_id, "dest")
        .await
        .unwrap();

    // 再换一个从未 list 的 Client 做移动；current_parent_id 让核心自行加载当前目录。
    let state_c = tempfile::tempdir().unwrap();
    let mover = client_for(&server, &user, state_c.path()).await;
    let moved = mover
        .move_node(
            uploaded.node_id,
            user.root_node_id,
            destination.id,
            "renamed.txt",
            renamed.row_version,
        )
        .await
        .unwrap();
    assert_eq!(moved.parent_id, Some(destination.id));
    assert_eq!(moved.name, "renamed.txt");
}
