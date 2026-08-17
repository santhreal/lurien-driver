//! JS dialog (`alert`/`confirm`/`prompt`/`beforeunload`) and page-initiated
//! download capture via WebDriver BiDi `browsingContext.*` events.
//!
//! Firefox surfaces user prompts and downloads as local BiDi events. This module
//! records them into a shared, cheaply-cloneable [`DialogLog`] (mirroring
//! [`crate::network::NetworkLog`]) so the agent can:
//!
//! - **Confirm alert-based XSS**: the `alert()` message is captured from
//!   `userPromptOpened` even when the prompt is auto-handled, so a payload that
//!   pops `alert(document.domain)` is *proven* fired without the automation
//!   hanging on the modal.
//! - **Read `confirm`/`prompt` text** and answer them via
//!   [`crate::Page::handle_user_prompt`] (when launched with the `ignore`
//!   prompt-handler so the prompt stays open).
//! - **Inspect page-initiated downloads**: suggested filename (path-traversal /
//!   exfil probes) and source URL (without a file landing on disk).
//!
//! The log never blocks the browser: under the BiDi default prompt handler
//! (`dismiss and notify`) the events still fire, so recording is side-effect
//! free. Auto-handling policy lives one layer up (the bridge), keeping this a
//! pure observation primitive.

use std::sync::Arc;
use tokio::sync::RwLock;

use rustenium_bidi_definitions::browsing_context::events::{
    DownloadEnd, DownloadWillBegin, UserPromptClosed, UserPromptOpened,
};
use rustenium_bidi_definitions::browsing_context::types::{
    DownloadCanceledParamsDownloadCompleteParamsUnion as DownloadUnion, UserPromptType,
};
use rustenium_bidi_definitions::session::types::UserPromptHandlerType;
use rustenium_bidi_definitions::Event;

/// Upper bound on retained dialogs/downloads so a hostile page that spams
/// `alert()` in a loop cannot drive unbounded memory growth (Law 7). Oldest
/// entries are dropped first; the most recent, what the agent cares about
/// always survive.
const MAX_ENTRIES: usize = 1000;

/// One captured JS user prompt.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CapturedDialog {
    /// Browsing context (tab/iframe) the prompt fired in.
    pub context: String,
    /// `alert` | `confirm` | `prompt` | `beforeunload`.
    pub kind: String,
    /// The dialog's message text (the XSS evidence for `alert(...)`).
    pub message: String,
    /// Default value pre-filled in a `prompt()` box, if any.
    pub default_value: Option<String>,
    /// The handler Firefox reported it will apply (`accept`/`dismiss`/`ignore`/
    /// `dismiss and notify`).
    pub handler: String,
    /// `Some(true/false)` once the prompt closed (accepted/dismissed); `None`
    /// while it is still open (only reachable under the `ignore` handler).
    pub accepted: Option<bool>,
    /// Text submitted when the prompt was answered, if any.
    pub user_text: Option<String>,
}

/// One page-initiated download.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CapturedDownload {
    /// Browsing context that initiated the download.
    pub context: String,
    /// Server-suggested filename (inspect for path traversal / exfil).
    pub suggested_filename: String,
    /// Source URL of the download.
    pub url: String,
    /// `will-begin` | `complete` | `canceled`.
    pub status: String,
    /// Local path the file was written to, when the download completed.
    pub filepath: Option<String>,
}

#[derive(Default)]
struct Inner {
    dialogs: Vec<CapturedDialog>,
    downloads: Vec<CapturedDownload>,
}

impl Inner {
    fn push_dialog(&mut self, d: CapturedDialog) {
        self.dialogs.push(d);
        if self.dialogs.len() > MAX_ENTRIES {
            let overflow = self.dialogs.len() - MAX_ENTRIES;
            self.dialogs.drain(0..overflow);
        }
    }

    fn push_download(&mut self, d: CapturedDownload) {
        self.downloads.push(d);
        if self.downloads.len() > MAX_ENTRIES {
            let overflow = self.downloads.len() - MAX_ENTRIES;
            self.downloads.drain(0..overflow);
        }
    }
}

/// Shared, cloneable handle to the dialog + download capture buffer.
#[derive(Clone, Default)]
pub struct DialogLog {
    inner: Arc<RwLock<Inner>>,
}

/// Map the typed `UserPromptType` to its stable wire string.
fn prompt_kind(t: &UserPromptType) -> &'static str {
    match t {
        UserPromptType::Alert => "alert",
        UserPromptType::Beforeunload => "beforeunload",
        UserPromptType::Confirm => "confirm",
        UserPromptType::Prompt => "prompt",
    }
}

/// Map the typed handler to its stable wire string.
fn handler_str(h: &UserPromptHandlerType) -> &'static str {
    match h {
        UserPromptHandlerType::Accept => "accept",
        UserPromptHandlerType::Dismiss => "dismiss",
        UserPromptHandlerType::Ignore => "ignore",
        UserPromptHandlerType::DismissAndNotify => "dismiss and notify",
    }
}

