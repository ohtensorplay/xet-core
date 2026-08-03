use std::fs::File;
use std::io::{Read, Write};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use http::header;
use sha2::{Digest, Sha256};
use xet_client::cas_client::auth::TokenRefresher;
use xet_client::hub_client::Operation;
use xet_pkg::legacy::data_client::download_async;
use xet_pkg::legacy::progress_tracking::{GroupProgressCallbackUpdater, ProgressUpdate, TrackingProgressUpdater};
use xet_pkg::legacy::{FileUploadSession, Sha256Policy, XetFileInfo, clean_file, default_config};
use xet_runtime::core::XetContext;

use crate::constants::{
    XET_ACCESS_TOKEN_HEADER, XET_CAS_URL, XET_FILE_ID, XET_SESSION_ID, XET_TOKEN_EXPIRATION_HEADER,
};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

fn xet_runtime() -> &'static XetContext {
    static RUNTIME: OnceLock<XetContext> = OnceLock::new();
    RUNTIME.get_or_init(|| XetContext::default().expect("xet context"))
}

use crate::errors::{GitXetError, Result};
use crate::git_repo::GitRepo;
use crate::git_url::GitUrl;
use crate::lfs_agent_protocol::{
    GitLFSProtocolError, InitRequestInner, ProgressUpdater, TransferAgent, TransferRequest,
};
use crate::token_refresher::new_git_token_refresher;

// This implements a Git LFS custom transfer agent that uploads and downloads files using the Xet protocol.
#[derive(Default)]
pub struct XetAgent {
    repo: OnceLock<GitRepo>,
    remote_url: Option<GitUrl>,
}

impl TransferAgent for XetAgent {
    async fn init_upload(&mut self, req: &InitRequestInner) -> Result<()> {
        self.init_remote(req)
    }

    async fn init_download(&mut self, req: &InitRequestInner) -> Result<()> {
        self.init_remote(req)
    }

    async fn upload_one<W: Write + Send + Sync + 'static>(
        &mut self,
        req: &TransferRequest,
        progress_updater: ProgressUpdater<W>,
    ) -> Result<()> {
        // Get the token refresher set up before the dummy progress update below,
        // so that if the internal git credential helper needs to prompt the user for credential,
        // only one prompt is presented.
        let repo = self.repo.get().unwrap(); // protocol state guarantees self.repo is set.

        let user_agent_headers = {
            let mut h = header::HeaderMap::new();
            h.insert(header::USER_AGENT, header::HeaderValue::from_static(USER_AGENT));
            h
        };

        let session_id = req.action.header.get(XET_SESSION_ID).map(|s| s.as_str()).unwrap_or_default();
        let token_refresher: Arc<dyn TokenRefresher> = Arc::new(new_git_token_refresher(
            xet_runtime(),
            repo,
            self.remote_url.clone(),
            &req.action.href,
            Operation::Upload,
            session_id,
            Some(Arc::new(user_agent_headers.clone())),
        )?);
        // From git-lfs:
        // > First worker is the only one allowed to start immediately.
        // > The rest wait until successful response from 1st worker to
        // > make sure only 1 login prompt is presented if necessary.
        //
        // Xet upload doesn't invoke interactive login, so we send a response right away
        // with positive progress to trigger simultaneous uploads.
        //
        // For reference see https://github.com/git-lfs/git-lfs/blob/2c7de1f90cbe13bf9c1ed43b84dda88bb32f2ba4/tq/adapterbase.go#L156
        // and https://github.com/git-lfs/git-lfs/blob/2c7de1f90cbe13bf9c1ed43b84dda88bb32f2ba4/tq/custom.go#L304
        progress_updater.update_bytes_so_far(1)?;

        let xet_updater = Arc::new(XetProgressUpdaterWrapper {
            updater: progress_updater,
        });

        let cas_url = req
            .action
            .header
            .get(XET_CAS_URL)
            .ok_or_else(|| GitXetError::internal("MEGA repository server didn't provide a CAS URL"))?
            .clone();
        let token = req
            .action
            .header
            .get(XET_ACCESS_TOKEN_HEADER)
            .ok_or_else(|| GitXetError::internal("MEGA repository server didn't provide a CAS access token"))?
            .clone();
        let token_expiry: u64 = req
            .action
            .header
            .get(XET_TOKEN_EXPIRATION_HEADER)
            .ok_or_else(|| {
                GitXetError::internal("MEGA repository server didn't provide a CAS access token expiration")
            })?
            .parse()
            .map_err(GitXetError::internal)?;

        let headers = user_agent_headers;

        let mut config = default_config(
            xet_runtime(),
            cas_url,
            Some((token, token_expiry)),
            Some(token_refresher),
            Some(Arc::new(headers)),
        )?
        .disable_progress_aggregation();
        if !session_id.is_empty() {
            config.session.session_id = Some(session_id.to_owned());
        }
        let session = FileUploadSession::new(config.into()).await?;
        let bridge = GroupProgressCallbackUpdater::start(session.clone(), xet_updater);

        let Some(file_path) = &req.path else {
            return Err(GitLFSProtocolError::bad_syntax("file path not provided for upload request").into());
        };

        let upload_result = async {
            clean_file(session.clone(), file_path, Sha256Policy::from_hex(&req.oid)).await?;

            // We need to actually upload the shard after each file upload to have the files registered, because
            //
            // 1. LFS custom transfer protocol is sequential: git-lfs waits for the upload/download result of the one
            //    file before sending the request to process the next one;
            // 2. git-lfs doesn't tell agents how many files to upload/download at the initiation phase;
            // 3. After sending a termination signal, git-lfs waits for 30s and sends SIGKILL to the agent. SIGKILL is
            //    not like SIGINT, it can't be intercepted or ignored by a process.
            // 4. Xet system is not a real-time system that guarantees response within any duration. Batching and thus
            //    effectively delaying shard upload means we risk data loss.
            //
            // See https://github.com/git-lfs/git-lfs/blob/2c7de1f90cbe13bf9c1ed43b84dda88bb32f2ba4/tq/custom.go#L233
            session.finalize().await?;
            Ok::<(), GitXetError>(())
        }
        .await;

        bridge.finalize().await;
        upload_result?;

        Ok(())
    }

    async fn download_one<W: Write + Send + Sync + 'static>(
        &mut self,
        req: &TransferRequest,
        progress_updater: ProgressUpdater<W>,
    ) -> Result<std::path::PathBuf> {
        let repo = self.repo.get().unwrap(); // protocol state guarantees self.repo is set.
        let user_agent_headers = user_agent_headers();
        let session_id = action_header(req, XET_SESSION_ID).unwrap_or_default();
        let token_refresher: Arc<dyn TokenRefresher> = Arc::new(new_git_token_refresher(
            xet_runtime(),
            repo,
            self.remote_url.clone(),
            &req.action.href,
            Operation::Download,
            session_id,
            Some(Arc::new(user_agent_headers.clone())),
        )?);
        let cas_url = required_action_header(req, XET_CAS_URL, "CAS URL")?;
        let token = required_action_header(req, XET_ACCESS_TOKEN_HEADER, "CAS access token")?;
        let token_expiry = parse_token_expiry(req)?;
        let file_id = required_action_header(req, XET_FILE_ID, "Xet file ID")?;

        let temp_dir = repo.git_path()?.join("lfs").join("tmp");
        std::fs::create_dir_all(&temp_dir)?;
        let temp_path = tempfile::Builder::new()
            .prefix("git-xet-download-")
            .tempfile_in(temp_dir)?
            .into_temp_path();
        let destination = temp_path.to_path_buf();
        let xet_updater: Arc<dyn TrackingProgressUpdater> = Arc::new(XetProgressUpdaterWrapper {
            updater: progress_updater,
        });
        download_async(
            xet_runtime(),
            vec![(
                XetFileInfo::new_with_sha256(file_id, req.size, req.oid.clone()),
                destination.to_string_lossy().into_owned(),
            )],
            Some(cas_url),
            Some((token, token_expiry)),
            Some(token_refresher),
            Some(vec![xet_updater]),
            Some(Arc::new(user_agent_headers)),
        )
        .await?;

        verify_lfs_download(destination, req.size, req.oid.clone()).await?;
        temp_path.keep().map_err(GitXetError::internal)
    }

    async fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