impl DialogLog {
    /// An empty log. Capture begins when it is installed on a session.
    pub fn new() -> Self {
        Self::default()
    }

    /// All captured dialogs, oldest first.
    pub async fn dialogs(&self) -> Vec<CapturedDialog> {
        self.inner.read().await.dialogs.clone()
    }

    /// All captured downloads, oldest first.
    pub async fn downloads(&self) -> Vec<CapturedDownload> {
        self.inner.read().await.downloads.clone()
    }

    /// Dialogs that are still open (no close event yet), the set the agent can
    /// answer with [`crate::Page::handle_user_prompt`].
    pub async fn open_dialogs(&self) -> Vec<CapturedDialog> {
        self.inner
            .read()
            .await
            .dialogs
            .iter()
            .filter(|d| d.accepted.is_none())
            .cloned()
            .collect()
    }

    /// The most recently opened dialog, if any.
    pub async fn last_dialog(&self) -> Option<CapturedDialog> {
        self.inner.read().await.dialogs.last().cloned()
    }

    /// Number of captured dialogs.
    pub async fn dialog_count(&self) -> usize {
        self.inner.read().await.dialogs.len()
    }

    /// Drop all recorded dialogs and downloads.
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.dialogs.clear();
        inner.downloads.clear();
    }

    /// Record a `userPromptOpened` event.
    pub async fn ingest_opened(&self, evt: &UserPromptOpened) {
        let p = &evt.params;
        let dialog = CapturedDialog {
            context: p.context.inner().to_string(),
            kind: prompt_kind(&p.r#type).to_string(),
            message: p.message.clone(),
            default_value: p.default_value.clone(),
            handler: handler_str(&p.handler).to_string(),
            accepted: None,
            user_text: None,
        };
        self.inner.write().await.push_dialog(dialog);
    }

    /// Record a `userPromptClosed` event, finalizing the matching open dialog
    /// (most-recent open prompt in the same context). Falls back to a standalone
    /// record if no open prompt is found (e.g. log started mid-prompt).
    pub async fn ingest_closed(&self, evt: &UserPromptClosed) {
        let p = &evt.params;
        let ctx = p.context.inner().to_string();
        let mut inner = self.inner.write().await;
        if let Some(d) = inner
            .dialogs
            .iter_mut()
            .rev()
            .find(|d| d.context == ctx && d.accepted.is_none())
        {
            d.accepted = Some(p.accepted);
            d.user_text = p.user_text.clone();
            return;
        }
        inner.push_dialog(CapturedDialog {
            context: ctx,
            kind: prompt_kind(&p.r#type).to_string(),
            message: String::new(),
            default_value: None,
            handler: String::new(),
            accepted: Some(p.accepted),
            user_text: p.user_text.clone(),
        });
    }

    /// Record a `downloadWillBegin` event.
    pub async fn ingest_download_begin(&self, evt: &DownloadWillBegin) {
        let p = &evt.params;
        self.inner.write().await.push_download(CapturedDownload {
            context: p.base_navigation_info.context.inner().to_string(),
            suggested_filename: p.suggested_filename.clone(),
            url: p.base_navigation_info.url.clone(),
            status: "will-begin".to_string(),
            filepath: None,
        });
    }

    /// Record a `downloadEnd` event, finalizing the matching in-flight download
    /// (most-recent `will-begin` for the same URL/context).
    pub async fn ingest_download_end(&self, evt: &DownloadEnd) {
        let (ctx, url, status, filepath) = match &evt
            .params
            .download_canceled_params_download_complete_params_union
        {
            DownloadUnion::DownloadCompleteParams(c) => (
                c.base_navigation_info.context.inner().to_string(),
                c.base_navigation_info.url.clone(),
                "complete".to_string(),
                c.filepath.clone(),
            ),
            DownloadUnion::DownloadCanceledParams(c) => (
                c.base_navigation_info.context.inner().to_string(),
                c.base_navigation_info.url.clone(),
                "canceled".to_string(),
                None,
            ),
        };
        let mut inner = self.inner.write().await;
        if let Some(d) = inner
            .downloads
            .iter_mut()
            .rev()
            .find(|d| d.context == ctx && d.url == url && d.status == "will-begin")
        {
            d.status = status;
            d.filepath = filepath;
            return;
        }
        let suggested_filename = filepath
            .as_ref()
            .and_then(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str().map(String::from))
            })
            .unwrap_or_default();
        inner.push_download(CapturedDownload {
            context: ctx,
            suggested_filename,
            url,
            status,
            filepath,
        });
    }
}

/// Build the event handler that feeds `browsingContext.*` dialog and download
/// events into `log`. Mirrors [`crate::network::make_network_handler`].
pub fn make_dialog_handler(
    log: DialogLog,
) -> impl FnMut(Event) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    use rustenium_bidi_definitions::browsing_context::events::BrowsingContextEvent as BCE;
    move |evt| {
        let log = log.clone();
        Box::pin(async move {
            if let Event::BrowsingContext(bce) = evt {
                match bce {
                    BCE::UserPromptOpened(e) => log.ingest_opened(&e).await,
                    BCE::UserPromptClosed(e) => log.ingest_closed(&e).await,
                    BCE::DownloadWillBegin(e) => log.ingest_download_begin(&e).await,
                    BCE::DownloadEnd(e) => log.ingest_download_end(&e).await,
                    _ => {}
                }
            }
        })
    }
}

/// BiDi event identifiers this module subscribes to.
pub const DIALOG_EVENTS: &[&str] = &[
    "browsingContext.userPromptOpened",
    "browsingContext.userPromptClosed",
    "browsingContext.downloadWillBegin",
    "browsingContext.downloadEnd",
];

#[cfg(test)]
mod tests {
    use super::*;
    use rustenium_bidi_definitions::browsing_context::events::{
        UserPromptClosedMethod, UserPromptClosedParams, UserPromptOpenedMethod,
        UserPromptOpenedParams,
    };
    use rustenium_bidi_definitions::browsing_context::types::BrowsingContext;

    fn opened(ctx: &str, kind: UserPromptType, message: &str) -> UserPromptOpened {
        UserPromptOpened {
            method: UserPromptOpenedMethod::UserPromptOpened,
            params: UserPromptOpenedParams {
                context: BrowsingContext::new(ctx),
                handler: UserPromptHandlerType::Ignore,
                message: message.to_string(),
                r#type: kind,
                default_value: None,
            },
        }
    }

    fn closed(
        ctx: &str,
        kind: UserPromptType,
        accepted: bool,
        text: Option<&str>,
    ) -> UserPromptClosed {
        UserPromptClosed {
            method: UserPromptClosedMethod::UserPromptClosed,
            params: UserPromptClosedParams {
                context: BrowsingContext::new(ctx),
                accepted,
                r#type: kind,
                user_text: text.map(str::to_string),
            },
        }
    }

    #[tokio::test]
    async fn captures_alert_message_for_xss_evidence() {
        let log = DialogLog::new();
        log.ingest_opened(&opened("ctx-1", UserPromptType::Alert, "1"))
            .await;
        let dialogs = log.dialogs().await;
        assert_eq!(dialogs.len(), 1);
        assert_eq!(dialogs[0].kind, "alert");
        assert_eq!(dialogs[0].message, "1");
        assert_eq!(dialogs[0].handler, "ignore");
        assert_eq!(dialogs[0].accepted, None);
    }

    #[tokio::test]
    async fn close_finalizes_matching_open_dialog() {
        let log = DialogLog::new();
        log.ingest_opened(&opened("ctx-1", UserPromptType::Prompt, "name?"))
            .await;
        assert_eq!(log.open_dialogs().await.len(), 1);
        log.ingest_closed(&closed(
            "ctx-1",
            UserPromptType::Prompt,
            true,
            Some("admin"),
        ))
        .await;
        let dialogs = log.dialogs().await;
        assert_eq!(dialogs.len(), 1, "close updates, does not append");
        assert_eq!(dialogs[0].accepted, Some(true));
        assert_eq!(dialogs[0].user_text.as_deref(), Some("admin"));
        assert!(log.open_dialogs().await.is_empty());
    }

    #[tokio::test]
    async fn close_without_open_pushes_standalone() {
        let log = DialogLog::new();
        log.ingest_closed(&closed("ctx-9", UserPromptType::Confirm, false, None))
            .await;
        let dialogs = log.dialogs().await;
        assert_eq!(dialogs.len(), 1);
        assert_eq!(dialogs[0].accepted, Some(false));
    }

    #[tokio::test]
    async fn dialogs_are_bounded() {
        let log = DialogLog::new();
        for i in 0..(MAX_ENTRIES + 50) {
            log.ingest_opened(&opened("ctx", UserPromptType::Alert, &i.to_string()))
                .await;
        }
        assert_eq!(log.dialog_count().await, MAX_ENTRIES);
        // Oldest dropped: the most recent message survives.
        let last = log.last_dialog().await.unwrap();
        assert_eq!(last.message, (MAX_ENTRIES + 49).to_string());
    }

    #[test]
    fn prompt_kind_maps_all_variants() {
        assert_eq!(prompt_kind(&UserPromptType::Alert), "alert");
        assert_eq!(prompt_kind(&UserPromptType::Beforeunload), "beforeunload");
        assert_eq!(prompt_kind(&UserPromptType::Confirm), "confirm");
        assert_eq!(prompt_kind(&UserPromptType::Prompt), "prompt");
    }

    #[test]
    fn handler_maps_all_variants() {
        assert_eq!(handler_str(&UserPromptHandlerType::Accept), "accept");
        assert_eq!(handler_str(&UserPromptHandlerType::Dismiss), "dismiss");
        assert_eq!(handler_str(&UserPromptHandlerType::Ignore), "ignore");
        assert_eq!(
            handler_str(&UserPromptHandlerType::DismissAndNotify),
            "dismiss and notify"
        );
    }
}