impl XetAgent {
    fn init_remote(&mut self, req: &InitRequestInner) -> Result<()> {
        let repo = GitRepo::open_from_cur_dir()?;
        let remote_url = match repo.remote_name_to_url(&req.remote) {
            Ok(url) => url,
            Err(_) => GitUrl::from_str(&req.remote)?,
        };
        self.repo.get_or_init(|| repo);
        self.remote_url = Some(remote_url);
        Ok(())
    }
}

fn user_agent_headers() -> header::HeaderMap {
    let mut headers = header::HeaderMap::new();
    headers.insert(header::USER_AGENT, header::HeaderValue::from_static(USER_AGENT));
    headers
}

fn action_header<'a>(req: &'a TransferRequest, name: &str) -> Option<&'a str> {
    req.action.header.get(name).map(String::as_str)
}

fn required_action_header(req: &TransferRequest, name: &str, description: &str) -> Result<String> {
    action_header(req, name)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| GitXetError::internal(format!("MEGA repository server didn't provide a {description}")))
}

fn parse_token_expiry(req: &TransferRequest) -> Result<u64> {
    required_action_header(req, XET_TOKEN_EXPIRATION_HEADER, "CAS access token expiration")?
        .parse()
        .map_err(GitXetError::internal)
}

async fn verify_lfs_download(path: std::path::PathBuf, expected_size: u64, expected_oid: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            size = size
                .checked_add(read as u64)
                .ok_or_else(|| GitXetError::internal("downloaded file size overflow"))?;
            hasher.update(&buffer[..read]);
        }
        let actual_oid: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        if size != expected_size || actual_oid != expected_oid {
            return Err(GitXetError::internal("downloaded Xet object failed Git LFS integrity verification"));
        }
        Ok(())
    })
    .await
    .map_err(GitXetError::internal)?
}

struct XetProgressUpdaterWrapper<W: Write + Send + Sync + 'static> {
    updater: ProgressUpdater<W>,
}

#[async_trait]
impl<W: Write + Send + Sync + 'static> TrackingProgressUpdater for XetProgressUpdaterWrapper<W> {
    async fn register_updates(&self, updates: ProgressUpdate) {
        let _ = self.updater.update_bytes_so_far(updates.total_bytes_completed);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use sha2::{Digest, Sha256};

    use super::verify_lfs_download;

    #[tokio::test]
    async fn verifies_downloaded_lfs_content_before_handoff() {
        let content = b"git-xet-direct-download";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(content).unwrap();
        let oid: String = Sha256::digest(content).iter().map(|byte| format!("{byte:02x}")).collect();

        verify_lfs_download(file.path().to_path_buf(), content.len() as u64, oid.clone())
            .await
            .unwrap();
        assert!(
            verify_lfs_download(file.path().to_path_buf(), content.len() as u64 + 1, oid)
                .await
                .is_err()
        );
    }
}
